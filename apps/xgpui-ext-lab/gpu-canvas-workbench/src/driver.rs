//! Runs the workbench engine either on its own thread (default) or on the UI
//! thread (`--ui-thread-engine`, for comparison), behind one interface.
//!
//! On its own thread the engine receives edits as commands, applies and
//! publishes them as they arrive, recovers from device loss by itself, and
//! shares a status snapshot with the UI. The UI never waits for the engine:
//! it places the surface's latest published frame, and polls the status once
//! per display frame while work is in flight.
use crate::check::percentile;
use crate::engine::{Engine, EngineStats, Inspection};
use crate::oracle::Dab;
use anyhow::{Result, anyhow};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use xgpui_surface::EngineSurface;

/// How long a lost engine keeps showing its last frame before rebuilding,
/// and between rebuild attempts.
const REBUILD_AFTER: Duration = Duration::from_millis(500);
/// Rebuild attempts before giving up, about 10 s: a GPU reset can leave only
/// a software adapter for a few seconds.
const REBUILD_ATTEMPTS: u32 = 20;
/// Inspections the engine can have in flight (its staging buffers).
const INSPECTION_LIMIT: usize = 4;

enum Command {
    Dab(Dab, Instant),
    Exposure(f32),
    Reset,
    Inspect(u32, u32),
    LoseDevice,
}

/// What the UI reads about the engine.
#[derive(Clone)]
pub struct Status {
    pub surface: EngineSurface,
    pub stats: EngineStats,
    pub generation: u64,
    pub lost: bool,
    pub pending_dabs: usize,
    pub inspections_in_flight: usize,
    pub exposure: f32,
    pub gpu_sync: bool,
    pub latency_ms: [f32; 2],
    pub record_ms: f32,
    pub present_ms: f32,
    /// p50 of the recording phases: edits, acquire, publish encode, finish.
    pub phase_ms: [f32; 4],
    pub error: Option<String>,
    /// Whether the engine runs on its own thread.
    pub threaded: bool,
}

impl Status {
    fn of(engine: &Engine, threaded: bool, error: Option<String>) -> Self {
        Self {
            surface: engine.surface().clone(),
            stats: engine.stats(),
            generation: engine.generation(),
            lost: engine.is_lost(),
            pending_dabs: engine.pending_dabs(),
            inspections_in_flight: engine.inspections_in_flight(),
            exposure: engine.exposure(),
            gpu_sync: engine.gpu_sync(),
            latency_ms: [
                percentile(&engine.latencies_ms, 0.5),
                percentile(&engine.latencies_ms, 0.95),
            ],
            record_ms: percentile(&engine.record_ms, 0.5),
            present_ms: percentile(&engine.present_ms, 0.5),
            phase_ms: std::array::from_fn(|i| percentile(&engine.phase_ms[i], 0.5)),
            error,
            threaded,
        }
    }
    /// Whether the engine still has work the UI should watch for.
    pub fn busy(&self) -> bool {
        self.lost
            || self.pending_dabs > 0
            || self.inspections_in_flight > 0
            || self.stats.revision != self.stats.published_revision
    }
}

/// Loss bookkeeping shared by both modes: report, wait, rebuild by replay.
struct Recovery {
    lost_at: Option<Instant>,
    started: Instant,
    failed_rebuilds: u32,
}
impl Recovery {
    fn lose(&mut self, engine: &mut Engine) {
        engine.lose_device();
        self.lost_at = Some(Instant::now());
        println!(
            "engine device lost at {:.1}s; showing the last frame",
            self.started.elapsed().as_secs_f32()
        );
    }
    /// Advance the engine once: rebuild when due, otherwise apply and
    /// publish. Returns an error to show, if any.
    fn step(&mut self, engine: &mut Engine) -> Option<String> {
        if engine.lost_by_driver() {
            // A real loss (a TDR): recover the same way as a simulated one.
            engine.mark_lost();
            self.lost_at = Some(Instant::now());
            println!(
                "engine device lost at {:.1}s; showing the last frame",
                self.started.elapsed().as_secs_f32()
            );
        }
        if engine.is_lost() {
            if self
                .lost_at
                .is_some_and(|lost| lost.elapsed() >= REBUILD_AFTER)
            {
                self.lost_at = None;
                return match engine.rebuild() {
                    Ok(()) => {
                        self.failed_rebuilds = 0;
                        let stats = engine.stats();
                        println!(
                            "engine rebuilt on generation {} on {}: {} dabs replayed in {:.1} ms",
                            engine.generation(),
                            engine.adapter(),
                            stats.replayed_dabs,
                            stats.replay_ms
                        );
                        None
                    }
                    Err(error) => {
                        self.failed_rebuilds += 1;
                        if self.failed_rebuilds < REBUILD_ATTEMPTS {
                            println!(
                                "engine rebuild {} failed, retrying: {error:#}",
                                self.failed_rebuilds
                            );
                            self.lost_at = Some(Instant::now());
                        }
                        Some(format!("rebuild failed: {error:#}"))
                    }
                };
            }
            return None;
        }
        engine.frame().err().map(|error| format!("{error:#}"))
    }
}

