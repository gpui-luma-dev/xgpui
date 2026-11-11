//! Releases each frame's custom-content snapshots as soon as the GPU has
//! finished sampling them, without waiting for a later frame (#197 K4).
//!
//! The immediate context signals a D3D11.4 fence after each frame's sampling
//! work; a worker thread waits on the fence and drops that frame's snapshots.
//! Event queries (read only during later frames) remain the fallback for
//! devices without fences.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

type Snapshot = Vec<Arc<dyn gpui::CustomContent>>;

struct EventHandle(HANDLE);
// SAFETY: an event handle is a process-wide kernel object reference; only the
// worker thread waits on it and closes it.
unsafe impl Send for EventHandle {}

pub(super) struct FenceRetirement {
    context: ID3D11DeviceContext4,
    fence: ID3D11Fence,
    next: u64,
    sender: Option<mpsc::Sender<(u64, Snapshot)>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl FenceRetirement {
    /// `None` when the device lacks D3D11.4 fences or the worker cannot start.
    pub(super) fn new(device: &ID3D11Device, context: &ID3D11DeviceContext) -> Option<Self> {
        let device: ID3D11Device5 = device.cast().ok()?;
        let context: ID3D11DeviceContext4 = context.cast().ok()?;
        let mut fence: Option<ID3D11Fence> = None;
        unsafe { device.CreateFence(0, D3D11_FENCE_FLAG_NONE, &mut fence) }.ok()?;
        let fence = fence?;
        let event = EventHandle(unsafe { CreateEventW(None, false, false, None) }.ok()?);
        let (sender, receiver) = mpsc::channel::<(u64, Snapshot)>();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_fence = fence.clone();
        let worker_stop = stop.clone();
        let worker = std::thread::Builder::new()
            .name("gpui-d3d11-retirement".into())
            .spawn(move || {
                let event = event;
                while let Ok((value, snapshot)) = receiver.recv() {
                    while !worker_stop.load(Ordering::Acquire)
                        && unsafe { worker_fence.GetCompletedValue() } < value
                    {
                        if unsafe { worker_fence.SetEventOnCompletion(value, event.0) }.is_err() {
                            break;
                        }
                        // Bounded so device removal or shutdown cannot wedge the worker.
                        unsafe { WaitForSingleObject(event.0, 100) };
                    }
                    drop(snapshot);
                }
                let _ = unsafe { CloseHandle(event.0) };
            })
            .ok()?;
        Some(Self {
            context,
            fence,
            next: 0,
            sender: Some(sender),
            stop,
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
        self.stop.store(true, Ordering::Release);
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
