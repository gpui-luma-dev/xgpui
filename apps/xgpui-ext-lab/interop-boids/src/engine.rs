//! The boids engine: wgpu's example on a wgpu device of its own, presenting
//! each step into an `EngineSurface` slot.
//!
//! The same simulation (device, surface and flock) can run in two places,
//! switchable at any time without losing the flock:
//!
//! - **Its own thread** (default): the UI never waits for it. A heavy step
//!   slows only the flock; controls and resizing stay at the display rate.
//! - **The UI thread**, for contrast: each UI frame records a step and waits
//!   for the previous one's GPU work, the way an engine embedded in the UI
//!   would. A heavy step then stretches every UI frame.
//!
//! Pacing: the engine steps once per frame the window composites. On its own
//! thread, after each present it waits until the compositor has shown that
//! frame (the surface's `composited` feedback), then steps again. Every step
//! is shown exactly once, at the display's rate, so motion is even. A timer
//! of its own at a similar rate drifts against vsync and drops a frame every
//! few seconds, which shows as a jump. The time step follows the measured
//! display interval, so the flock moves at the same speed on 60 Hz and
//! 120 Hz displays.
//!
//! Presentation adapts to the step's GPU time, as the crate's heavy-engine
//! guidance suggests (on its own thread; on the UI thread it is always
//! observe-only). Short steps present with a GPU-side wait, so the next
//! UI frame shows them. Once steps take longer than the display's refresh
//! interval (`Window::refresh_interval`, passed in by the view), the
//! engine switches to observe-only presentation (`set_gpu_wait(false)`): the
//! compositor never waits on the GPU for an engine frame, and shows each one
//! once it is complete. With GPU waits, a 50 ms step held up the UI's own
//! frames too; with observe-only, a 3 ms step was sometimes not ready for the
//! UI frame that its present triggered, and took a second one.
//!
//! While paused the engine does no GPU work; the last presented frame stays
//! on screen because the compositor keeps showing it.
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
/// Step GPU time, as a fraction of the display's refresh interval, above which
/// presentation turns observe-only, and below which it returns to GPU waits
/// (hysteresis).
const OBSERVE_ONLY_ABOVE: f32 = 1.;
const GPU_WAIT_BELOW: f32 = 0.75;
/// Steps not measured after a new flock: the first ones include pipeline
/// warm-up.
const WARM_UP_STEPS: u32 = 5;

enum Command {
    Pause(bool),
    Particles(u32),
    Speed(f32),
    Refresh(Duration),
    /// Hand the simulation back to the UI thread.
    Stop,
}

/// What the UI reads about the engine.
#[derive(Clone, Debug, Default)]
pub struct Status {
    pub adapter: String,
    pub format: String,
    pub particles: u32,
    pub paused: bool,
    pub threaded: bool,
    /// Simulation steps since start.
    pub steps: u64,
    /// Steps completed in the last second.
    pub steps_per_second: f32,
    /// CPU time to record and present the last step, including the wait for
    /// the previous step's GPU work.
    pub record_ms: f32,
    /// Measured interval between composited frames (the step interval).
    pub display_ms: f32,
    /// The display's refresh interval, as the window reports it.
    pub refresh_ms: f32,
    /// GPU time per step, smoothed (measured on the engine's own thread).
    pub gpu_ms: f32,
    /// Whether presentation is observe-only (otherwise GPU waits).
    pub observe_only: bool,
    /// Presented frames the window didn't show within `SHOW_TIMEOUT`
    /// (for example while it is hidden); the engine stepped anyway.
    pub show_timeouts: u64,
    /// Steps skipped because every slot was in use.
    pub ring_full: u64,
    pub error: Option<String>,
}

enum Mode {
    Threaded {
        commands: Sender<Command>,
        thread: JoinHandle<Box<Sim>>,
    },
    Inline(Box<Sim>),
    /// Only while switching.
    Moving,
}

pub struct Engine {
    mode: Mode,
    status: Arc<Mutex<Status>>,
    surface: EngineSurface,
}