enum Mode {
    Inline {
        engine: Box<Engine>,
        recovery: Recovery,
        error: Option<String>,
        inspections: Vec<Inspection>,
    },
    Threaded {
        commands: Sender<Command>,
        status: Arc<Mutex<Status>>,
        inspections: Arc<Mutex<Vec<Inspection>>>,
        thread: Option<JoinHandle<()>>,
    },
}

pub struct EngineDriver {
    mode: Mode,
    status: Status,
}

impl EngineDriver {
    pub fn new(slots: usize, threaded: bool, started: Instant) -> Result<Self> {
        if !threaded {
            let engine = Box::new(Engine::new(slots)?);
            let status = Status::of(&engine, false, None);
            return Ok(Self {
                mode: Mode::Inline {
                    engine,
                    recovery: Recovery {
                        lost_at: None,
                        started,
                        failed_rebuilds: 0,
                    },
                    error: None,
                    inspections: Vec::new(),
                },
                status,
            });
        }
        let (commands, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let inspections = Arc::new(Mutex::new(Vec::new()));
        let found = inspections.clone();
        let thread = std::thread::Builder::new()
            .name("workbench-engine".into())
            .spawn(move || {
                // The engine (device, queue, surface target) is created and
                // used only on this thread.
                let mut engine = match Engine::new(slots) {
                    Ok(engine) => engine,
                    Err(error) => {
                        let _ = ready_tx.send(Err(format!("{error:#}")));
                        return;
                    }
                };
                let status = Arc::new(Mutex::new(Status::of(&engine, true, None)));
                if ready_tx.send(Ok(status.clone())).is_err() {
                    return;
                }
                run(&mut engine, receiver, status, found, started);
            })?;
        let status = ready_rx
            .recv()
            .map_err(|_| anyhow!("engine thread exited"))?
            .map_err(|error| anyhow!(error))?;
        let snapshot = status.lock().unwrap().clone();
        Ok(Self {
            mode: Mode::Threaded {
                commands,
                status,
                inspections,
                thread: Some(thread),
            },
            status: snapshot,
        })
    }

    fn send(&mut self, command: Command) {
        match &mut self.mode {
            Mode::Threaded { commands, .. } => {
                let _ = commands.send(command);
            }
            Mode::Inline {
                engine, recovery, ..
            } => match command {
                Command::Dab(dab, _) => engine.dab(dab),
                Command::Exposure(stops) => engine.set_exposure(stops),
                Command::Reset => engine.reset(),
                Command::Inspect(x, y) => {
                    engine.inspect(x, y);
                }
                Command::LoseDevice => {
                    if !engine.is_lost() {
                        recovery.lose(engine);
                    }
                }
            },
        }
    }

    pub fn dab(&mut self, dab: Dab) {
        self.send(Command::Dab(dab, Instant::now()));
    }
    pub fn set_exposure(&mut self, stops: f32) {
        self.status.exposure = stops;
        self.send(Command::Exposure(stops));
    }
    pub fn reset(&mut self) {
        self.status.exposure = 0.;
        self.send(Command::Reset);
    }
    /// `false` when inspections are at their limit or the device is lost.
    pub fn inspect(&mut self, x: u32, y: u32) -> bool {
        if self.status.lost || self.status.inspections_in_flight >= INSPECTION_LIMIT {
            return false;
        }
        self.status.inspections_in_flight += 1;
        self.send(Command::Inspect(x, y));
        true
    }
    pub fn lose_device(&mut self) {
        self.status.lost = true;
        self.send(Command::LoseDevice);
    }

    /// Once per display frame: on the UI thread, advance the engine; on its
    /// own thread, just read its status. Returns the status.
    pub fn step(&mut self) -> &Status {
        match &mut self.mode {
            Mode::Inline {
                engine,
                recovery,
                error,
                inspections,
            } => {
                if let Some(message) = recovery.step(engine) {
                    *error = Some(message);
                } else if !engine.is_lost() {
                    *error = None;
                }
                inspections.extend(engine.poll_inspections());
                self.status = Status::of(engine, false, error.clone());
            }
            Mode::Threaded { status, .. } => {
                self.status = status.lock().unwrap().clone();
            }
        }
        &self.status
    }

    pub fn status(&self) -> &Status {
        &self.status
    }
    pub fn surface(&self) -> &EngineSurface {
        &self.status.surface
    }
    pub fn stats(&self) -> EngineStats {
        self.status.stats
    }
    pub fn generation(&self) -> u64 {
        self.status.generation
    }
    pub fn is_lost(&self) -> bool {
        self.status.lost
    }
    pub fn pending_dabs(&self) -> usize {
        self.status.pending_dabs
    }
    pub fn inspections_in_flight(&self) -> usize {
        self.status.inspections_in_flight
    }
    pub fn exposure(&self) -> f32 {
        self.status.exposure
    }
    pub fn gpu_sync(&self) -> bool {
        self.status.gpu_sync
    }
    pub fn threaded(&self) -> bool {
        self.status.threaded
    }

    pub fn take_inspections(&mut self) -> Vec<Inspection> {
        match &mut self.mode {
            Mode::Inline { inspections, .. } => std::mem::take(inspections),
            Mode::Threaded { inspections, .. } => std::mem::take(&mut *inspections.lock().unwrap()),
        }
    }
}

impl Drop for EngineDriver {
    fn drop(&mut self) {
        if let Mode::Threaded {
            commands, thread, ..
        } = &mut self.mode
        {
            // Closing the channel stops the thread.
            let (closed, _) = mpsc::channel();
            *commands = closed;
            if let Some(thread) = thread.take() {
                let _ = thread.join();
            }
        }
    }
}

/// The engine thread: wait for commands (briefly while work is in flight,
/// otherwise until one arrives), apply them all, then advance once.
fn run(
    engine: &mut Engine,
    commands: Receiver<Command>,
    status: Arc<Mutex<Status>>,
    found: Arc<Mutex<Vec<Inspection>>>,
    started: Instant,
) {
    let mut recovery = Recovery {
        lost_at: None,
        started,
        failed_rebuilds: 0,
    };
    let mut error = None;
    let mut published = Instant::now();
    loop {
        let wait = if engine.busy() {
            Duration::from_millis(1)
        } else {
            Duration::from_millis(250)
        };
        let mut next = match commands.recv_timeout(wait) {
            Ok(command) => Some(command),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return,
        };
        while let Some(command) = next {
            match command {
                Command::Dab(dab, sent) => {
                    engine.dab(dab);
                    engine.note_edit_time(sent);
                }
                Command::Exposure(stops) => engine.set_exposure(stops),
                Command::Reset => engine.reset(),
                Command::Inspect(x, y) => {
                    engine.inspect(x, y);
                }
                Command::LoseDevice => {
                    if !engine.is_lost() {
                        recovery.lose(engine);
                    }
                }
            }
            next = commands.try_recv().ok();
        }
        if let Some(message) = recovery.step(engine) {
            error = Some(message);
        } else if !engine.is_lost() {
            error = None;
        }
        let results = engine.poll_inspections();
        if !results.is_empty() {
            found.lock().unwrap().extend(results);
        }
        // Share a fresh status at most every 4 ms (percentiles sort samples).
        if published.elapsed() >= Duration::from_millis(4) || !engine.busy() {
            published = Instant::now();
            *status.lock().unwrap() = Status::of(engine, true, error.clone());
        }
    }
}
