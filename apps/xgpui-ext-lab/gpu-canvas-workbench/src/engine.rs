//! The workbench engine: a wgpu device of its own with a persistent working
//! image (a storage buffer of linear P3 pixels), compute pipelines that edit
//! only the touched region, and immutable snapshots published through an
//! `EngineSurface` once per edit revision.
use crate::oracle::{Dab, HEIGHT, WIDTH};
use anyhow::{Context as _, Result, ensure};
use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use std::time::Instant;
use xgpui_engine_surface::{
    AcquireError, EngineSurface, SurfaceDesc, SurfaceFormat, WgpuSurfaceTarget, wgpu,
};

#[cfg(target_os = "macos")]
pub const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::METAL, "Metal");
#[cfg(target_os = "linux")]
pub const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::VULKAN, "Vulkan");
#[cfg(target_os = "windows")]
pub const BACKEND: (wgpu::Backends, &str) = (wgpu::Backends::DX12, "D3D12");

/// Dabs applied per paint pass; more wait for the next frame.
pub const MAX_DABS: usize = 256;
/// Edge of the tiles counted as touched (diagnostics only).
pub const TILE: u32 = 64;
const INSPECT: u32 = 4;
const STAGING: usize = 4;
const PIXEL_BYTES: u64 = 16;

const SHADER: &str = r#"
struct Params {
    origin: vec2<u32>,
    size: vec2<u32>,
    canvas: vec2<u32>,
    count: u32,
    exposure: f32,
};
struct Dab { shape: vec4<f32>, color: vec4<f32> };

@group(0) @binding(0) var<storage, read_write> working: array<vec4<f32>>;
@group(0) @binding(1) var<uniform> p: Params;
@group(0) @binding(2) var<storage, read> dabs: array<Dab>;
@group(0) @binding(3) var slot: texture_storage_2d<rgba16float, write>;

fn checker(x: u32, y: u32) -> f32 {
    return select(0.30, 0.18, (x / 16u + y / 16u) % 2u == 0u);
}

fn base(x: u32, y: u32) -> vec3<f32> {
    let c = checker(x, y);
    if (y < 80u) {
        let v = f32(x) / f32(p.canvas.x - 1u);
        return vec3<f32>(v);
    }
    if (y < 120u) {
        let v = 0.02 * (f32(x) / f32(p.canvas.x - 1u));
        return vec3<f32>(v);
    }
    if (y >= 140u && y < 220u && x >= 20u) {
        let i = (x - 20u) / 140u;
        if (i < 4u && (x - 20u) % 140u < 120u) {
            var patches = array<vec3<f32>, 4>(
                vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0),
                vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(1.0, 1.0, 0.0));
            return patches[i];
        }
    }
    if (y >= 240u && y < 380u) {
        var colors = array<vec3<f32>, 4>(
            vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 0.6, 1.0),
            vec3<f32>(1.0, 1.0, 1.0), vec3<f32>(0.0, 0.0, 0.0));
        var alphas = array<f32, 4>(0.25, 0.5, 0.75, 0.9);
        for (var i = 0u; i < 4u; i++) {
            let dx = f32(x) + 0.5 - (90.0 + 150.0 * f32(i));
            let dy = f32(y) + 0.5 - 310.0;
            if (dx * dx + dy * dy <= 3600.0) {
                return colors[i] * alphas[i] + c * (1.0 - alphas[i]);
            }
        }
    }
    return vec3<f32>(c);
}

@compute @workgroup_size(8, 8)
fn generate(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.canvas.x || id.y >= p.canvas.y) { return; }
    working[id.y * p.canvas.x + id.x] = vec4<f32>(base(id.x, id.y), 1.0);
}

@compute @workgroup_size(8, 8)
fn paint(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.size.x || id.y >= p.size.y) { return; }
    let x = p.origin.x + id.x;
    let y = p.origin.y + id.y;
    let index = y * p.canvas.x + x;
    var pixel = working[index].rgb;
    for (var i = 0u; i < p.count; i++) {
        let d = dabs[i];
        let dx = f32(x) + 0.5 - d.shape.x;
        let dy = f32(y) + 0.5 - d.shape.y;
        if (dx * dx + dy * dy <= d.shape.z * d.shape.z) {
            pixel = d.color.rgb * d.shape.w + pixel * (1.0 - d.shape.w);
        }
    }
    working[index] = vec4<f32>(pixel, 1.0);
}