impl Engine {
    /// Create the engine's device and surface, on its own thread when
    /// `threaded`.
    pub fn new(particles: u32, threaded: bool) -> Result<Self> {
        let sim = Box::new(Sim::new(particles)?);
        let status = sim.status.clone();
        let surface = sim.gpu.target.surface().clone();
        let mut sim = sim;
        sim.moved(false);
        let mut engine = Self {
            mode: Mode::Inline(sim),
            status,
            surface,
        };
        engine.set_threaded(threaded)?;
        Ok(engine)
    }

    pub fn surface(&self) -> &EngineSurface {
        &self.surface
    }
    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }
    pub fn threaded(&self) -> bool {
        matches!(self.mode, Mode::Threaded { .. })
    }

    fn send(&mut self, command: Command) {
        match &mut self.mode {
            Mode::Threaded { commands, .. } => {
                let _ = commands.send(command);
            }
            Mode::Inline(sim) => sim.apply(command),
            Mode::Moving => {}
        }
    }
    pub fn set_paused(&mut self, paused: bool) {
        self.send(Command::Pause(paused));
    }
    /// Scale the simulation's time step: 1 is wgpu's speed at 60 Hz.
    pub fn set_speed(&mut self, speed: f32) {
        self.send(Command::Speed(speed));
    }
    /// The display's refresh interval, which sets when steps are long
    /// enough to present observe-only.
    pub fn set_refresh_interval(&mut self, refresh: Duration) {
        self.send(Command::Refresh(refresh));
    }
    /// Start a new flock of `particles` boids.
    pub fn set_particles(&mut self, particles: u32) {
        self.send(Command::Particles(particles));
    }

    /// Move the simulation to its own thread or onto the UI thread. The
    /// device, surface and flock move with it.
    pub fn set_threaded(&mut self, threaded: bool) -> Result<()> {
        if threaded == self.threaded() {
            return Ok(());
        }
        let mut sim = match std::mem::replace(&mut self.mode, Mode::Moving) {
            Mode::Inline(sim) => sim,
            Mode::Threaded { commands, thread } => {
                let _ = commands.send(Command::Stop);
                thread
                    .join()
                    .map_err(|_| anyhow::anyhow!("engine thread panicked"))?
            }
            Mode::Moving => unreachable!(),
        };
        sim.moved(threaded);
        self.mode = if threaded {
            let (commands, receiver) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("boids-engine".into())
                .spawn(move || {
                    let mut sim = sim;
                    sim.run(receiver);
                    sim
                })?;
            Mode::Threaded { commands, thread }
        } else {
            Mode::Inline(sim)
        };
        Ok(())
    }

    /// On the UI thread, record and present one step; call once per UI
    /// frame. Returns whether the engine needs another frame.
    pub fn step_inline(&mut self) -> bool {
        match &mut self.mode {
            Mode::Inline(sim) => {
                if sim.paused && !sim.redraw {
                    return false;
                }
                sim.note_shown();
                sim.advance();
                !sim.paused
            }
            _ => false,
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        if let Mode::Threaded { commands, thread } = std::mem::replace(&mut self.mode, Mode::Moving)
        {
            let _ = commands.send(Command::Stop);
            let _ = thread.join();
        }
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    target: WgpuSurfaceTarget,
    boids: Boids,
    /// When the last step was submitted, until its GPU work is done.
    submitted: Option<Instant>,
    /// Set by the queue when the last step's GPU work is done.
    done: Arc<Mutex<Option<Instant>>>,
    /// Step GPU time, smoothed, in ms.
    gpu_ms: f32,
    /// Steps left before measuring (`WARM_UP_STEPS`).
    unmeasured: u32,
    observe_only: bool,
}

impl Gpu {
    fn new(particles: u32) -> Result<(Self, String)> {
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
        Ok((
            Self {
                device,
                queue,
                target,
                boids,
                submitted: None,
                done: Arc::default(),
                gpu_ms: 0.,
                unmeasured: WARM_UP_STEPS,
                observe_only: false,
            },
            adapter_name,
        ))
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
        *self.done.lock().unwrap() = None;
        self.submitted = Some(Instant::now());
        self.target.present(slot, [encoder.finish()])?;
        let done = self.done.clone();
        self.queue.on_submitted_work_done(move || {
            *done.lock().unwrap() = Some(Instant::now());
        });
        Ok(true)
    }

    /// Learn the last step's GPU time once it is done, and pick the
    /// presentation mode from it. Accurate only while polled often, as the
    /// engine thread does while it waits for the compositor.
    fn measure_gpu(&mut self, refresh: Duration) {
        let _ = self.device.poll(wgpu::PollType::Poll);
        let (Some(submitted), Some(done)) = (self.submitted, *self.done.lock().unwrap()) else {
            return;
        };
        self.submitted = None;
        if self.unmeasured > 0 {
            self.unmeasured -= 1;
            return;
        }
        let ms = (done - submitted).as_secs_f32() * 1000.;
        self.gpu_ms = if self.gpu_ms == 0. {
            ms
        } else {
            self.gpu_ms * 0.7 + ms * 0.3
        };
        let frame_ms = refresh.as_secs_f32() * 1000.;
        let observe_only = if self.observe_only {
            self.gpu_ms > GPU_WAIT_BELOW * frame_ms
        } else {
            self.gpu_ms > OBSERVE_ONLY_ABOVE * frame_ms
        };
        self.set_observe_only(observe_only);
    }

    fn set_observe_only(&mut self, observe_only: bool) {
        if observe_only != self.observe_only {
            self.observe_only = observe_only;
            self.target.set_gpu_wait(!observe_only);
        }
    }
}

/// The simulation and its bookkeeping, owned by whichever thread runs it.
struct Sim {
    gpu: Gpu,
    status: Arc<Mutex<Status>>,
    paused: bool,
    speed: f32,
    /// A frame to show without stepping: the first one, and a new flock
    /// chosen while paused.
    redraw: bool,
    steps: u64,
    recent: VecDeque<Instant>,
    ring_full: u64,
    show_timeouts: u64,
    /// The present count and time of the last frame, until it is shown.
    awaiting: Option<(u64, Instant)>,
    last_shown: Option<Instant>,
    /// Display interval, smoothed; starts at 60 Hz.
    display: Duration,
    /// The display's refresh interval, from the window
    /// (`Window::refresh_interval`). A slow engine is shown less often than
    /// the display refreshes, so `display` can't stand in for it.
    refresh: Duration,
}

/// What `advance` did.
enum Advanced {
    Presented,
    RingFull,
    Failed,
}

impl Sim {
    fn new(particles: u32) -> Result<Self> {
        let (gpu, adapter) = Gpu::new(particles)?;
        let status = Arc::new(Mutex::new(Status {
            adapter,
            format: format!("{:?}", gpu.target.format()),
            particles,
            ..Default::default()
        }));
        Ok(Self {
            gpu,
            status,
            paused: false,
            speed: 1.,
            redraw: true,
            steps: 0,
            recent: VecDeque::new(),
            ring_full: 0,
            show_timeouts: 0,
            awaiting: None,
            last_shown: None,
            display: Duration::from_secs_f32(1. / 60.),
            refresh: Duration::from_secs_f32(1. / 60.),
        })
    }

    /// The simulation changed threads: forget timing that spans the move.
    fn moved(&mut self, threaded: bool) {
        self.awaiting = None;
        self.last_shown = None;
        self.recent.clear();
        if !threaded {
            // On the UI thread the step's GPU time can't be measured (the
            // queue is polled once per UI frame), so always present
            // observe-only, as a heavy engine would. Every UI frame still
            // shows one new step, one frame after it was recorded. On its own
            // thread the engine measures again and adapts.
            self.gpu.set_observe_only(true);
            self.gpu.submitted = None;
        }
        let mut status = self.status.lock().unwrap();
        status.threaded = threaded;
        status.observe_only = self.gpu.observe_only;
    }

    fn apply(&mut self, command: Command) {
        match command {
            Command::Speed(speed) => self.speed = speed,
            Command::Refresh(refresh) => self.refresh = refresh,
            Command::Pause(pause) => {
                if self.paused && !pause {
                    // The pause is not a display interval.
                    self.last_shown = None;
                }
                self.paused = pause;
            }
            Command::Particles(particles) => {
                self.gpu.boids = Boids::new(&self.gpu.device, self.gpu.target.format(), particles);
                self.gpu.gpu_ms = 0.;
                self.gpu.unmeasured = WARM_UP_STEPS;
                self.redraw = true;
            }
            Command::Stop => {}
        }
        let mut status = self.status.lock().unwrap();
        status.paused = self.paused;
        status.particles = self.gpu.boids.particles();
        if self.paused {
            self.recent.clear();
            status.steps_per_second = 0.;
        }
    }

    /// Check whether the window has shown the last frame, and learn the
    /// display interval from when it did.
    fn note_shown(&mut self) {
        let Some((presents, at)) = self.awaiting else {
            return;
        };
        let stats = self.gpu.target.surface().stats();
        let now = Instant::now();
        if stats.composited + stats.unshown >= presents {
            self.awaiting = None;
            if let Some(last) = self.last_shown.replace(now) {
                let interval = now - last;
                // Ignore gaps (pauses, hidden windows, slow steps) when
                // learning the display interval.
                if interval < Duration::from_millis(50) {
                    self.display = self.display.mul_f32(0.9) + interval.mul_f32(0.1);
                }
            }
        } else if now - at >= SHOW_TIMEOUT {
            self.awaiting = None;
            self.last_shown = None;
            self.show_timeouts += 1;
        }
    }

    /// Record and present one frame: a step unless paused, otherwise a
    /// redraw of the current flock.
    fn advance(&mut self) -> Advanced {
        let step = !self.paused;
        if step {
            // Advance by one display interval of simulated time: wgpu's
            // deltaT at 60 Hz, scaled by the speed.
            let delta_t = Boids::DELTA_T * self.speed * self.display.as_secs_f32() * 60.;
            self.gpu.boids.set_delta_t(&self.gpu.queue, delta_t);
        }
        let started = Instant::now();
        let result = self.gpu.frame(step);
        let record_ms = started.elapsed().as_secs_f32() * 1000.;
        let mut status = self.status.lock().unwrap();
        let advanced = match result {
            Ok(true) => {
                self.redraw = false;
                status.error = None;
                self.awaiting = Some((self.gpu.target.surface().stats().presents, started));
                if step {
                    self.steps += 1;
                    self.recent.push_back(started);
                }
                Advanced::Presented
            }
            Ok(false) => {
                // Every slot is in use; the compositor returns one once it
                // stops sampling it.
                self.ring_full += 1;
                Advanced::RingFull
            }
            Err(error) => {
                // A lost device or invalidated surface: stop stepping and
                // keep the last frame on screen.
                status.error = Some(format!("{error:#}"));
                self.paused = true;
                self.redraw = false;
                status.paused = true;
                Advanced::Failed
            }
        };
        while self
            .recent
            .front()
            .is_some_and(|at| started.duration_since(*at) > Duration::from_secs(1))
        {
            self.recent.pop_front();
        }
        status.steps = self.steps;
        status.steps_per_second = self.recent.len() as f32;
        status.record_ms = record_ms;
        status.display_ms = self.display.as_secs_f32() * 1000.;
        status.refresh_ms = self.refresh.as_secs_f32() * 1000.;
        status.gpu_ms = self.gpu.gpu_ms;
        status.observe_only = self.gpu.observe_only;
        status.ring_full = self.ring_full;
        status.show_timeouts = self.show_timeouts;
        advanced
    }

    /// On its own thread: apply commands, then step once per composited
    /// frame until paused. Returns on `Stop` or when the UI side is gone.
    fn run(&mut self, commands: Receiver<Command>) {
        loop {
            self.gpu.measure_gpu(self.refresh);
            self.note_shown();
            // Wait for a command while paused, or briefly while the last
            // frame is still waiting to be shown.
            let command = if self.paused && !self.redraw {
                match commands.recv() {
                    Ok(command) => Some(command),
                    Err(_) => return,
                }
            } else if self.awaiting.is_some() {
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
            match command {
                Some(Command::Stop) => return,
                Some(command) => self.apply(command),
                None => {
                    if let Advanced::RingFull = self.advance() {
                        std::thread::sleep(POLL);
                    }
                }
            }
        }
    }
}
