//! The boids engine: wgpu's example on a device and thread of its own,
//! presenting each step into an `EngineSurface` slot.
//!
//! Pacing: the engine steps once per frame the window composites. After each
//! present it waits until the compositor has shown that frame (the surface's
//! `composited` feedback), then steps again. Every step is shown exactly once,
//! at the display's own rate, so motion is even: no frame is dropped unshown
//! and none is shown twice. A timer of its own at a similar rate drifts
//! against vsync and drops a frame every few seconds, which shows as a jump.
//! The time step follows the measured display interval, so the flock moves
//! at the same speed on 60 Hz and 120 Hz displays.
//!
//! While paused the engine blocks on its command channel and does no GPU
//! work; the last presented frame stays on screen because the compositor
//! keeps showing it.
use crate::boids::Boids;
use anyhow::{Context as _, Result};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use xgpui_gpu_interop::{
    AcquireError, EngineSurface, SurfaceDesc, SurfaceFormat, WgpuSurfaceTarget, wgpu,
};

#[cfg(target_os = "macos")]
const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::METAL, "Metal");
#[cfg(target_os = "linux")]
const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::VULKAN, "Vulkan");
#[cfg(target_os = "windows")]
const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::DX12, "D3D12");

/// Slot size: the size wgpu's own boids test renders at (4:3).
pub const WIDTH: u32 = 1024;
pub const HEIGHT: u32 = 768;
/// One window shows the surface: 3 slots, as the crate recommends.
const SLOTS: usize = 3;
/// How long to wait for the window to show a frame before stepping anyway,
/// so the simulation carries on (slowly) while the window is hidden.
const SHOW_TIMEOUT: Duration = Duration::from_millis(100);
/// How often to check the compositor feedback while waiting.
const POLL: Duration = Duration::from_micros(500);

enum Command {
    Pause(bool),
    Particles(u32),
    Speed(f32),
}

/// What the UI reads about the engine.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub adapter: String,
    pub format: String,
    pub particles: u32,
    pub paused: bool,
    /// Simulation steps since start.
    pub steps: u64,
    /// Steps completed in the last second.
    pub steps_per_second: f32,
    /// CPU time to record and present the last step.
    pub record_ms: f32,
    /// Measured interval between composited frames.
    pub display_ms: f32,
    /// Presented frames the window didn't show within `SHOW_TIMEOUT`
    /// (for example while it is hidden); the engine stepped anyway.
    pub show_timeouts: u64,
    /// Steps skipped because every slot was in use.
    pub ring_full: u64,
    pub error: Option<String>,
}

