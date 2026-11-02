use super::*;
use std::{collections::HashSet, num::NonZeroU64};
use wgpu::util::DeviceExt;
// Error scopes are thread-local. Pop synchronously before yielding so browser
// tasks and compositor work cannot accidentally enter another job's scope.
fn scoped<T>(
    device: &wgpu::Device,
    f: impl FnOnce() -> Result<T>,
) -> (Result<T>, Pin<Box<dyn Future<Output = Result<()>>>>) {
    let validation = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let allocation = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let result = f();
    let internal = internal.pop();
    let allocation = allocation.pop();
    let validation = validation.pop();
    (
        result,
        Box::pin(async move {
            let i = internal.await;
            let a = allocation.await;
            let v = validation.await;
            if let Some(e) = i {
                return Err(execution(e.to_string()));
            }
            if let Some(e) = a {
                return Err(error(c::ContentErrorKind::AllocationFailure, e.to_string()));
            }
            if let Some(e) = v {
                return Err(error(c::ContentErrorKind::ShaderValidation, e.to_string()));
            }
            Ok(())
        }),
    )
}
type Resources = Vec<Option<Vec<Arc<Resource>>>>;
fn resource(resources: &Resources, id: c::ResourceId) -> &Arc<Resource> {
    &resources[id.node()].as_ref().unwrap()[id.output()]
}
fn padded(value: u64) -> u64 {
    value.div_ceil(4) * 4
}
fn mip_layout(d: &c::ImageDescriptor) -> Vec<(u64, u32, u32, u32, u32)> {
    let mut offset = 0;
    let mut result = Vec::new();
    for level in 0..d.mip_levels() {
        let w = (d.extent().width() >> level).max(1);
        let h = (d.extent().height() >> level).max(1);
        let packed = w * d.format().bytes_per_pixel() as u32;
        let row = packed.div_ceil(256) * 256;
        result.push((offset, w, h, row, packed));
        offset += u64::from(row) * u64::from(h);
    }
    result
}
// wgpu does not expose driver allocation sizes. Reserve padded payloads in
// conservative texture/buffer granules, rather than claiming physical byte counts.
fn buffer_charge(bytes: u64) -> u64 {
    bytes.div_ceil(4096) * 4096
}
fn image_charge(d: &c::ImageDescriptor) -> u64 {
    let bytes: u64 = mip_layout(d)
        .iter()
        .map(|(_, _, h, row, _)| u64::from(*h) * u64::from(*row))
        .sum();
    bytes.div_ceil(65536) * 65536
}

fn charge(d: &c::ResourceDescriptor) -> u64 {
    match d {
        c::ResourceDescriptor::Image(d) => image_charge(d),
        c::ResourceDescriptor::Buffer(d) => buffer_charge(d.byte_len()),
    }
}
fn buffer(device: &wgpu::Device, bytes: &[u8], usage: wgpu::BufferUsages) -> Arc<Resource> {
    Arc::new(Resource::Buffer {
        buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("portable content bytes"),
            contents: bytes,
            usage,
        }),
        charge: buffer_charge(bytes.len() as u64),
    })
}
fn copy_image(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    target: &wgpu::Texture,
    d: &c::ImageDescriptor,
) {
    for level in 0..d.mip_levels() {
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: source,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: target,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: (d.extent().width() >> level).max(1),
                height: (d.extent().height() >> level).max(1),
                depth_or_array_layers: 1,
            },
        );
    }
}
fn clear(
    encoder: &mut wgpu::CommandEncoder,
    texture: &wgpu::Texture,
    d: &c::ImageDescriptor,
    color: [f32; 4],
) {
    for level in 0..d.mip_levels() {
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("portable defined image clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: color[0] as f64,
                        g: color[1] as f64,
                        b: color[2] as f64,
                        a: color[3] as f64,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }
}
struct Flight {
    node: usize,
    signal: Arc<Signal>,
    _lease: c::SubmissionLease<Arc<Resource>>,
}
struct Run<'a> {
    worker: &'a Worker,
    graph: Arc<c::ContentGraph>,
    tracker: c::ExecutionTracker<Arc<Resource>>,
    resources: Resources,
    pipelines: Vec<Option<Arc<Pipeline>>>,
    usages: Vec<Vec<wgpu::TextureUsages>>,
    flights: VecDeque<Flight>,
    stats: WgpuContentStats,
}
impl Worker {
    pub(super) async fn execute(
        &mut self,
        graph: Arc<c::ContentGraph>,
        limits: WgpuContentLimits,
        cancelled: &AtomicBool,
        readback: bool,
    ) -> Result<WgpuContentFrame> {
        if cancelled.load(Ordering::Acquire) {
            return Err(error(
                c::ContentErrorKind::Cancelled,
                "content request cancelled before execution",
            ));
        }
        for node in graph.nodes() {
            if let c::ContentOperation::ExternalImage(image) = node.operation() {
                let lease = image.acquire()?;
                self.validate_external(&lease, image.descriptor())?;
            }
        }
        let result = self
            .execute_scoped(graph, limits, cancelled, readback)
            .await;
        if result.as_ref().is_err_and(|e| {
            matches!(
                e.kind,
                c::ContentErrorKind::ShaderValidation | c::ContentErrorKind::AllocationFailure
            )
        }) {
            self.pipelines.clear();
            self.pipeline_order.clear();
            self.byte_copy = None;
        }
        result
    }

