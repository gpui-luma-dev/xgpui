use crate::MIB;
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

/// Shared limits for accounted image memory, not a hard process/GPU-memory cap.
#[derive(Clone, Copy, Debug)]
pub struct SchedulerLimits {
    pub max_jobs: usize,
    pub max_bytes: usize,
}
impl Default for SchedulerLimits {
    fn default() -> Self {
        Self {
            max_jobs: 2,
            max_bytes: 128 * MIB,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SchedulerStats {
    pub members: usize,
    pub active_jobs: usize,
    pub queued_jobs: usize,
    pub retained_bytes: usize,
    pub in_flight_bytes: usize,
    pub managed_bytes: usize,
    pub peak_managed_bytes: usize,
    pub peak_active_jobs: usize,
}
/// Clone one pool into every SvgImage in a gallery. Limits are immutable.
/// Retained RGBA is charged twice (CPU plus nominal GPU); a job reserves three
/// times RGBA for output/conversion/upload. Filter intermediates, atlas packing,
/// parsed trees and allocator/driver retention are outside these estimates.
#[derive(Clone)]
pub struct SvgScheduler(Arc<Mutex<Pool>>);
struct Pool {
    limits: SchedulerLimits,
    next_id: u64,
    members: BTreeSet<u64>,
    revision: u64,
    changed: HashMap<u64, Waker>,
    waiting: VecDeque<(u64, u64)>,
    waiters: HashMap<u64, Waker>,
    stats: SchedulerStats,
}
impl Default for SvgScheduler {
    fn default() -> Self {
        Self::new(SchedulerLimits::default()).expect("valid defaults")
    }
}
impl SvgScheduler {
    pub fn new(limits: SchedulerLimits) -> Result<Self> {
        ensure!(
            (1..=64).contains(&limits.max_jobs),
            "scheduler max_jobs must be 1..=64"
        );
        ensure!(
            (64 * 1024..=1024 * MIB).contains(&limits.max_bytes),
            "scheduler max_bytes must be 64 KiB..=1 GiB"
        );
        Ok(Self(Arc::new(Mutex::new(Pool {
            limits,
            next_id: 0,
            members: BTreeSet::new(),
            revision: 0,
            changed: HashMap::new(),
            waiting: VecDeque::new(),
            waiters: HashMap::new(),
            stats: SchedulerStats::default(),
        }))))
    }
    pub fn limits(&self) -> SchedulerLimits {
        self.0.lock().unwrap().limits
    }
    pub fn stats(&self) -> SchedulerStats {
        let p = self.0.lock().unwrap();
        SchedulerStats {
            members: p.members.len(),
            queued_jobs: p.waiting.len(),
            ..p.stats
        }
    }
    pub(crate) fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
    pub(crate) fn join(&self) -> Client {
        let (id, wakes) = {
            let mut p = self.0.lock().unwrap();
            p.next_id += 1;
            let id = p.next_id;
            p.members.insert(id);
            p.revision += 1;
            (id, p.changed.drain().map(|(_, w)| w).collect::<Vec<_>>())
        };
        wake(wakes);
        Client {
            scheduler: self.clone(),
            id,
        }
    }
}
fn wake(wakers: Vec<Waker>) {
    for w in wakers {
        w.wake();
    }
}
fn waiters(p: &Pool) -> Vec<Waker> {
    p.waiters.values().cloned().collect()
}
pub(crate) struct Client {
    scheduler: SvgScheduler,
    id: u64,
}
impl Client {
    pub fn scheduler(&self) -> &SvgScheduler {
        &self.scheduler
    }
    pub fn output_budget(&self) -> usize {
        let p = self.scheduler.0.lock().unwrap();
        // Leave room for all current images plus max_jobs replacements.
        (p.limits.max_bytes / (2 * p.members.len() + 3 * p.limits.max_jobs)).max(4) / 4 * 4
    }
    pub fn revision(&self) -> u64 {
        self.scheduler.0.lock().unwrap().revision
    }
    pub fn changed(&self, revision: u64) -> Changed {
        Changed {
            scheduler: self.scheduler.clone(),
            id: self.id,
            revision,
        }
    }
    pub fn acquire(&self, bytes: usize) -> Acquire {
        let id = {
            let mut p = self.scheduler.0.lock().unwrap();
            p.next_id += 1;
            p.next_id
        };
        Acquire {
            scheduler: self.scheduler.clone(),
            id,
            client: self.id,
            bytes,
            registered: false,
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let wakes = {
            let mut p = self.scheduler.0.lock().unwrap();
            p.members.remove(&self.id);
            let tickets: Vec<_> = p
                .waiting
                .iter()
                .filter(|(_, client)| *client == self.id)
                .map(|(id, _)| *id)
                .collect();
            p.waiting.retain(|(_, client)| *client != self.id);
            for id in tickets {
                p.waiters.remove(&id);
            }
            p.changed.remove(&self.id);
            p.revision += 1;
            let mut wakes = waiters(&p);
            wakes.extend(p.changed.drain().map(|(_, w)| w));
            wakes
        };
        wake(wakes);
    }
}
pub(crate) struct Changed {
    scheduler: SvgScheduler,
    id: u64,
    revision: u64,
}
impl Future for Changed {
    type Output = u64;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u64> {
        let mut p = self.scheduler.0.lock().unwrap();
        if p.revision != self.revision {
            p.changed.remove(&self.id);
            Poll::Ready(p.revision)
        } else {
            p.changed.insert(self.id, cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Drop for Changed {
    fn drop(&mut self) {
        self.scheduler.0.lock().unwrap().changed.remove(&self.id);
    }
}
pub(crate) struct Acquire {
    scheduler: SvgScheduler,
    id: u64,
    client: u64,
    bytes: usize,
    registered: bool,
}
impl Future for Acquire {
    type Output = Reservation;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Reservation> {
        let this = self.get_mut();
        let mut p = this.scheduler.0.lock().unwrap();
        if !this.registered {
            p.waiting.push_back((this.id, this.client));
            this.registered = true;
        }
        let charge = this.bytes.saturating_mul(3);
        if p.waiting.front().map(|(id, _)| *id) == Some(this.id)
            && p.stats.active_jobs < p.limits.max_jobs
            && charge <= p.limits.max_bytes.saturating_sub(p.stats.managed_bytes)
        {
            p.waiting.pop_front();
            p.waiters.remove(&this.id);
            this.registered = false;
            p.stats.active_jobs += 1;
            p.stats.in_flight_bytes += charge;
            p.stats.managed_bytes += charge;
            p.stats.peak_active_jobs = p.stats.peak_active_jobs.max(p.stats.active_jobs);
            p.stats.peak_managed_bytes = p.stats.peak_managed_bytes.max(p.stats.managed_bytes);
            let wakes = waiters(&p);
            drop(p);
            wake(wakes);
            Poll::Ready(Reservation {
                scheduler: this.scheduler.clone(),
                charge,
                active: true,
            })
        } else {
            p.waiters.insert(this.id, cx.waker().clone());
            Poll::Pending
        }
    }
}
impl Drop for Acquire {
    fn drop(&mut self) {
        if self.registered {
            let wakes = {
                let mut p = self.scheduler.0.lock().unwrap();
                p.waiting.retain(|&(id, _)| id != self.id);
                p.waiters.remove(&self.id);
                waiters(&p)
            };
            wake(wakes);
        }
    }
}
/// Moves into the synchronous worker so cancelling its host never frees live work.
pub(crate) struct Reservation {
    scheduler: SvgScheduler,
    charge: usize,
    active: bool,
}
impl Reservation {
    pub fn retain(mut self) -> Self {
        let wakes = {
            let mut p = self.scheduler.0.lock().unwrap();
            p.stats.active_jobs -= 1;
            p.stats.in_flight_bytes -= self.charge;
            let retained = self.charge / 3 * 2;
            p.stats.managed_bytes -= self.charge - retained;
            p.stats.retained_bytes += retained;
            self.charge = retained;
            self.active = false;
            waiters(&p)
        };
        wake(wakes);
        self
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let wakes = {
            let mut p = self.scheduler.0.lock().unwrap();
            p.stats.managed_bytes -= self.charge;
            if self.active {
                p.stats.active_jobs -= 1;
                p.stats.in_flight_bytes -= self.charge;
            } else {
                p.stats.retained_bytes -= self.charge;
            }
            waiters(&p)
        };
        wake(wakes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn poll(f: &mut Acquire) -> Poll<Reservation> {
        Pin::new(f).poll(&mut Context::from_waker(Waker::noop()))
    }
    fn ready(f: &mut Acquire) -> Reservation {
        match poll(f) {
            Poll::Ready(r) => r,
            _ => panic!("expected admission"),
        }
    }
    #[test]
    fn fifo_concurrency_and_cancelled_waiters() {
        let pool = SvgScheduler::new(SchedulerLimits {
            max_jobs: 1,
            max_bytes: 64 * 1024,
        })
        .unwrap();
        let a = pool.join();
        let b = pool.join();
        let c = pool.join();
        let first = ready(&mut a.acquire(1024));
        let mut second = b.acquire(1024);
        assert!(poll(&mut second).is_pending());
        let mut third = c.acquire(1024);
        assert!(poll(&mut third).is_pending());
        drop(first);
        assert!(poll(&mut third).is_pending()); // cannot jump FIFO
        drop(second);
        let last = ready(&mut third);
        assert_eq!(pool.stats().active_jobs, 1);
        drop(last);
        assert_eq!(pool.stats().managed_bytes, 0);
        assert_eq!(pool.stats().peak_active_jobs, 1);
    }
    #[test]
    fn cancelling_an_old_ticket_cannot_remove_a_new_request_for_the_same_client() {
        let pool = SvgScheduler::new(SchedulerLimits {
            max_jobs: 1,
            max_bytes: 64 * 1024,
        })
        .unwrap();
        let client = pool.join();
        let active = ready(&mut client.acquire(1024));
        let mut old = client.acquire(1024);
        assert!(poll(&mut old).is_pending());
        let mut new = client.acquire(1024);
        assert!(poll(&mut new).is_pending());
        drop(old);
        drop(active);
        let admitted = ready(&mut new);
        drop(admitted);
        assert_eq!(pool.stats().managed_bytes, 0);
        assert_eq!(pool.stats().queued_jobs, 0);
    }
    #[test]
    fn memory_accounts_replacements_and_retained_gpu_estimate() {
        let pool = SvgScheduler::new(SchedulerLimits {
            max_jobs: 2,
            max_bytes: 64 * 1024,
        })
        .unwrap();
        let a = pool.join();
        let b = pool.join();
        let retained = ready(&mut a.acquire(12 * 1024)).retain();
        assert_eq!(pool.stats().retained_bytes, 24 * 1024);
        let work = ready(&mut b.acquire(12 * 1024));
        assert_eq!(pool.stats().managed_bytes, 60 * 1024);
        let mut replacement = a.acquire(12 * 1024);
        assert!(poll(&mut replacement).is_pending());
        drop(work);
        let replacement = ready(&mut replacement);
        assert_eq!(pool.stats().peak_managed_bytes, 60 * 1024);
        drop(a); // unmount cannot release its running worker's reservation
        assert_eq!(pool.stats().active_jobs, 1);
        drop(retained);
        drop(replacement);
        assert_eq!(pool.stats().managed_bytes, 0);
        assert_eq!(pool.stats().active_jobs, 0);
    }
    #[test]
    fn fair_shares_leave_replacement_headroom_and_membership_wakes() {
        let pool = SvgScheduler::default();
        let a = pool.join();
        let old = a.output_budget();
        let rev = a.revision();
        let mut changed = a.changed(rev);
        assert!(
            Pin::new(&mut changed)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        let b = pool.join();
        assert!(
            Pin::new(&mut changed)
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_ready()
        );
        let quota = a.output_budget();
        assert!(quota < old);
        assert!(2 * 2 * quota + 3 * pool.limits().max_jobs * quota <= pool.limits().max_bytes);
        drop(b);
        assert_eq!(a.output_budget(), old);
        assert!(
            SvgScheduler::new(SchedulerLimits {
                max_jobs: 0,
                max_bytes: MIB
            })
            .is_err()
        );
        assert!(
            SvgScheduler::new(SchedulerLimits {
                max_jobs: 1,
                max_bytes: 1
            })
            .is_err()
        );
    }
}