pub struct Engine {
    commands: Sender<Command>,
    status: Arc<Mutex<Status>>,
    surface: EngineSurface,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Start the engine thread and wait until it has a device and a surface.
    pub fn spawn(particles: u32, paused: bool) -> Result<Self> {
        let (commands, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("boids-engine".into())
            .spawn(move || {
                // The device, queue, surface target and simulation are
                // created and used only on this thread.
                let gpu = match Gpu::new(particles) {
                    Ok(gpu) => gpu,
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("{error:#}")));
                        return;
                    }
                };
                let status = Arc::new(Mutex::new(Status {
                    adapter: gpu.adapter.clone(),
                    format: format!("{:?}", gpu.target.format()),
                    particles,
                    paused,
                    ..Default::default()
                }));
                let surface = gpu.target.surface().clone();
                if ready_tx.send(Ok((status.clone(), surface))).is_err() {
                    return;
                }
                run(gpu, receiver, status, paused);
            })?;
        let (status, surface) = ready_rx
            .recv()
            .context("engine thread exited")?
            .map_err(anyhow::Error::msg)?;
        Ok(Self {
            commands,
            status,
            surface,
            thread: Some(thread),
        })
    }

    pub fn surface(&self) -> &EngineSurface {
        &self.surface
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn set_paused(&self, paused: bool) {
        let _ = self.commands.send(Command::Pause(paused));
    }
    /// Scale the simulation's time step: 1 is wgpu's speed at 60 Hz.
    pub fn set_speed(&self, speed: f32) {
        let _ = self.commands.send(Command::Speed(speed));
    }
    /// Start a new flock of `particles` boids.
    pub fn set_particles(&self, particles: u32) {
        let _ = self.commands.send(Command::Particles(particles));
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        // Closing the channel stops the thread.
        let (closed, _) = mpsc::channel();
        self.commands = closed;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Gpu {
    adapter: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: WgpuSurfaceTarget,
    boids: Boids,
}

impl Gpu {
    fn new(particles: u32) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: BACKEND.0,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            gpui::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .with_context(|| format!("no wgpu {} adapter", BACKEND.1))?;
        let info = adapter.get_info();
        let adapter_name = format!("{} ({})", info.name, BACKEND.1);
        // On Linux the device needs the external-memory extensions the
        // surface target shares slots with.
        #[cfg(target_os = "linux")]
        let (device, queue) = WgpuSurfaceTarget::request_device(&adapter)?;
        #[cfg(not(target_os = "linux"))]
        let (device, queue) = gpui::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("boids_engine"),
            required_features: WgpuSurfaceTarget::required_features(),
            required_limits: Boids::required_limits().using_resolution(adapter.limits()),
            ..Default::default()
        }))?;
        // BGRA8 sRGB-encoded slots. The wgpu texture is `Bgra8Unorm` on every
        // platform (there is no sRGB view), so a shader writes encoded
        // values. Boids draws white on black, which is the same either way.
        let target = WgpuSurfaceTarget::new(
            SurfaceDesc {
                width: WIDTH,
                height: HEIGHT,
                format: SurfaceFormat::Bgra8Srgb,
                slots: SLOTS,
            },
            &device,
            &queue,
        )?;
        let boids = Boids::new(&device, target.format(), particles);
        Ok(Self {
            adapter: adapter_name,
            device,
            queue,
            target,
            boids,
        })
    }

    /// Acquire a slot, record one frame into it (stepping the flock when
    /// `step`) and present it. `Ok(false)` when every slot is in use.
    fn frame(&mut self, step: bool) -> Result<bool> {
        // Keep at most one frame of engine work in flight, so a slow GPU
        // lowers the step rate instead of queueing work.
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let slot = match self.target.surface().acquire() {
            Ok(slot) => slot,
            Err(AcquireError::WouldBlock) => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        let texture = self.target.texture(&slot)?;
        let view = texture.create_view(&Default::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.boids.render(&mut encoder, &view, step);
        self.target.present(slot, [encoder.finish()])?;
        Ok(true)
    }
}

/// The engine thread: apply commands, then step once per composited frame
/// until paused.
fn run(mut gpu: Gpu, commands: Receiver<Command>, status: Arc<Mutex<Status>>, mut paused: bool) {
    let mut speed = 1f32;
    let mut recent: VecDeque<Instant> = VecDeque::new();
    let mut steps = 0u64;
    let mut ring_full = 0u64;
    let mut show_timeouts = 0u64;
    // When the last presented frame was presented, until it is shown.
    let mut awaiting: Option<(u64, Instant)> = None;
    let mut last_shown: Option<Instant> = None;
    // Display interval, smoothed; starts at 60 Hz.
    let mut display = Duration::from_secs_f32(1. / 60.);
    // A frame to show without stepping: the first one, and a new flock
    // chosen while paused.
    let mut redraw = true;
    loop {
        // Has the window shown the last frame?
        if let Some((presents, at)) = awaiting {
            let stats = gpu.target.surface().stats();
            let now = Instant::now();
            if stats.composited + stats.unshown >= presents {
                awaiting = None;
                if let Some(last) = last_shown.replace(now) {
                    let interval = now - last;
                    // Ignore gaps (pauses, hidden windows) when learning the
                    // display interval.
                    if interval < Duration::from_millis(50) {
                        display = display.mul_f32(0.9) + interval.mul_f32(0.1);
                    }
                }
            } else if now - at >= SHOW_TIMEOUT {
                awaiting = None;
                last_shown = None;
                show_timeouts += 1;
            }
        }
        // Wait for a command while paused, or briefly while the last frame
        // is still waiting to be shown.
        let command = if paused && !redraw {
            match commands.recv() {
                Ok(command) => Some(command),
                Err(_) => return,
            }
        } else if awaiting.is_some() {
            match commands.recv_timeout(POLL) {
                Ok(command) => Some(command),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match commands.try_recv() {
                Ok(command) => Some(command),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
        };
        if let Some(command) = command {
            match command {
                Command::Speed(new) => speed = new,
                Command::Pause(pause) => {
                    if paused && !pause {
                        // The pause is not a display interval.
                        last_shown = None;
                    }
                    paused = pause;
                }
                Command::Particles(particles) => {
                    gpu.boids = Boids::new(&gpu.device, gpu.target.format(), particles);
                    redraw = true;
                }
            }
            let mut status = status.lock().unwrap();
            status.paused = paused;
            status.particles = gpu.boids.particles();
            if paused {
                recent.clear();
                status.steps_per_second = 0.;
            }
            continue;
        }
        let step = !paused;
        if step {
            // Advance by one display interval of simulated time: wgpu's
            // deltaT at 60 Hz, scaled by the speed.
            let delta_t = Boids::DELTA_T * speed * display.as_secs_f32() * 60.;
            gpu.boids.set_delta_t(&gpu.queue, delta_t);
        }
        let started = Instant::now();
        let result = gpu.frame(step);
        let record_ms = started.elapsed().as_secs_f32() * 1000.;
        let mut status = status.lock().unwrap();
        match &result {
            Ok(true) => {
                redraw = false;
                status.error = None;
                awaiting = Some((gpu.target.surface().stats().presents, started));
                if step {
                    steps += 1;
                    recent.push_back(started);
                }
            }
            Ok(false) => {
                // Every slot is in use; the compositor returns one once it
                // stops sampling it.
                ring_full += 1;
                drop(status);
                std::thread::sleep(POLL);
                continue;
            }
            Err(error) => {
                // A lost device or invalidated surface: stop stepping and
                // keep the last frame on screen.
                status.error = Some(format!("{error:#}"));
                paused = true;
                redraw = false;
                status.paused = true;
            }
        }
        while recent
            .front()
            .is_some_and(|at| started.duration_since(*at) > Duration::from_secs(1))
        {
            recent.pop_front();
        }
        status.steps = steps;
        status.steps_per_second = recent.len() as f32;
        status.record_ms = record_ms;
        status.display_ms = display.as_secs_f32() * 1000.;
        status.ring_full = ring_full;
        status.show_timeouts = show_timeouts;
    }
}