@compute @workgroup_size(8, 8)
fn publish(@builtin(global_invocation_id) id: vec3<u32>) {
    if (id.x >= p.canvas.x || id.y >= p.canvas.y) { return; }
    let pixel = working[id.y * p.canvas.x + id.x].rgb;
    let out = clamp(pixel * exp2(p.exposure), vec3<f32>(0.0), vec3<f32>(1.0));
    textureStore(slot, vec2<i32>(id.xy), vec4<f32>(out, 1.0));
}
"#;

/// Numerical values read back from the working image.
#[derive(Clone, Debug)]
pub struct Inspection {
    pub x: u32,
    pub y: u32,
    /// Edit revision the values belong to.
    pub revision: u64,
    /// Engine generation (device) that produced them.
    pub generation: u64,
    /// `INSPECT`×`INSPECT` linear P3 pixels starting at (x, y).
    pub pixels: Vec<[f32; 4]>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EngineStats {
    pub revision: u64,
    pub published_revision: u64,
    pub dabs_applied: u64,
    pub paint_passes: u64,
    pub tiles_touched: u64,
    pub publishes: u64,
    pub publish_blocked: u64,
    pub submissions: u64,
    /// GPU objects created (buffers, textures, bind groups) since startup.
    pub resources_created: u64,
    /// Bytes the compute publish pass writes into slots (GPU-side copies).
    pub gpu_bytes_published: u64,
    /// Bytes uploaded from the CPU (dab records and parameters).
    pub cpu_bytes_uploaded: u64,
    /// Bytes read back to the CPU (inspection only).
    pub cpu_bytes_read: u64,
    pub inspections: u64,
    /// Device generations replaced so far.
    pub recoveries: u64,
    /// Dabs replayed by the last recovery, and how long it took.
    pub replayed_dabs: u64,
    pub replay_ms: f32,
}

struct Staging {
    buffer: wgpu::Buffer,
    // 0 idle, 1 mapping, 2 mapped, 3 failed.
    state: Arc<AtomicU8>,
    request: Option<(u32, u32, u64)>,
}

pub struct Engine {
    pub device: wgpu::Device,
    queue: wgpu::Queue,
    target: WgpuSurfaceTarget,
    working: wgpu::Buffer,
    /// Whole-canvas parameters (generate, publish).
    params: wgpu::Buffer,
    /// Paint-pass parameters, separate so a paint and a publish can share one
    /// submission (queued buffer writes apply to the whole submission).
    paint_params: wgpu::Buffer,
    dab_buffer: wgpu::Buffer,
    generate: wgpu::ComputePipeline,
    paint: wgpu::ComputePipeline,
    publish: wgpu::ComputePipeline,
    generate_bind: wgpu::BindGroup,
    paint_bind: wgpu::BindGroup,
    publish_binds: Vec<Option<wgpu::BindGroup>>,
    staging: Vec<Staging>,
    pending: Vec<Dab>,
    /// Every dab since the last reset: the retained CPU description used to
    /// rebuild the working image on a new device.
    log: Vec<Dab>,
    generation: u64,
    lost: bool,
    slots: usize,
    reset_pending: bool,
    exposure: f32,
    dirty_since: Option<Instant>,
    stats: EngineStats,
    pub latencies_ms: Vec<f32>,
    /// CPU time of publishing frames: recording, and inside `present`
    /// (submission and sync signal).
    pub record_ms: Vec<f32>,
    pub present_ms: Vec<f32>,
    /// Recording split into phases: edits, slot acquire (and first-use bind
    /// group), publish pass encoding, `encoder.finish()`.
    pub phase_ms: [Vec<f32>; 4],
    /// Staging belt for uploads, or `None` for queued writes.
    belt: Option<wgpu::util::StagingBelt>,
}

#[derive(Clone, Copy)]
enum Upload {
    Params,
    PaintParams,
    Dabs,
}

/// Use queued buffer writes instead of the staging belt (comparison).
pub static QUEUE_UPLOADS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl Engine {
    pub fn new(slots: usize) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: BACKEND.0,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            gpui::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .with_context(|| format!("no wgpu {} adapter", BACKEND.1))?;
        #[cfg(target_os = "linux")]
        let (device, queue) = WgpuSurfaceTarget::request_device(&adapter)?;
        #[cfg(not(target_os = "linux"))]
        let (device, queue) = gpui::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("workbench_engine"),
            required_features: WgpuSurfaceTarget::required_features(),
            ..Default::default()
        }))?;
        let format = SurfaceFormat::Rgba16FloatLinearP3;
        ensure!(format.supports_storage(), "slots must allow storage writes");
        let target = WgpuSurfaceTarget::new(
            SurfaceDesc {
                width: WIDTH,
                height: HEIGHT,
                format,
                slots,
            },
            &device,
            &queue,
        )?;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("workbench"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let (generate, paint, publish) =
            (pipeline("generate"), pipeline("paint"), pipeline("publish"));
        let working = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("workbench_working"),
            size: (WIDTH * HEIGHT) as u64 * PIXEL_BYTES,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("workbench_params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let paint_params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("workbench_paint_params"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dab_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("workbench_dabs"),
            size: (MAX_DABS * 32) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entry = |binding, resource| wgpu::BindGroupEntry { binding, resource };
        let generate_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("workbench_generate"),
            layout: &generate.get_bind_group_layout(0),
            entries: &[
                entry(0, working.as_entire_binding()),
                entry(1, params.as_entire_binding()),
            ],
        });
        let paint_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("workbench_paint"),
            layout: &paint.get_bind_group_layout(0),
            entries: &[
                entry(0, working.as_entire_binding()),
                entry(1, paint_params.as_entire_binding()),
                entry(2, dab_buffer.as_entire_binding()),
            ],
        });
        let staging = (0..STAGING)
            .map(|_| Staging {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("workbench_inspect"),
                    size: (INSPECT * INSPECT) as u64 * PIXEL_BYTES,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU8::new(0)),
                request: None,
            })
            .collect();
        let device_for_belt = device.clone();
        let stats = EngineStats {
            // Working image, two params, dabs, two bind groups, staging.
            resources_created: 6 + STAGING as u64,
            // The generated image is the first revision to publish.
            revision: 1,
            ..Default::default()
        };
        Ok(Self {
            publish_binds: vec![None; slots],
            device,
            queue,
            target,
            working,
            params,
            paint_params,
            dab_buffer,
            generate,
            paint,
            publish,
            generate_bind,
            paint_bind,
            staging,
            pending: Vec::new(),
            log: Vec::new(),
            generation: 0,
            lost: false,
            slots,
            reset_pending: true,
            exposure: 0.,
            dirty_since: Some(Instant::now()),
            stats,
            latencies_ms: Vec::new(),
            record_ms: Vec::new(),
            present_ms: Vec::new(),
            phase_ms: Default::default(),
            belt: (!QUEUE_UPLOADS.load(Ordering::Relaxed))
                .then(|| wgpu::util::StagingBelt::new(device_for_belt, 64 * 1024)),
        })
    }

    pub fn surface(&self) -> &EngineSurface {
        self.target.surface()
    }
    pub fn gpu_sync(&self) -> bool {
        self.target.gpu_sync()
    }
    pub fn stats(&self) -> EngineStats {
        self.stats
    }
    pub fn exposure(&self) -> f32 {
        self.exposure
    }
    pub fn pending_dabs(&self) -> usize {
        self.pending.len()
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn is_lost(&self) -> bool {
        self.lost
    }
    pub fn logged_dabs(&self) -> usize {
        self.log.len()
    }

    /// Simulate device loss: destroy the device and invalidate the surface.
    /// The last published frame keeps displaying; later frames fail until
    /// `rebuild`.
    pub fn lose_device(&mut self) {
        self.device.destroy();
        self.target.surface().invalidate();
        self.lost = true;
    }

    /// Replace a lost generation: a new device, surface and working set,
    /// with the working image reconstructed by replaying the retained edit
    /// log (CPU description, not GPU state). In-flight inspections of the
    /// old generation are dropped with it and never delivered. Publishes
    /// the reconstructed image as a new revision.
    pub fn rebuild(&mut self) -> Result<()> {
        let started = Instant::now();
        let mut fresh = Engine::new(self.slots)?;
        fresh.generation = self.generation + 1;
        fresh.exposure = self.exposure;
        fresh.log = std::mem::take(&mut self.log);
        fresh.pending = fresh.log.clone();
        fresh.latencies_ms = std::mem::take(&mut self.latencies_ms);
        fresh.record_ms = std::mem::take(&mut self.record_ms);
        fresh.present_ms = std::mem::take(&mut self.present_ms);
        fresh.phase_ms = std::mem::take(&mut self.phase_ms);
        let replayed = fresh.pending.len() as u64;
        fresh.stats = EngineStats {
            revision: self.stats.revision + 1,
            published_revision: self.stats.published_revision,
            recoveries: self.stats.recoveries + 1,
            replayed_dabs: replayed,
            ..fresh.stats
        };
        fresh.frame()?;
        fresh.wait_idle();
        fresh.stats.replay_ms = started.elapsed().as_secs_f32() * 1000.;
        *self = fresh;
        Ok(())
    }

    pub fn inspections_in_flight(&self) -> usize {
        self.staging.iter().filter(|s| s.request.is_some()).count()
    }

    /// Edit-to-publish latency counts from `at` (when the edit was made,
    /// e.g. on the UI thread) if that is earlier than its arrival here.
    pub fn note_edit_time(&mut self, at: Instant) {
        if self.dirty_since.is_none_or(|since| at < since) {
            self.dirty_since = Some(at);
        }
    }

    /// Whether work is in flight (edits, publish, inspections, recovery).
    pub fn busy(&self) -> bool {
        self.lost
            || !self.pending.is_empty()
            || self.inspections_in_flight() > 0
            || self.stats.revision != self.stats.published_revision
    }

    fn touch(&mut self) {
        self.stats.revision += 1;
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Regenerate the image and drop queued dabs. New revision.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.log.clear();
        self.reset_pending = true;
        self.exposure = 0.;
        self.touch();
    }

    /// Queue one dab. Each call is a new revision.
    pub fn dab(&mut self, dab: Dab) {
        self.pending.push(dab);
        self.log.push(dab);
        self.touch();
    }

    /// Linear exposure in stops, applied when publishing (the whole image
    /// changes; the working image does not). New revision.
    pub fn set_exposure(&mut self, stops: f32) {
        if stops != self.exposure {
            self.exposure = stops;
            self.touch();
        }
    }

    /// Upload `bytes` to `buffer`: through the staging belt (a copy recorded
    /// in `encoder`, reusing mapped chunks), or with a queued write that
    /// applies to the whole next submission (`--queue-uploads`).
    fn upload(&mut self, encoder: &mut wgpu::CommandEncoder, which: Upload, bytes: &[u8]) {
        let buffer = match which {
            Upload::Params => &self.params,
            Upload::PaintParams => &self.paint_params,
            Upload::Dabs => &self.dab_buffer,
        };
        match &mut self.belt {
            Some(belt) => {
                let size = wgpu::BufferSize::new(bytes.len() as u64).expect("non-empty upload");
                belt.write_buffer(encoder, buffer, 0, size)
                    .copy_from_slice(bytes);
            }
            None => self.queue.write_buffer(buffer, 0, bytes),
        }
        self.stats.cpu_bytes_uploaded += bytes.len() as u64;
    }

    fn write_params(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        paint: bool,
        origin: [u32; 2],
        size: [u32; 2],
        count: u32,
    ) {
        let mut bytes = Vec::with_capacity(32);
        for v in [origin[0], origin[1], size[0], size[1], WIDTH, HEIGHT, count] {
            bytes.extend_from_slice(&v.to_ne_bytes());
        }
        bytes.extend_from_slice(&self.exposure.to_ne_bytes());
        let which = if paint {
            Upload::PaintParams
        } else {
            Upload::Params
        };
        self.upload(encoder, which, &bytes);
    }

    /// Close `encoder` (and the belt's chunks used by it).
    fn finish(&mut self, encoder: wgpu::CommandEncoder) -> wgpu::CommandBuffer {
        if let Some(belt) = &mut self.belt {
            belt.finish();
        }
        encoder.finish()
    }

    /// After a submission: let the belt reuse chunks once the GPU is done.
    fn recall(&mut self) {
        if let Some(belt) = &mut self.belt {
            belt.recall();
        }
    }

    fn submit(&mut self, encoder: wgpu::CommandEncoder) {
        let commands = self.finish(encoder);
        self.queue.submit([commands]);
        self.recall();
        self.stats.submissions += 1;
    }

    /// Record the reset and queued dabs (up to `MAX_DABS`) into `encoder`,
    /// touching only the dabs' bounding region. Returns whether anything was
    /// recorded. One batch per submission: the dab buffer and paint
    /// parameters are written by queue writes that apply per submission.
    fn record_edits(&mut self, encoder: &mut wgpu::CommandEncoder) -> bool {
        let mut ran = false;
        if std::mem::take(&mut self.reset_pending) {
            self.write_params(encoder, false, [0, 0], [WIDTH, HEIGHT], 0);
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.generate);
                pass.set_bind_group(0, &self.generate_bind, &[]);
                pass.dispatch_workgroups(WIDTH.div_ceil(8), HEIGHT.div_ceil(8), 1);
            }
            ran = true;
        }
        if self.pending.is_empty() {
            return ran;
        }
        let batch: Vec<Dab> = self
            .pending
            .drain(..self.pending.len().min(MAX_DABS))
            .collect();
        let (mut x0, mut y0, mut x1, mut y1) = (WIDTH, HEIGHT, 0, 0);
        let mut tiles = HashSet::new();
        let mut bytes = Vec::with_capacity(batch.len() * 32);
        for dab in &batch {
            let bx0 = (dab.x - dab.radius).floor().max(0.) as u32;
            let by0 = (dab.y - dab.radius).floor().max(0.) as u32;
            let bx1 = ((dab.x + dab.radius).ceil().max(0.) as u32).min(WIDTH);
            let by1 = ((dab.y + dab.radius).ceil().max(0.) as u32).min(HEIGHT);
            if bx0 >= bx1 || by0 >= by1 {
                continue;
            }
            (x0, y0, x1, y1) = (x0.min(bx0), y0.min(by0), x1.max(bx1), y1.max(by1));
            for ty in by0 / TILE..=(by1 - 1) / TILE {
                for tx in bx0 / TILE..=(bx1 - 1) / TILE {
                    tiles.insert((tx, ty));
                }
            }
            for v in [
                dab.x,
                dab.y,
                dab.radius,
                dab.opacity,
                dab.color[0],
                dab.color[1],
                dab.color[2],
                1.,
            ] {
                bytes.extend_from_slice(&v.to_ne_bytes());
            }
        }
        self.stats.dabs_applied += batch.len() as u64;
        if x0 >= x1 {
            return ran;
        }
        self.upload(encoder, Upload::Dabs, &bytes);
        self.write_params(
            encoder,
            true,
            [x0, y0],
            [x1 - x0, y1 - y0],
            batch.len() as u32,
        );
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.paint);
            pass.set_bind_group(0, &self.paint_bind, &[]);
            pass.dispatch_workgroups((x1 - x0).div_ceil(8), (y1 - y0).div_ceil(8), 1);
        }
        self.stats.paint_passes += 1;
        self.stats.tiles_touched += tiles.len() as u64;
        true
    }

    /// Apply queued edits, then publish a snapshot if the revision changed.
    /// Presentation-only frames (nothing changed) do no GPU work.
    pub fn frame(&mut self) -> Result<()> {
        // A lost device must not be used; edits keep queuing in the log.
        ensure!(!self.lost, AcquireError::Invalidated);
        let started = Instant::now();
        // Edits and the publish pass share one submission in the common case
        // (one dab batch); extra batches are submitted ahead of it.
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut recorded = self.record_edits(&mut encoder);
        while !self.pending.is_empty() {
            self.submit(encoder);
            encoder = self.device.create_command_encoder(&Default::default());
            recorded = self.record_edits(&mut encoder);
        }
        let edits_done = Instant::now();
        if self.stats.published_revision == self.stats.revision {
            if recorded {
                self.submit(encoder);
            }
            return Ok(());
        }
        let slot = match self.target.surface().acquire() {
            Ok(slot) => slot,
            Err(AcquireError::WouldBlock) => {
                // The working image keeps the edits; publish next frame.
                if recorded {
                    self.submit(encoder);
                }
                self.stats.publish_blocked += 1;
                return Ok(());
            }
            Err(error @ AcquireError::Invalidated) => return Err(error.into()),
        };
        let index = slot.index();
        if self.publish_binds[index].is_none() {
            let texture = self.target.texture(&slot)?;
            let view = texture.create_view(&Default::default());
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("workbench_publish"),
                layout: &self.publish.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.working.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: self.params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                ],
            });
            self.publish_binds[index] = Some(bind);
            self.stats.resources_created += 1;
        }
        let acquired = Instant::now();
        self.write_params(&mut encoder, false, [0, 0], [WIDTH, HEIGHT], 0);
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.publish);
            pass.set_bind_group(0, self.publish_binds[index].as_ref().unwrap(), &[]);
            pass.dispatch_workgroups(WIDTH.div_ceil(8), HEIGHT.div_ceil(8), 1);
        }
        let encoded = Instant::now();
        let commands = self.finish(encoder);
        let presenting = Instant::now();
        for (phase, (from, to)) in self.phase_ms.iter_mut().zip([
            (started, edits_done),
            (edits_done, acquired),
            (acquired, encoded),
            (encoded, presenting),
        ]) {
            phase.push((to - from).as_secs_f32() * 1000.);
        }
        self.record_ms
            .push((presenting - started).as_secs_f32() * 1000.);
        let presented = self.target.present(slot, [commands]);
        self.recall();
        presented?;
        self.present_ms
            .push(presenting.elapsed().as_secs_f32() * 1000.);
        self.stats.submissions += 1;
        self.stats.publishes += 1;
        self.stats.gpu_bytes_published += (WIDTH * HEIGHT) as u64 * 8;
        self.stats.published_revision = self.stats.revision;
        if let Some(since) = self.dirty_since.take() {
            self.latencies_ms
                .push(since.elapsed().as_secs_f32() * 1000.);
        }
        Ok(())
    }

    /// Read a small region of the working image asynchronously. The result
    /// carries the revision of all edits queued before this call, even if
    /// more edits arrive before it completes. `false` when every staging
    /// buffer is busy (bounded; retry later).
    pub fn inspect(&mut self, x: u32, y: u32) -> bool {
        if self.lost {
            return false;
        }
        let Some(index) = self.staging.iter().position(|s| s.request.is_none()) else {
            return false;
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.record_edits(&mut encoder);
        while !self.pending.is_empty() {
            self.submit(encoder);
            encoder = self.device.create_command_encoder(&Default::default());
            self.record_edits(&mut encoder);
        }
        let x = x.min(WIDTH - INSPECT);
        let y = y.min(HEIGHT - INSPECT);
        let row = INSPECT as u64 * PIXEL_BYTES;
        for r in 0..INSPECT {
            let source = ((y + r) * WIDTH + x) as u64 * PIXEL_BYTES;
            encoder.copy_buffer_to_buffer(
                &self.working,
                source,
                &self.staging[index].buffer,
                r as u64 * row,
                row,
            );
        }
        self.submit(encoder);
        let staging = &mut self.staging[index];
        staging.request = Some((x, y, self.stats.revision));
        staging.state.store(1, Ordering::Release);
        let state = staging.state.clone();
        staging
            .buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                state.store(if result.is_ok() { 2 } else { 3 }, Ordering::Release);
            });
        true
    }

    /// Completed inspections, without blocking.
    pub fn poll_inspections(&mut self) -> Vec<Inspection> {
        if self.lost {
            return Vec::new();
        }
        let _ = self.device.poll(wgpu::PollType::Poll);
        let mut done = Vec::new();
        for staging in &mut self.staging {
            let state = staging.state.load(Ordering::Acquire);
            if staging.request.is_none() || state < 2 {
                continue;
            }
            let (x, y, revision) = staging.request.take().unwrap();
            staging.state.store(0, Ordering::Release);
            if state == 3 {
                continue;
            }
            let pixels = {
                let Ok(data) = staging.buffer.slice(..).get_mapped_range() else {
                    staging.buffer.unmap();
                    continue;
                };
                data.chunks_exact(16)
                    .map(|p| {
                        let f =
                            |i: usize| f32::from_ne_bytes(p[i * 4..i * 4 + 4].try_into().unwrap());
                        [f(0), f(1), f(2), f(3)]
                    })
                    .collect()
            };
            staging.buffer.unmap();
            self.stats.inspections += 1;
            self.stats.cpu_bytes_read += (INSPECT * INSPECT) as u64 * PIXEL_BYTES;
            done.push(Inspection {
                x,
                y,
                revision,
                generation: self.generation,
                pixels,
            });
        }
        done
    }

    /// Check mode only: block until the engine is idle.
    pub fn wait_idle(&self) {
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
    }

    /// Check mode only: the whole working image (blocking readback).
    pub fn read_working(&mut self) -> Result<Vec<[f32; 4]>> {
        let size = (WIDTH * HEIGHT) as u64 * PIXEL_BYTES;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("workbench_check_working"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.working, 0, &buffer, 0, size);
        self.queue.submit([encoder.finish()]);
        let data = map_blocking(&self.device, &buffer)?;
        Ok(data
            .chunks_exact(16)
            .map(|p| {
                let f = |i: usize| f32::from_ne_bytes(p[i * 4..i * 4 + 4].try_into().unwrap());
                [f(0), f(1), f(2), f(3)]
            })
            .collect())
    }

    /// Check mode only: run the publish pass into a check texture (the same
    /// shader and parameters as a slot) and read it back as RGBA halves.
    /// Slot transport itself is covered by the extension's tests.
    pub fn read_published(&mut self) -> Result<Vec<[u16; 4]>> {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("workbench_check_publish"),
            size: wgpu::Extent3d {
                width: WIDTH,
                height: HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("workbench_check_publish"),
            layout: &self.publish.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.working.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.params.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.write_params(&mut encoder, false, [0, 0], [WIDTH, HEIGHT], 0);
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.publish);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(WIDTH.div_ceil(8), HEIGHT.div_ceil(8), 1);
        }
        self.submit(encoder);
        let row = (WIDTH as u64 * 8).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("workbench_check_slot"),
            size: row * HEIGHT as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row as u32),
                    rows_per_image: None,
                },
            },
            texture.size(),
        );
        self.queue.submit([encoder.finish()]);
        let data = map_blocking(&self.device, &buffer)?;
        let mut out = Vec::with_capacity((WIDTH * HEIGHT) as usize);
        for y in 0..HEIGHT as usize {
            for x in 0..WIDTH as usize {
                let p = &data[y * row as usize + x * 8..];
                let h = |i: usize| u16::from_ne_bytes([p[i * 2], p[i * 2 + 1]]);
                out.push([h(0), h(1), h(2), h(3)]);
            }
        }
        Ok(out)
    }
}

fn map_blocking(device: &wgpu::Device, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = send.send(result);
        });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    receive.recv()??;
    let data = buffer
        .slice(..)
        .get_mapped_range()
        .map_err(|error| anyhow::anyhow!("map failed: {error:?}"))?
        .to_vec();
    buffer.unmap();
    Ok(data)
}