    fn validate_external(
        &self,
        lease: &c::ExternalImageLease,
        descriptor: &c::ImageDescriptor,
    ) -> Result<()> {
        #[cfg(all(unix, feature = "dma-buf"))]
        if let Some(frame) = lease.backing::<crate::custom_draw::dma_buf::DmaBufFrame>() {
            return frame.validate_external(&self.device, &self.queue, descriptor);
        }
        let _ = (lease, descriptor);
        Err(unsupported("unsupported wgpu external backing"))
    }
    fn import_external(
        &self,
        image: &c::ExternalImage,
        descriptor: &c::ImageDescriptor,
    ) -> Result<Arc<Resource>> {
        let lease = image.acquire()?;
        self.validate_external(&lease, descriptor)?;
        let texture = self.import_external_texture(&lease, descriptor)?;
        Ok(Arc::new(Resource::ExternalImage {
            texture,
            charge: image_charge(descriptor),
            _lease: lease,
        }))
    }
    fn import_external_texture(
        &self,
        lease: &c::ExternalImageLease,
        descriptor: &c::ImageDescriptor,
    ) -> Result<wgpu::Texture> {
        #[cfg(all(unix, feature = "dma-buf"))]
        if let Some(frame) = lease.backing::<crate::custom_draw::dma_buf::DmaBufFrame>() {
            return frame.import_external(&self.device, &self.queue, descriptor, lease);
        }
        let _ = (lease, descriptor);
        Err(unsupported("unsupported wgpu external backing"))
    }

