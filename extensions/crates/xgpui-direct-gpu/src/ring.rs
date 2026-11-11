//! Platform-neutral slot ownership: free → writing → published → free.
//!
//! A published slot is held by `Arc<Lease>` clones: the surface's latest frame
//! and every content snapshot the renderer retains. xgpui backends drop content
//! snapshots only after the compositions that sampled them have completed on
//! the GPU (issue #198, spike 0), so the last `Lease` drop marks the slot
//! reusable. That drop can happen on a GPU completion thread; it only takes a
//! short lock.
use crate::SurfaceStats;
use std::sync::{Arc, Mutex, Weak};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Slot {
    Free,
    Writing,
    Published,
}

#[derive(Debug)]
struct State {
    slots: Vec<Slot>,
    presents: u64,
    would_block: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct Ring(Arc<Mutex<State>>);

/// Exclusive write access to one slot. Dropping it unpublished frees the slot.
#[derive(Debug)]
pub(crate) struct Writing {
    ring: Arc<Mutex<State>>,
    index: usize,
    published: bool,
}

/// Shared read ownership of a published slot.
#[derive(Debug)]
pub(crate) struct Lease {
    ring: Weak<Mutex<State>>,
    index: usize,
}

impl Ring {
    pub fn new(slots: usize) -> Self {
        Self(Arc::new(Mutex::new(State {
            slots: vec![Slot::Free; slots],
            presents: 0,
            would_block: 0,
        })))
    }

    /// Lowest free slot, or `None` (counted) when every slot is in use.
    pub fn acquire(&self) -> Option<Writing> {
        let mut state = self.0.lock().unwrap();
        match state.slots.iter().position(|slot| *slot == Slot::Free) {
            Some(index) => {
                state.slots[index] = Slot::Writing;
                Some(Writing {
                    ring: self.0.clone(),
                    index,
                    published: false,
                })
            }
            None => {
                state.would_block += 1;
                None
            }
        }
    }

    /// Publish a written slot; it stays published while any lease clone lives.
    pub fn publish(&self, mut writing: Writing) -> Arc<Lease> {
        assert!(
            Arc::ptr_eq(&self.0, &writing.ring),
            "slot belongs to another surface"
        );
        let mut state = self.0.lock().unwrap();
        debug_assert_eq!(state.slots[writing.index], Slot::Writing);
        state.slots[writing.index] = Slot::Published;
        state.presents += 1;
        writing.published = true;
        Arc::new(Lease {
            ring: Arc::downgrade(&self.0),
            index: writing.index,
        })
    }

    pub fn stats(&self) -> SurfaceStats {
        let state = self.0.lock().unwrap();
        let count = |kind| state.slots.iter().filter(|slot| **slot == kind).count();
        SurfaceStats {
            free: count(Slot::Free),
            writing: count(Slot::Writing),
            published: count(Slot::Published),
            presents: state.presents,
            would_block: state.would_block,
        }
    }

    pub fn owns(&self, writing: &Writing) -> bool {
        Arc::ptr_eq(&self.0, &writing.ring)
    }
}

impl Writing {
    pub fn index(&self) -> usize {
        self.index
    }
}
impl Drop for Writing {
    fn drop(&mut self) {
        if !self.published {
            let mut state = self.ring.lock().unwrap();
            state.slots[self.index] = Slot::Free;
        }
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(ring) = self.ring.upgrade() {
            let mut state = ring.lock().unwrap();
            state.slots[self.index] = Slot::Free;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_until_full_then_would_block() {
        let ring = Ring::new(2);
        let a = ring.acquire().unwrap();
        let b = ring.acquire().unwrap();
        assert_ne!(a.index(), b.index());
        assert!(ring.acquire().is_none());
        assert_eq!(ring.stats().would_block, 1);
        drop(a);
        assert!(ring.acquire().is_some(), "dropped unpublished slot is free");
    }

    #[test]
    fn published_slot_returns_only_after_last_lease() {
        let ring = Ring::new(2);
        let lease = ring.publish(ring.acquire().unwrap());
        let retained_scene = lease.clone();
        let _writing = ring.acquire().unwrap();
        assert!(
            ring.acquire().is_none(),
            "published slot must not be reused"
        );
        drop(lease);
        assert!(ring.acquire().is_none(), "a retained scene still holds it");
        drop(retained_scene);
        assert!(ring.acquire().is_some());
        let stats = ring.stats();
        assert_eq!(stats.presents, 1);
    }

    #[test]
    fn lease_outliving_the_ring_is_harmless() {
        let ring = Ring::new(2);
        let lease = ring.publish(ring.acquire().unwrap());
        drop(ring);
        drop(lease);
    }

    #[test]
    fn lease_drop_from_another_thread() {
        let ring = Ring::new(2);
        let lease = ring.publish(ring.acquire().unwrap());
        std::thread::spawn(move || drop(lease)).join().unwrap();
        assert_eq!(ring.stats().free, 2);
    }

    #[test]
    #[should_panic(expected = "another surface")]
    fn rejects_foreign_slots() {
        let a = Ring::new(2);
        let b = Ring::new(2);
        let writing = b.acquire().unwrap();
        a.publish(writing);
    }
}
