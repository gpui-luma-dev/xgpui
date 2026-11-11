//! Releases each frame's custom-content snapshots as soon as the GPU has
//! finished sampling them, without waiting for a later frame (#197 K4).
//!
//! The immediate context signals a D3D11.4 fence after each frame's sampling
//! work; a worker thread waits on the fence and drops that frame's snapshots.
//! Event queries (read only during later frames) remain the fallback for
//! devices without fences, or when `GPUI_D3D11_FENCE_RETIREMENT=0`.
//!
//! The worker blocks in the kernel on two events (fence reached, or stop) and
//! calls into the driver only once per frame. On shutdown it releases nothing:
//! queued snapshots are leaked rather than dropped during teardown.
use super::*;
use std::sync::mpsc;
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects};

type Snapshot = Vec<Arc<dyn gpui::CustomContent>>;

/// Kernel event handles shared with the worker.
#[derive(Clone, Copy)]
struct Events {
    fence: HANDLE,
    stop: HANDLE,
}
// SAFETY: event handles are process-wide kernel object references; the
// worker waits on them and the owner closes them after joining the worker.
unsafe impl Send for Events {}

pub(super) struct FenceRetirement {
    context: ID3D11DeviceContext4,
    fence: ID3D11Fence,
    next: u64,
    events: Events,
    sender: Option<mpsc::Sender<(u64, Snapshot)>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

/// Diagnostic switch: `GPUI_D3D11_FENCE_RETIREMENT=0` keeps query retirement.
fn disabled() -> bool {
    std::env::var("GPUI_D3D11_FENCE_RETIREMENT").is_ok_and(|value| value == "0")
}

impl FenceRetirement {
    /// `None` when disabled, the device lacks D3D11.4 fences, or the worker
    /// cannot start.
    pub(super) fn new(device: &ID3D11Device, context: &ID3D11DeviceContext) -> Option<Self> {
        if disabled() {
            log::info!("D3D11 fence retirement disabled; using event queries");
            return None;
        }
        let device: ID3D11Device5 = device.cast().ok()?;
        let context: ID3D11DeviceContext4 = context.cast().ok()?;
        let mut fence: Option<ID3D11Fence> = None;
        unsafe { device.CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence) }.ok()?;
        let fence = fence?;
        let fence_event = unsafe { CreateEventW(None, false, false, None) }.ok()?;
        let stop_event = match unsafe { CreateEventW(None, true, false, None) } {
            Ok(event) => event,
            Err(_) => {
                let _ = unsafe { CloseHandle(fence_event) };
                return None;
            }
        };
        let events = Events {
            fence: fence_event,
            stop: stop_event,
        };
        let (sender, receiver) = mpsc::channel::<(u64, Snapshot)>();
        let worker_fence = fence.clone();
        let worker = std::thread::Builder::new()
            .name("gpui-d3d11-retirement".into())
            .spawn(move || {
                let events = events;
                while let Ok((value, snapshot)) = receiver.recv() {
                    if unsafe { worker_fence.GetCompletedValue() } < value {
                        if unsafe { worker_fence.SetEventOnCompletion(value, events.fence) }
                            .is_err()
                        {
                            std::mem::forget(snapshot);
                            break;
                        }
                        let woke = unsafe {
                            WaitForMultipleObjects(&[events.fence, events.stop], false, INFINITE)
                        };
                        if woke != WAIT_OBJECT_0 {
                            // Stopping: release nothing during teardown.
                            std::mem::forget(snapshot);
                            break;
                        }
                    }
                    drop(snapshot);
                }
                for (_, snapshot) in receiver.try_iter() {
                    std::mem::forget(snapshot);
                }
            });
        let worker = match worker {
            Ok(worker) => worker,
            Err(_) => {
                unsafe {
                    let _ = CloseHandle(fence_event);
                    let _ = CloseHandle(stop_event);
                }
                return None;
            }
        };
        Some(Self {
            context,
            fence,
            next: 0,
            events,
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    /// Signal after the frame's sampling work; the worker drops `snapshot`
    /// once the GPU has passed the signal.
    pub(super) fn retire(&mut self, snapshot: Snapshot) -> Result<()> {
        self.next += 1;
        unsafe { self.context.Signal(&self.fence, self.next)? };
        if let Some(sender) = &self.sender {
            let _ = sender.send((self.next, snapshot));
        }
        Ok(())
    }
}

impl Drop for FenceRetirement {
    fn drop(&mut self) {
        let _ = unsafe { SetEvent(self.events.stop) };
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        unsafe {
            let _ = CloseHandle(self.events.fence);
            let _ = CloseHandle(self.events.stop);
        }
    }
}