    async fn execute_scoped(
        &mut self,
        graph: Arc<c::ContentGraph>,
        limits: WgpuContentLimits,
        cancelled: &AtomicBool,
        readback: bool,
    ) -> Result<WgpuContentFrame> {
        let mut stats = WgpuContentStats {
            worker_thread: Some(if cfg!(target_family = "wasm") {
                "wgpu-content-browser".into()
            } else {
                std::thread::current()
                    .name()
                    .unwrap_or("wgpu-content")
                    .into()
            }),
            ..Default::default()
        };
        let device = self.device.clone();
        let (prepared, scopes) = scoped(&device, || self.prepare(&graph, &mut stats));
        scopes.await?;
        let (pipelines, usages) = prepared?;
        let readback_bytes = if readback {
            graph
                .roots()
                .iter()
                .map(|r| graph.nodes()[r.node()].outputs()[r.output()].byte_len())
                .sum::<u64>()
        } else {
            0
        };
        if readback_bytes > limits.max_readback_bytes {
            return Err(error(
                c::ContentErrorKind::ResourceLimit,
                "content readback exceeds byte budget",
            ));
        }
        let headroom = if readback {
            graph
                .roots()
                .iter()
                .map(|r| charge(&graph.nodes()[r.node()].outputs()[r.output()]))
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        let available = limits
            .max_gpu_bytes
            .checked_sub(self.exported.load(Ordering::Acquire))
            .and_then(|v| v.checked_sub(headroom))
            .ok_or_else(|| {
                error(
                    c::ContentErrorKind::ResourceLimit,
                    "retained GPU outputs/readback exceed budget",
                )
            })?;
        let charges = graph
            .nodes()
            .iter()
            .map(|node| {
                let mut bytes = node.outputs().iter().map(charge).sum::<u64>();
                if matches!(node.operation(), c::ContentOperation::UploadImage(_)) {
                    let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                        unreachable!()
                    };
                    bytes += image_charge(d);
                }
                let bindings = match node.operation() {
                    c::ContentOperation::Render(p) => p.bindings(),
                    c::ContentOperation::Compute(p) => p.bindings(),
                    _ => &[],
                };
                for b in bindings {
                    if let c::BoundResource::Uniform(data) = b.resource() {
                        bytes += buffer_charge(data.len() as u64);
                    }
                }
                if self.byte_copy.is_some() {
                    bytes += 4096;
                }
                bytes
            })
            .collect();
        let plan = c::ExecutionPlan::new(
            graph.clone(),
            charges,
            c::ExecutionLimits {
                max_live_bytes: available,
                max_recording_nodes: 1,
            },
        )?;
        let mut run = Run {
            worker: self,
            tracker: plan.start_with_owner(),
            resources: vec![None; graph.nodes().len()],
            graph,
            pipelines,
            usages,
            flights: VecDeque::new(),
            stats,
        };
        let result = run.run(&limits, cancelled).await;
        let drain = run.drain().await;
        result?;
        drain?;
        let mut owners = HashMap::new();
        let mut gpu = Vec::new();
        for root in run.graph.roots() {
            let resource = resource(&run.resources, *root).clone();
            let c::ResourceDescriptor::Image(d) =
                &run.graph.nodes()[root.node()].outputs()[root.output()]
            else {
                unreachable!()
            };
            let owner = owners
                .entry(*root)
                .or_insert_with(|| {
                    let charge = resource.charge();
                    self.exported.fetch_add(charge, Ordering::AcqRel);
                    Arc::new(ImageOwner {
                        resource,
                        _reservation: Reservation {
                            charge,
                            budget: self.exported.clone(),
                        },
                    })
                })
                .clone();
            gpu.push(GpuImage {
                owner,
                descriptor: d.clone(),
            });
        }
        let mut stats = run.stats;
        drop(run.resources);
        drop(run.tracker);
        let images = if readback {
            let mut images = Vec::new();
            for image in &gpu {
                images.push(self.read_image(image, &mut stats).await?);
            }
            images
        } else {
            Vec::new()
        };
        if readback {
            gpu.clear();
        }
        Ok(WgpuContentFrame { images, gpu, stats })
    }
    fn prepare(
        &mut self,
        graph: &Arc<c::ContentGraph>,
        stats: &mut WgpuContentStats,
    ) -> Result<(Vec<Option<Arc<Pipeline>>>, Vec<Vec<wgpu::TextureUsages>>)> {
        let mut pipelines = Vec::new();
        let mut variants = HashSet::new();
        let mut usages: Vec<Vec<_>> = graph
            .nodes()
            .iter()
            .map(|n| {
                n.outputs()
                    .iter()
                    .map(|_| {
                        wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::COPY_DST
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::RENDER_ATTACHMENT
                    })
                    .collect()
            })
            .collect();
        for (id, node) in graph.nodes().iter().enumerate() {
            for d in node.outputs() {
                match d {
                    c::ResourceDescriptor::Image(d)
                        if d.extent().width() <= self.capabilities.max_image_dimension
                            && d.extent().height() <= self.capabilities.max_image_dimension => {}
                    c::ResourceDescriptor::Buffer(d)
                        if d.byte_len() <= self.capabilities.max_buffer_bytes => {}
                    _ => return Err(unsupported("content resource exceeds wgpu device limits")),
                }
            }
            let bindings = match node.operation() {
                c::ContentOperation::Compute(p) => {
                    p.program()
                        .validate_dispatch(p.dispatch(), self.capabilities.compute.as_ref())?;
                    p.bindings()
                }
                c::ContentOperation::Render(p) => p.bindings(),
                _ => &[],
            };
            let program = match node.operation() {
                c::ContentOperation::Compute(p) => Some(p.program()),
                c::ContentOperation::Render(p) => Some(p.program()),
                _ => None,
            };
            for binding in bindings {
                if matches!(
                    program.unwrap().binding(binding.name()).unwrap().kind(),
                    c::ProgramBindingKind::StorageImage { .. }
                ) {
                    match binding.resource() {
                        c::BoundResource::Image(r) => {
                            if matches!(
                                graph.nodes()[r.node()].operation(),
                                c::ContentOperation::ExternalImage(_)
                            ) {
                                return Err(unsupported(
                                    "external images support sampled reads and copies, not storage binding",
                                ));
                            }
                            usages[r.node()][r.output()] |= wgpu::TextureUsages::STORAGE_BINDING
                        }
                        c::BoundResource::Output { output, .. } => {
                            usages[id][*output] |= wgpu::TextureUsages::STORAGE_BINDING
                        }
                        _ => {}
                    }
                }
            }
            let pipeline = if let Some(key) = PipelineKey::of(node) {
                variants.insert(key.clone());
                if variants.len() > self.options.pipeline_cache_entries {
                    return Err(error(
                        c::ContentErrorKind::ResourceLimit,
                        "content graph exceeds pipeline-variant budget",
                    ));
                }
                Some(self.pipeline(key, stats)?)
            } else {
                None
            };
            pipelines.push(pipeline);
        }
        if graph.nodes().iter().any(|n|matches!(n.operation(),c::ContentOperation::CopyBuffer{offset,..} if !offset.is_multiple_of(4))||matches!(n.operation(),c::ContentOperation::CopyBuffer{..})&&!n.outputs()[0].byte_len().is_multiple_of(4)||matches!(n.operation(),c::ContentOperation::Compute(p) if p.bindings().iter().any(|b|matches!(b.resource(),c::BoundResource::Output{initial:Some(c::InitialResource::Buffer(r)),..} if !r.offset.is_multiple_of(4)||!r.byte_len.is_multiple_of(4))))){self.ensure_byte_copy()?;}
        Ok((pipelines, usages))
    }
    async fn read_image(
        &self,
        image: &GpuImage,
        stats: &mut WgpuContentStats,
    ) -> Result<WgpuContentImage> {
        let d = &image.descriptor;
        let layout = mip_layout(d);
        let bytes = image_charge(d);
        let (setup, scopes) = scoped(&self.device, || {
            let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("portable validation readback"),
                size: bytes,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = self.device.create_command_encoder(&Default::default());
            for (level, (offset, w, h, row, _)) in layout.iter().enumerate() {
                encoder.copy_texture_to_buffer(
                    wgpu::TexelCopyTextureInfo {
                        texture: image.texture(),
                        mip_level: level as u32,
                        origin: Default::default(),
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyBufferInfo {
                        buffer: &staging,
                        layout: wgpu::TexelCopyBufferLayout {
                            offset: *offset,
                            bytes_per_row: Some(*row),
                            rows_per_image: Some(*h),
                        },
                    },
                    wgpu::Extent3d {
                        width: *w,
                        height: *h,
                        depth_or_array_layers: 1,
                    },
                );
            }
            self.queue.submit([encoder.finish()]);

            let signal = Signal::new();
            let status = Arc::new(Mutex::new(None));
            let flag = signal.clone();
            let outcome = status.clone();
            staging.map_async(wgpu::MapMode::Read, .., move |result| {
                *outcome.lock().unwrap() = Some(result.map_err(|e| execution(e.to_string())));
                flag.finish();
            });
            Ok((staging, signal, status))
        });
        scopes.await?;
        let (staging, signal, status) = setup?;
        Completed(signal).await;
        status.lock().unwrap().take().unwrap()?;
        let mapped = staging
            .get_mapped_range(..)
            .map_err(|e| execution(e.to_string()))?;
        let mut levels = Vec::new();
        for (offset, _, h, row, packed) in layout {
            let mut bytes = Vec::new();
            for y in 0..h {
                let start = (offset + u64::from(y) * u64::from(row)) as usize;
                bytes.extend_from_slice(&mapped[start..start + packed as usize]);
            }
            levels.push(bytes);
        }
        drop(mapped);
        staging.unmap();
        stats.readbacks += 1;
        Ok(WgpuContentImage {
            descriptor: d.clone(),
            levels,
        })
    }
    fn ensure_byte_copy(&mut self) -> Result<()> {
        if self.byte_copy.is_some() {
            return Ok(());
        }
        if self.capabilities.compute.is_none() {
            return Err(unsupported(
                "unaligned portable buffer copies require compute support",
            ));
        }
        let module=self.device.create_shader_module(wgpu::ShaderModuleDescriptor{label:Some("portable byte copy"),source:wgpu::ShaderSource::Wgsl("struct Params { offset:u32, len:u32, pad:vec2<u32> } @group(0) @binding(0) var<storage,read> source:array<u32>; @group(0) @binding(1) var<storage,read_write> destination:array<u32>; @group(0) @binding(2) var<uniform> p:Params; @compute @workgroup_size(64) fn copy(@builtin(global_invocation_id) id:vec3<u32>){let start=id.x*4u;if start>=p.len{return;}var word=0u;for(var i=0u;i<4u;i++){if start+i<p.len{let at=p.offset+start+i;word|=((source[at/4u]>>((at%4u)*8u))&255u)<<(i*8u);}}destination[id.x]=word;}".into())});
        let pipeline = self
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("portable byte copy"),
                layout: None,
                module: &module,
                entry_point: Some("copy"),
                compilation_options: Default::default(),
                cache: None,
            });
        let layout = pipeline.get_bind_group_layout(0);
        self.byte_copy = Some((pipeline, layout));
        Ok(())
    }
}
impl Run<'_> {
    fn retire(&mut self) {
        for id in 0..self.resources.len() {
            if !self.tracker.has_reservation(id).unwrap() {
                self.resources[id] = None;
            }
        }
    }
    async fn receive(&mut self) -> Result<()> {
        let flight = self
            .flights
            .pop_front()
            .ok_or_else(|| execution("portable admission stalled with no pending work"))?;
        Completed(flight.signal.clone()).await;
        self.tracker.completed(flight.node, true)?;
        drop(flight);
        self.retire();
        Ok(())
    }
    async fn drain(&mut self) -> Result<()> {
        while !self.flights.is_empty() {
            self.receive().await?;
        }
        Ok(())
    }
    async fn run(&mut self, _limits: &WgpuContentLimits, cancelled: &AtomicBool) -> Result<()> {
        for id in 0..self.graph.nodes().len() {
            while self
                .flights
                .front()
                .is_some_and(|f| f.signal.done.load(Ordering::Acquire))
            {
                self.receive().await?;
            }
            #[cfg(feature = "test-support")]
            if let Some((node, gate)) = &_limits.pause_before_node {
                if *node == id {
                    gate.enter(cancelled).await;
                }
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(error(
                    c::ContentErrorKind::Cancelled,
                    "content request cancelled; submitted prefix retires",
                ));
            }
            #[cfg(feature = "test-support")]
            if _limits.fail_before_node == Some(id) {
                return Err(execution("injected portable failure before operation"));
            }
            loop {
                match self.tracker.try_admit(id)? {
                    c::Admission::Admitted => break,
                    c::Admission::Deferred(_) => self.receive().await?,
                }
            }
            while self.flights.len() >= self.worker.options.queue_capacity {
                self.receive().await?;
            }
            self.stats.peak_reserved_bytes = self
                .stats
                .peak_reserved_bytes
                .max(self.tracker.live_bytes());
            let device = self.worker.device.clone();
            let (encoded, scopes) = scoped(&device, || self.encode(id));
            let driver = scopes.await;
            if let Err(e) = encoded {
                self.tracker.discarded(id)?;
                self.retire();
                return Err(e);
            }
            driver?;
        }
        Ok(())
    }
    fn allocate(
        &self,
        d: &c::ResourceDescriptor,
        usage: wgpu::TextureUsages,
        upload: Option<&[u8]>,
    ) -> Arc<Resource> {
        match d {
            c::ResourceDescriptor::Image(d) => Arc::new(Resource::Image {
                texture: self.worker.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("portable image"),
                    size: wgpu::Extent3d {
                        width: d.extent().width(),
                        height: d.extent().height(),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: d.mip_levels(),
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: format(d.format()),
                    usage,
                    view_formats: &[],
                }),
                charge: image_charge(d),
            }),
            c::ResourceDescriptor::Buffer(d) => {
                let mut usage = wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
                if self.worker.capabilities.compute.is_some()
                    || d.usage().contains(c::BufferUsage::STORAGE)
                {
                    usage |= wgpu::BufferUsages::STORAGE;
                }
                if d.usage().contains(c::BufferUsage::VERTEX) {
                    usage |= wgpu::BufferUsages::VERTEX;
                }
                if d.usage().contains(c::BufferUsage::INDEX) {
                    usage |= wgpu::BufferUsages::INDEX;
                }
                if d.usage().contains(c::BufferUsage::UNIFORM) {
                    usage |= wgpu::BufferUsages::UNIFORM;
                }
                if let Some(data) = upload {
                    buffer(&self.worker.device, data, usage)
                } else {
                    Arc::new(Resource::Buffer {
                        buffer: self.worker.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("portable buffer"),
                            size: padded(d.byte_len()),
                            usage,
                            mapped_at_creation: false,
                        }),
                        charge: buffer_charge(d.byte_len()),
                    })
                }
            }
        }
    }
    fn copy_buffer(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        offset: u64,
        target: &wgpu::Buffer,
        len: u64,
        owners: &mut Vec<Arc<Resource>>,
    ) -> Result<()> {
        if offset.is_multiple_of(4) && len.is_multiple_of(4) {
            encoder.copy_buffer_to_buffer(source, offset, target, 0, len);
            return Ok(());
        }
        let (pipeline, layout) = self
            .worker
            .byte_copy
            .as_ref()
            .ok_or_else(|| unsupported("unaligned buffer copy has no compute lowering"))?;
        if source.size() > self.worker.capabilities.bindings.max_storage_bytes
            || target.size() > self.worker.capabilities.bindings.max_storage_bytes
        {
            return Err(unsupported(
                "unaligned buffer copy exceeds storage binding limits",
            ));
        }
        let bytes = [
            u32::try_from(offset).map_err(|_| unsupported("byte copy offset exceeds u32"))?,
            u32::try_from(len).map_err(|_| unsupported("byte copy length exceeds u32"))?,
            0,
            0,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
        let uniform = buffer(&self.worker.device, &bytes, wgpu::BufferUsages::UNIFORM);
        let group = self
            .worker
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("portable byte copy"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: source.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: target.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.buffer().as_entire_binding(),
                    },
                ],
            });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(len.div_ceil(256) as u32, 1, 1);
        drop(pass);
        owners.push(uniform);
        Ok(())
    }
    fn groups(
        &self,
        pipeline: &Pipeline,
        bindings: &[c::ResolvedBinding],
        outputs: &[Arc<Resource>],
        owners: &mut Vec<Arc<Resource>>,
    ) -> Vec<wgpu::BindGroup> {
        enum Binding {
            Buffer(wgpu::Buffer, u64, u64),
            Image(wgpu::TextureView),
            Sampler(wgpu::Sampler),
        }
        let mut native = Vec::new();
        for b in bindings {
            let value = match b.resource() {
                c::BoundResource::Uniform(data) => {
                    let uniform = buffer(&self.worker.device, data, wgpu::BufferUsages::UNIFORM);
                    let value =
                        Binding::Buffer(uniform.buffer().clone(), 0, padded(data.len() as u64));
                    owners.push(uniform);
                    value
                }
                c::BoundResource::Buffer(r) => Binding::Buffer(
                    resource(&self.resources, r.source).buffer().clone(),
                    r.offset,
                    r.byte_len,
                ),
                c::BoundResource::Image(r) => {
                    let storage = matches!(
                        pipeline.program.binding(b.name()).unwrap().kind(),
                        c::ProgramBindingKind::StorageImage { .. }
                    );
                    Binding::Image(resource(&self.resources, *r).texture().create_view(
                        &wgpu::TextureViewDescriptor {
                            mip_level_count: storage.then_some(1),
                            ..Default::default()
                        },
                    ))
                }
                c::BoundResource::Output { output, .. } => match outputs[*output].as_ref() {
                    Resource::Buffer { buffer, .. } => {
                        Binding::Buffer(buffer.clone(), 0, buffer.size())
                    }
                    Resource::Image { texture, .. } | Resource::ExternalImage { texture, .. } => {
                        Binding::Image(texture.create_view(&wgpu::TextureViewDescriptor {
                            mip_level_count: Some(1),
                            ..Default::default()
                        }))
                    }
                },
                c::BoundResource::Sampler(s) => {
                    let filter = |f| match f {
                        c::FilterMode::Nearest => wgpu::FilterMode::Nearest,
                        c::FilterMode::Linear => wgpu::FilterMode::Linear,
                    };
                    let address = |a| match a {
                        c::AddressMode::Clamp => wgpu::AddressMode::ClampToEdge,
                        c::AddressMode::Repeat => wgpu::AddressMode::Repeat,
                        c::AddressMode::MirrorRepeat => wgpu::AddressMode::MirrorRepeat,
                    };
                    Binding::Sampler(self.worker.device.create_sampler(&wgpu::SamplerDescriptor {
                        address_mode_u: address(s.address[0]),
                        address_mode_v: address(s.address[1]),
                        mag_filter: filter(s.mag_filter),
                        min_filter: filter(s.min_filter),
                        mipmap_filter: match s.mip_filter {
                            Some(c::FilterMode::Linear) => wgpu::MipmapFilterMode::Linear,
                            _ => wgpu::MipmapFilterMode::Nearest,
                        },
                        lod_max_clamp: if s.mip_filter.is_some() { 32. } else { 0. },
                        ..Default::default()
                    }))
                }
            };
            native.push((b.address(), value));
        }
        pipeline
            .layouts
            .iter()
            .enumerate()
            .map(|(group, layout)| {
                let entries: Vec<_> = native
                    .iter()
                    .filter(|(address, _)| address.group == group as u32)
                    .map(|(address, value)| wgpu::BindGroupEntry {
                        binding: address.binding,
                        resource: match value {
                            Binding::Buffer(buffer, offset, len) => {
                                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                    buffer,
                                    offset: *offset,
                                    size: NonZeroU64::new(*len),
                                })
                            }
                            Binding::Image(view) => wgpu::BindingResource::TextureView(view),
                            Binding::Sampler(s) => wgpu::BindingResource::Sampler(s),
                        },
                    })
                    .collect();
                self.worker
                    .device
                    .create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("portable pass bindings"),
                        layout,
                        entries: &entries,
                    })
            })
            .collect()
    }
    fn encode(&mut self, id: usize) -> Result<()> {
        let node = &self.graph.nodes()[id];
        let outputs: Vec<_> = node
            .outputs()
            .iter()
            .enumerate()
            .map(|(index, d)| {
                if let (
                    c::ContentOperation::ExternalImage(image),
                    c::ResourceDescriptor::Image(desc),
                ) = (node.operation(), d)
                {
                    self.worker.import_external(image, desc)
                } else {
                    Ok(self.allocate(
                        d,
                        self.usages[id][index],
                        match node.operation() {
                            c::ContentOperation::UploadBuffer(data) => Some(data.as_ref()),
                            _ => None,
                        },
                    ))
                }
            })
            .collect::<Result<Vec<_>>>()?;
        if matches!(node.operation(), c::ContentOperation::ExternalImage(_)) {
            self.stats.external_imports += 1;
        } else {
            self.stats.image_allocations += node
                .outputs()
                .iter()
                .filter(|d| matches!(d, c::ResourceDescriptor::Image(_)))
                .count();
        }
        let mut owners = outputs.clone();
        let mut encoder =
            self.worker
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("portable content operation"),
                });
        for out in &outputs {
            if let Resource::Buffer { buffer, .. } = out.as_ref() {
                if !matches!(node.operation(), c::ContentOperation::UploadBuffer(_)) {
                    encoder.clear_buffer(buffer, 0, None);
                }
            }
        }
        match node.operation() {
            c::ContentOperation::Solid(color) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                clear(&mut encoder, outputs[0].texture(), d, color.rgba());
            }
            c::ContentOperation::UploadBuffer(_) | c::ContentOperation::ZeroBuffer => {}
            c::ContentOperation::CopyBuffer { source, offset } => self.copy_buffer(
                &mut encoder,
                resource(&self.resources, *source).buffer(),
                *offset,
                outputs[0].buffer(),
                node.outputs()[0].byte_len(),
                &mut owners,
            )?,
            c::ContentOperation::CopyImage { source } => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                copy_image(
                    &mut encoder,
                    resource(&self.resources, *source).texture(),
                    outputs[0].texture(),
                    d,
                );
            }
            c::ContentOperation::ExternalImage(_) => {}
            c::ContentOperation::UploadImage(data) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                let layout = mip_layout(d);
                let staging_buffer = self.worker.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("portable image upload"),
                    size: image_charge(d),
                    usage: wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: true,
                });
                {
                    let mut mapped = staging_buffer
                        .slice(..)
                        .get_mapped_range_mut()
                        .map_err(|error| execution(error.to_string()))?;
                    let mut at = 0;
                    for (offset, _, h, row, packed) in &layout {
                        for y in 0..*h {
                            let start = (*offset + u64::from(y) * u64::from(*row)) as usize;
                            mapped
                                .slice(start..start + *packed as usize)
                                .copy_from_slice(&data[at..at + *packed as usize]);
                            at += *packed as usize;
                        }
                    }
                }
                staging_buffer.unmap();
                let staging = Arc::new(Resource::Buffer {
                    buffer: staging_buffer,
                    charge: image_charge(d),
                });
                for (level, (offset, w, h, row, _)) in layout.iter().enumerate() {
                    encoder.copy_buffer_to_texture(
                        wgpu::TexelCopyBufferInfo {
                            buffer: staging.buffer(),
                            layout: wgpu::TexelCopyBufferLayout {
                                offset: *offset,
                                bytes_per_row: Some(*row),
                                rows_per_image: Some(*h),
                            },
                        },
                        wgpu::TexelCopyTextureInfo {
                            texture: outputs[0].texture(),
                            mip_level: level as u32,
                            origin: Default::default(),
                            aspect: wgpu::TextureAspect::All,
                        },
                        wgpu::Extent3d {
                            width: *w,
                            height: *h,
                            depth_or_array_layers: 1,
                        },
                    );
                }
                owners.push(staging);
            }
            c::ContentOperation::Compute(pass) => {
                for b in pass.bindings() {
                    if let c::BoundResource::Output { output, initial } = b.resource() {
                        match (initial, &node.outputs()[*output]) {
                            (
                                Some(c::InitialResource::Image(r)),
                                c::ResourceDescriptor::Image(d),
                            ) => copy_image(
                                &mut encoder,
                                resource(&self.resources, *r).texture(),
                                outputs[*output].texture(),
                                d,
                            ),
                            (Some(c::InitialResource::Buffer(r)), _) => self.copy_buffer(
                                &mut encoder,
                                resource(&self.resources, r.source).buffer(),
                                r.offset,
                                outputs[*output].buffer(),
                                r.byte_len,
                                &mut owners,
                            )?,
                            (None, c::ResourceDescriptor::Image(d)) => clear(
                                &mut encoder,
                                outputs[*output].texture(),
                                d,
                                [
                                    0.,
                                    0.,
                                    0.,
                                    if d.interpretation().alpha == c::AlphaMode::Opaque {
                                        1.
                                    } else {
                                        0.
                                    },
                                ],
                            ),
                            _ => {}
                        }
                    }
                }
                let pipeline = self.pipelines[id].as_ref().unwrap();
                let groups = self.groups(pipeline, pass.bindings(), &outputs, &mut owners);
                let PipelineState::Compute(state) = &pipeline.state else {
                    unreachable!()
                };
                let mut encoder = encoder.begin_compute_pass(&Default::default());
                encoder.set_pipeline(state);
                for (index, group) in groups.iter().enumerate() {
                    encoder.set_bind_group(index as u32, group, &[]);
                }
                let [x, y, z] = pass.dispatch();
                encoder.dispatch_workgroups(x, y, z);
            }
            c::ContentOperation::Render(pass) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                let load = match pass.load() {
                    c::ResolvedLoad::Clear(bits) => {
                        let [r, g, b, a] = bits.map(f32::from_bits);
                        wgpu::LoadOp::Clear(wgpu::Color {
                            r: r as f64,
                            g: g as f64,
                            b: b as f64,
                            a: a as f64,
                        })
                    }
                    c::ResolvedLoad::Load(source) => {
                        copy_image(
                            &mut encoder,
                            resource(&self.resources, *source).texture(),
                            outputs[0].texture(),
                            d,
                        );
                        wgpu::LoadOp::Load
                    }
                };
                let pipeline = self.pipelines[id].as_ref().unwrap();
                let groups = self.groups(pipeline, pass.bindings(), &outputs, &mut owners);
                let PipelineState::Render(state) = &pipeline.state else {
                    unreachable!()
                };
                let view = outputs[0]
                    .texture()
                    .create_view(&wgpu::TextureViewDescriptor {
                        mip_level_count: Some(1),
                        ..Default::default()
                    });
                let mut encoder = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("portable render pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                encoder.set_pipeline(state);
                for (index, group) in groups.iter().enumerate() {
                    encoder.set_bind_group(index as u32, group, &[]);
                }
                match pass.geometry() {
                    c::ResolvedGeometry::Fullscreen => encoder.draw(0..3, 0..1),
                    c::ResolvedGeometry::Mesh {
                        vertices,
                        index,
                        elements,
                        instances,
                        vertex_count,
                    } => {
                        for (slot, vertex) in vertices.iter().enumerate() {
                            let b = &vertex.buffer;
                            encoder.set_vertex_buffer(
                                slot as u32,
                                resource(&self.resources, b.source).buffer().slice(
                                    b.offset
                                        ..b.offset
                                            + b.byte_len.min(
                                                u64::from(match vertex.layout.step {
                                                    c::VertexStep::Vertex => *vertex_count,
                                                    c::VertexStep::Instance => instances.end,
                                                }) * u64::from(vertex.layout.stride),
                                            ),
                                ),
                            );
                        }
                        if let Some((b, format)) = index {
                            encoder.set_index_buffer(
                                resource(&self.resources, b.source)
                                    .buffer()
                                    .slice(b.offset..b.offset + b.byte_len),
                                match format {
                                    c::IndexFormat::Uint16 => wgpu::IndexFormat::Uint16,
                                    c::IndexFormat::Uint32 => wgpu::IndexFormat::Uint32,
                                },
                            );
                            encoder.draw_indexed(elements.clone(), 0, instances.clone());
                        } else {
                            encoder.draw(elements.clone(), instances.clone());
                        }
                    }
                }
            }
        }
        for owner in owners {
            self.tracker.retain_owner(id, owner)?;
        }
        self.resources[id] = Some(outputs);
        let lease = self.tracker.submitted(id)?;
        let signal = Signal::new();
        self.worker.queue.submit([encoder.finish()]);
        let completed = signal.clone();
        self.worker
            .queue
            .on_submitted_work_done(move || completed.finish());
        self.flights.push_back(Flight {
            node: id,
            signal,
            _lease: lease,
        });
        self.stats.operations += 1;
        self.stats.submissions += 1;
        self.stats.max_in_flight = self.stats.max_in_flight.max(self.flights.len());
        Ok(())
    }
}
