use super::*;
use block2::RcBlock;
use objc2::{rc::Retained, runtime::ProtocolObject};
use std::{collections::HashSet, ptr::NonNull, rc::Rc};

struct Mip {
    offset: usize,
    row: usize,
    width: usize,
    height: usize,
    packed_row: usize,
}
fn mip_layout(d: &c::ImageDescriptor) -> Result<(Vec<Mip>, u64)> {
    let mut offset = 0usize;
    let mut levels = Vec::new();
    for level in 0..d.mip_levels() {
        let width = ((d.extent().width() >> level).max(1)) as usize;
        let height = ((d.extent().height() >> level).max(1)) as usize;
        let packed_row = width * d.format().bytes_per_pixel() as usize;
        let row = packed_row.div_ceil(256) * 256;
        levels.push(Mip {
            offset,
            row,
            width,
            height,
            packed_row,
        });
        offset = offset
            .checked_add(
                row.checked_mul(height)
                    .ok_or_else(|| allocation("mip staging overflow"))?,
            )
            .ok_or_else(|| allocation("mip staging overflow"))?;
    }
    Ok((levels, offset as u64))
}
struct Completion {
    node: usize,
    result: Result<()>,
}
struct Prepared {
    outputs: Vec<Arc<Resource>>,
    owners: Vec<Arc<Resource>>,
    uniforms: HashMap<Arc<str>, Arc<Resource>>,
    sizes: Option<Arc<Resource>>,
    upload: Option<Arc<Resource>>,
    samplers: HashMap<Arc<str>, Retained<ProtocolObject<dyn MTLSamplerState>>>,
}
struct Run<'a> {
    worker: &'a Worker,
    graph: Arc<c::ContentGraph>,
    tracker: c::ExecutionTracker<Arc<Resource>>,
    resources: Vec<Option<Vec<Arc<Resource>>>>,
    pipelines: Vec<Option<Rc<Pipeline>>>,
    usages: Vec<Vec<MTLTextureUsage>>,
    charges: Vec<u64>,
    sender: mpsc::Sender<Completion>,
    receiver: mpsc::Receiver<Completion>,
    in_flight: usize,
    stats: MetalContentStats,
    readback_headroom: u64,
}
impl Worker {
    pub(super) fn execute(
        &mut self,
        graph: Arc<c::ContentGraph>,
        limits: MetalContentLimits,
        cancelled: &AtomicBool,
    ) -> Result<MetalContentFrame> {
        if cancelled.load(Ordering::Acquire) {
            return Err(failure(
                c::ContentErrorKind::Cancelled,
                "content job canceled before execution",
            ));
        }
        let mut stats = MetalContentStats {
            worker_thread: std::thread::current().name().map(str::to_owned),
            ..Default::default()
        };
        let mut pipelines = Vec::new();
        let mut variants = HashSet::new();
        for node in graph.nodes() {
            let pipeline = if let Some(key) = PipelineKey::of(node) {
                variants.insert(key.clone());
                if variants.len() > self.options.pipeline_cache_entries {
                    return Err(failure(
                        c::ContentErrorKind::ResourceLimit,
                        "graph exceeds configured native pipeline-variant budget",
                    ));
                }
                Some(objc2::rc::autoreleasepool(|_| {
                    self.pipeline(key, &mut stats)
                })?)
            } else {
                None
            };
            pipelines.push(pipeline);
        }
        let mut usages: Vec<Vec<_>> = graph
            .nodes()
            .iter()
            .map(|n| {
                n.outputs()
                    .iter()
                    .map(|_| MTLTextureUsage::ShaderRead)
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
                    _ => {
                        return Err(failure(
                            c::ContentErrorKind::UnsupportedCapability,
                            "graph resource exceeds Metal device limits",
                        ));
                    }
                }
            }
            match node.operation() {
                c::ContentOperation::Solid(_) | c::ContentOperation::Render(_) => {
                    usages[id][0] |= MTLTextureUsage::RenderTarget
                }
                c::ContentOperation::Compute(pass) => {
                    pass.program()
                        .validate_dispatch(pass.dispatch(), self.capabilities.compute.as_ref())?;
                    for binding in pass.bindings() {
                        if let c::BoundResource::Output { output, initial } = binding.resource()
                            && let c::ResourceDescriptor::Image(d) = &node.outputs()[*output]
                        {
                            if !self
                                .capabilities
                                .storage_image_formats
                                .contains(&d.format())
                            {
                                return Err(failure(
                                    c::ContentErrorKind::UnsupportedCapability,
                                    "unsupported Metal storage texture format",
                                ));
                            }
                            usages[id][*output] |= MTLTextureUsage::ShaderWrite;
                            if initial.is_none() {
                                usages[id][*output] |= MTLTextureUsage::RenderTarget;
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        let mut readback_bytes = 0u64;
        let mut readback_headroom = 0;
        for root in graph.roots() {
            let c::ResourceDescriptor::Image(d) =
                &graph.nodes()[root.node()].outputs()[root.output()]
            else {
                unreachable!()
            };
            readback_bytes = readback_bytes
                .checked_add(d.byte_len())
                .ok_or_else(|| allocation("readback size overflow"))?;
            readback_headroom =
                readback_headroom.max(buffer_charge(&self.device, mip_layout(d)?.1)?);
        }
        if readback_bytes > limits.max_readback_bytes {
            return Err(failure(
                c::ContentErrorKind::ResourceLimit,
                "requested CPU readback exceeds configured byte budget",
            ));
        }
        let available = limits
            .max_gpu_bytes
            .checked_sub(readback_headroom)
            .ok_or_else(|| {
                failure(
                    c::ContentErrorKind::ResourceLimit,
                    "GPU budget cannot fit readback staging",
                )
            })?;
        let mut charges = Vec::new();
        for (id, node) in graph.nodes().iter().enumerate() {
            let mut charge = 0u64;
            for (output, d) in node.outputs().iter().enumerate() {
                charge = charge
                    .checked_add(match d {
                        c::ResourceDescriptor::Image(d) => objc2::rc::autoreleasepool(|_| {
                            self.device
                                .heapTextureSizeAndAlignWithDescriptor(&texture_descriptor(
                                    d,
                                    usages[id][output],
                                ))
                                .size as u64
                        })
                        .max(d.byte_len()),
                        c::ResourceDescriptor::Buffer(d) => {
                            buffer_charge(&self.device, d.byte_len())?
                        }
                    })
                    .ok_or_else(|| allocation("allocation charge overflow"))?;
            }
            if let c::ContentOperation::UploadImage(_) = node.operation() {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                charge = charge
                    .checked_add(buffer_charge(&self.device, mip_layout(d)?.1)?)
                    .ok_or_else(|| allocation("upload charge overflow"))?;
            }
            let bindings = match node.operation() {
                c::ContentOperation::Render(p) => p.bindings(),
                c::ContentOperation::Compute(p) => p.bindings(),
                _ => &[],
            };
            for binding in bindings {
                if let c::BoundResource::Uniform(bytes) = binding.resource() {
                    charge = charge
                        .checked_add(buffer_charge(&self.device, bytes.len() as u64)?)
                        .ok_or_else(|| allocation("uniform charge overflow"))?;
                }
            }
            if let Some(pipeline) = &pipelines[id] {
                let words = pipeline.sizes.len() + pipeline.vertices.len();
                if words != 0 {
                    charge = charge
                        .checked_add(buffer_charge(&self.device, (words * 4) as u64)?)
                        .ok_or_else(|| allocation("size-table charge overflow"))?;
                }
            }
            charges.push(charge);
        }
        let plan = c::ExecutionPlan::new(
            graph.clone(),
            charges.clone(),
            c::ExecutionLimits {
                max_live_bytes: available,
                max_recording_nodes: 1,
            },
        )?;
        let tracker = plan.start_with_owner::<Arc<Resource>>();
        let (sender, receiver) = mpsc::channel();
        let mut run = Run {
            worker: self,
            resources: (0..graph.nodes().len()).map(|_| None).collect(),
            graph,
            tracker,
            pipelines,
            usages,
            charges,
            sender,
            receiver,
            in_flight: 0,
            stats,
            readback_headroom,
        };
        let result = run.run(&limits, cancelled);
        // Even returned errors/cancellation retire the submitted prefix before the
        // next job can spend this worker's device-memory budget.
        let cleanup = run.drain();
        // Admission can succeed before waiting for a native queue slot. If a
        // prior submission fails during that wait, no recorder exists for this
        // reservation, but it still needs an explicit discard acknowledgment.
        for id in 0..run.resources.len() {
            if run.tracker.state(id)? == c::ExecutionState::Encoding {
                run.tracker.discarded(id)?;
            }
        }
        run.tracker.cancel_unstarted();
        run.tracker.release_roots();
        run.retire();
        debug_assert!(run.tracker.is_quiescent());
        match result {
            Err(error) => Err(error),
            Ok(images) => {
                cleanup?;
                Ok(MetalContentFrame {
                    images,
                    stats: run.stats,
                })
            }
        }
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
    fn event(&mut self, event: Completion) -> Result<()> {
        self.in_flight -= 1;
        self.tracker.completed(event.node, event.result.is_ok())?;
        self.retire();
        event.result.map_err(|error| {
            failure(
                error.kind,
                format!("content operation {}: {}", event.node, error.message),
            )
        })
    }
    fn receive(&mut self) -> Result<()> {
        let event = self
            .receiver
            .recv()
            .map_err(|_| execution("Metal completion channel closed"))?;
        self.event(event)
    }
    fn poll(&mut self) -> Result<()> {
        while let Ok(event) = self.receiver.try_recv() {
            self.event(event)?;
        }
        Ok(())
    }
    fn drain(&mut self) -> Result<()> {
        let mut first = None;
        while self.in_flight != 0 {
            if let Err(error) = self.receive() {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
    fn run(
        &mut self,
        limits: &MetalContentLimits,
        cancelled: &AtomicBool,
    ) -> Result<Vec<MetalContentImage>> {
        for id in 0..self.graph.nodes().len() {
            self.poll()?;
            #[cfg(feature = "test-support")]
            if let Some((node, gate)) = &limits.pause_before_node
                && *node == id
            {
                gate.arrive();
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(failure(
                    c::ContentErrorKind::Cancelled,
                    "content job canceled; submitted prefix retires",
                ));
            }
            #[cfg(feature = "test-support")]
            if limits.fail_before_node == Some(id) {
                return Err(execution("injected failure before content operation"));
            }
            loop {
                match self.tracker.try_admit(id)? {
                    c::Admission::Admitted => break,
                    c::Admission::Deferred(_) if self.in_flight != 0 => self.receive()?,
                    c::Admission::Deferred(_) => {
                        return Err(execution(
                            "admission cannot advance despite a feasible serial plan",
                        ));
                    }
                }
            }
            #[cfg(feature = "test-support")]
            if limits.fail_after_admit == Some(id) {
                return Err(execution("injected failure after admission"));
            }
            while self.in_flight >= self.worker.options.queue_capacity {
                self.receive()?;
            }
            self.stats.peak_reserved_bytes = self
                .stats
                .peak_reserved_bytes
                .max(self.tracker.live_bytes() + self.readback_headroom);
            let result = objc2::rc::autoreleasepool(|_| self.encode(id));
            if let Err(error) = result {
                self.tracker.discarded(id)?;
                self.retire();
                return Err(error);
            }
        }
        self.drain()?;
        let mut images = Vec::new();
        for root in self.graph.roots() {
            if cancelled.load(Ordering::Acquire) {
                return Err(failure(
                    c::ContentErrorKind::Cancelled,
                    "content readback canceled",
                ));
            }
            if !self.tracker.is_valid(root.node())? {
                return Err(execution("cannot read back an invalid root"));
            }
            images.push(objc2::rc::autoreleasepool(|_| self.readback(*root))?);
            self.stats.readbacks += 1;
            self.stats.max_in_flight = self.stats.max_in_flight.max(1);
        }
        Ok(images)
    }
    fn prepare(&self, id: usize) -> Result<Prepared> {
        let node = &self.graph.nodes()[id];
        let mut prepared = Prepared {
            outputs: Vec::new(),
            owners: Vec::new(),
            uniforms: HashMap::new(),
            sizes: None,
            upload: None,
            samplers: HashMap::new(),
        };
        for (output, d) in node.outputs().iter().enumerate() {
            let resource = match d {
                c::ResourceDescriptor::Image(d) => Arc::new(Resource::Image(
                    self.worker
                        .device
                        .newTextureWithDescriptor(&texture_descriptor(d, self.usages[id][output]))
                        .ok_or_else(|| allocation("allocate Metal content texture"))?,
                )),
                c::ResourceDescriptor::Buffer(d) => {
                    allocate_buffer(&self.worker.device, d.byte_len())?
                }
            };
            prepared.owners.push(resource.clone());
            prepared.outputs.push(resource);
        }
        match node.operation() {
            c::ContentOperation::UploadBuffer(bytes) => unsafe {
                std::ptr::copy_nonoverlapping(
                    bytes.as_ptr(),
                    prepared.outputs[0]
                        .buffer()
                        .contents()
                        .as_ptr()
                        .cast::<u8>(),
                    bytes.len(),
                );
            },
            c::ContentOperation::UploadImage(bytes) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                let (mips, len) = mip_layout(d)?;
                let staging = allocate_buffer(&self.worker.device, len)?;
                let pointer = staging.buffer().contents().as_ptr().cast::<u8>();
                let mut source = 0;
                for mip in &mips {
                    for row in 0..mip.height {
                        // SAFETY: validated packed payload and newly allocated staging range.
                        unsafe {
                            std::ptr::copy_nonoverlapping(
                                bytes.as_ptr().add(source),
                                pointer.add(mip.offset + row * mip.row),
                                mip.packed_row,
                            );
                        }
                        source += mip.packed_row;
                    }
                }
                prepared.owners.push(staging.clone());
                prepared.upload = Some(staging);
            }
            _ => {}
        }
        let bindings = match node.operation() {
            c::ContentOperation::Render(p) => p.bindings(),
            c::ContentOperation::Compute(p) => p.bindings(),
            _ => &[],
        };
        for binding in bindings {
            match binding.resource() {
                c::BoundResource::Uniform(bytes) => {
                    let buffer = allocate_buffer(&self.worker.device, bytes.len() as u64)?;
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            bytes.as_ptr(),
                            buffer.buffer().contents().as_ptr().cast::<u8>(),
                            bytes.len(),
                        );
                    }
                    prepared.owners.push(buffer.clone());
                    prepared.uniforms.insert(binding.name().into(), buffer);
                }
                c::BoundResource::Sampler(sampler) => {
                    let d = MTLSamplerDescriptor::new();
                    d.setMinFilter(filter(sampler.min_filter));
                    d.setMagFilter(filter(sampler.mag_filter));
                    d.setMipFilter(match sampler.mip_filter {
                        None => MTLSamplerMipFilter::NotMipmapped,
                        Some(c::FilterMode::Nearest) => MTLSamplerMipFilter::Nearest,
                        Some(c::FilterMode::Linear) => MTLSamplerMipFilter::Linear,
                    });
                    d.setSAddressMode(address(sampler.address[0]));
                    d.setTAddressMode(address(sampler.address[1]));
                    d.setLodMinClamp(0.);
                    d.setLodMaxClamp(if sampler.mip_filter.is_none() {
                        0.
                    } else {
                        f32::MAX
                    });
                    prepared.samplers.insert(
                        binding.name().into(),
                        self.worker
                            .device
                            .newSamplerStateWithDescriptor(&d)
                            .ok_or_else(|| allocation("allocate Metal sampler"))?,
                    );
                }
                _ => {}
            }
        }
        if let Some(pipeline) = &self.pipelines[id] {
            let mut lengths = Vec::new();
            for global in &pipeline.sizes {
                let len = global
                    .as_ref()
                    .and_then(|name| {
                        bindings
                            .iter()
                            .find(|binding| binding.name() == name.as_ref())
                    })
                    .map(|binding| match binding.resource() {
                        c::BoundResource::Buffer(b) => b.byte_len,
                        c::BoundResource::Output { output, .. } => {
                            node.outputs()[*output].byte_len()
                        }
                        _ => 0,
                    })
                    .unwrap_or(0);
                lengths
                    .push(u32::try_from(len).map_err(|_| {
                        execution("runtime buffer length exceeds Metal size table")
                    })?);
            }
            if let c::ContentOperation::Render(pass) = node.operation()
                && let c::ResolvedGeometry::Mesh {
                    vertices,
                    vertex_count,
                    instances,
                    ..
                } = pass.geometry()
            {
                for slot in &pipeline.vertices {
                    let vertex = vertices.iter().find(|v| v.slot == *slot).unwrap();
                    let count = match vertex.layout.step {
                        c::VertexStep::Vertex => *vertex_count,
                        c::VertexStep::Instance => instances.end,
                    };
                    let accessible = u64::from(count) * u64::from(vertex.layout.stride);
                    lengths.push(vertex.buffer.byte_len.min(accessible) as u32);
                }
            }
            if !lengths.is_empty() {
                let buffer = allocate_buffer(&self.worker.device, (lengths.len() * 4) as u64)?;
                for (index, len) in lengths.iter().enumerate() {
                    unsafe {
                        std::ptr::copy_nonoverlapping(
                            len.to_le_bytes().as_ptr(),
                            buffer
                                .buffer()
                                .contents()
                                .as_ptr()
                                .cast::<u8>()
                                .add(index * 4),
                            4,
                        );
                    }
                }
                prepared.owners.push(buffer.clone());
                prepared.sizes = Some(buffer);
            }
        }
        let actual: u64 = prepared.owners.iter().map(|r| r.allocated_bytes()).sum();
        if actual > self.charges[id] {
            return Err(failure(
                c::ContentErrorKind::ResourceLimit,
                format!(
                    "operation {id} allocated {actual} Metal bytes; reserved {}",
                    self.charges[id]
                ),
            ));
        }
        Ok(prepared)
    }
    fn encode(&mut self, id: usize) -> Result<()> {
        let prepared = self.prepare(id)?;
        for owner in &prepared.owners {
            self.tracker.retain_owner(id, owner.clone())?;
        }
        let command = self
            .worker
            .queue
            .commandBuffer()
            .ok_or_else(|| allocation("allocate Metal content command buffer"))?;
        command.setLabel(Some(&objc2_foundation::NSString::from_str(&format!(
            "xgpui.content operation {id}"
        ))));
        let node = &self.graph.nodes()[id];
        match node.operation() {
            c::ContentOperation::Solid(color) => clear_image(
                &command,
                prepared.outputs[0].image(),
                color.rgba().map(f64::from),
            )?,
            c::ContentOperation::UploadBuffer(_) | c::ContentOperation::ZeroBuffer => {} // Fully initialized before publication.
            c::ContentOperation::UploadImage(_) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                upload_image(
                    &command,
                    prepared.upload.as_ref().unwrap().buffer(),
                    prepared.outputs[0].image(),
                    d,
                )?;
            }
            c::ContentOperation::CopyImage { source } => {
                copy_image(
                    &command,
                    resource(&self.resources, *source).image(),
                    prepared.outputs[0].image(),
                )?;
            }
            c::ContentOperation::CopyBuffer { source, offset } => {
                self.stats.byte_copy_dispatches += self.copy_buffer(
                    &command,
                    resource(&self.resources, *source).buffer(),
                    *offset,
                    prepared.outputs[0].buffer(),
                    node.outputs()[0].byte_len(),
                )?;
            }
            c::ContentOperation::Compute(pass) => {
                for binding in pass.bindings() {
                    if let c::BoundResource::Output { output, initial } = binding.resource() {
                        match (initial, &node.outputs()[*output]) {
                            (Some(c::InitialResource::Image(image)), _) => copy_image(
                                &command,
                                resource(&self.resources, *image).image(),
                                prepared.outputs[*output].image(),
                            )?,
                            (Some(c::InitialResource::Buffer(b)), _) => {
                                self.stats.byte_copy_dispatches += self.copy_buffer(
                                    &command,
                                    resource(&self.resources, b.source).buffer(),
                                    b.offset,
                                    prepared.outputs[*output].buffer(),
                                    b.byte_len,
                                )?
                            }
                            (None, c::ResourceDescriptor::Image(d)) => clear_image(
                                &command,
                                prepared.outputs[*output].image(),
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
                            )?,
                            _ => {}
                        }
                    }
                }
                let pipeline = self.pipelines[id].as_ref().unwrap();
                let PipelineState::Compute(state) = &pipeline.state else {
                    unreachable!()
                };
                let encoder = command
                    .computeCommandEncoder()
                    .ok_or_else(|| allocation("Metal compute encoder"))?;
                encoder.setComputePipelineState(state);
                self.bind_compute(&encoder, pipeline, pass.bindings(), &prepared);
                for (index, length) in pipeline.workgroup_memory.iter().enumerate() {
                    unsafe {
                        encoder.setThreadgroupMemoryLength_atIndex(*length, index);
                    }
                }
                encoder.dispatchThreadgroups_threadsPerThreadgroup(
                    mtl_size(pass.dispatch()),
                    mtl_size(pipeline.workgroup),
                );
                encoder.endEncoding();
            }
            c::ContentOperation::Render(pass) => {
                if let c::ResolvedLoad::Load(image) = pass.load() {
                    copy_image(
                        &command,
                        resource(&self.resources, *image).image(),
                        prepared.outputs[0].image(),
                    )?;
                }
                let descriptor = RenderPassDescriptor::new();
                let attachment =
                    unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
                attachment.setTexture(Some(prepared.outputs[0].image()));
                attachment.setStoreAction(MTLStoreAction::Store);
                match pass.load() {
                    c::ResolvedLoad::Clear(bits) => {
                        attachment.setLoadAction(MTLLoadAction::Clear);
                        let [red, green, blue, alpha] = bits.map(|b| f64::from(f32::from_bits(b)));
                        attachment.setClearColor(MTLClearColor {
                            red,
                            green,
                            blue,
                            alpha,
                        });
                    }
                    c::ResolvedLoad::Load(_) => attachment.setLoadAction(MTLLoadAction::Load),
                }
                let pipeline = self.pipelines[id].as_ref().unwrap();
                let PipelineState::Render(state) = &pipeline.state else {
                    unreachable!()
                };
                let encoder = command
                    .renderCommandEncoderWithDescriptor(&descriptor)
                    .ok_or_else(|| allocation("Metal render encoder"))?;
                encoder.setRenderPipelineState(state);
                self.bind_render(&encoder, pipeline, pass.bindings(), &prepared);
                let t = prepared.outputs[0].image();
                encoder.setViewport(MTLViewport {
                    originX: 0.,
                    originY: 0.,
                    width: t.width() as f64,
                    height: t.height() as f64,
                    znear: 0.,
                    zfar: 1.,
                });
                match pass.geometry() {
                    c::ResolvedGeometry::Fullscreen => unsafe {
                        encoder.drawPrimitives_vertexStart_vertexCount(
                            MTLPrimitiveType::Triangle,
                            0,
                            3,
                        );
                    },
                    c::ResolvedGeometry::Mesh {
                        vertices,
                        index,
                        elements,
                        instances,
                        ..
                    } => {
                        for vertex in vertices {
                            unsafe {
                                encoder.setVertexBuffer_offset_atIndex(
                                    Some(resource(&self.resources, vertex.buffer.source).buffer()),
                                    vertex.buffer.offset as usize,
                                    (VERTEX_BASE + vertex.slot) as usize,
                                );
                            }
                        }
                        if let Some((indices, format)) = index {
                            let (kind, width) = match format {
                                c::IndexFormat::Uint16 => (MTLIndexType::UInt16, 2),
                                c::IndexFormat::Uint32 => (MTLIndexType::UInt32, 4),
                            };
                            unsafe {
                                encoder.drawIndexedPrimitives_indexCount_indexType_indexBuffer_indexBufferOffset_instanceCount_baseVertex_baseInstance(MTLPrimitiveType::Triangle, (elements.end - elements.start) as usize, kind, resource(&self.resources, indices.source).buffer(), indices.offset as usize + elements.start as usize * width, (instances.end - instances.start) as usize, 0, instances.start as usize);
                            }
                        } else {
                            unsafe {
                                encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(MTLPrimitiveType::Triangle, elements.start as usize, (elements.end - elements.start) as usize, (instances.end - instances.start) as usize, instances.start as usize);
                            }
                        }
                    }
                }
                encoder.endEncoding();
            }
        }
        let lease = self.tracker.submitted(id)?;
        let completion = Mutex::new(Some(lease));
        let sender = self.sender.clone();
        let block = RcBlock::new(
            move |command: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                let result =
                    objc2::rc::autoreleasepool(|_| command_result(unsafe { command.as_ref() }));
                drop(completion.lock().unwrap().take());
                let _ = sender.send(Completion { node: id, result });
            },
        );
        unsafe {
            command.addCompletedHandler(RcBlock::as_ptr(&block));
        }
        self.resources[id] = Some(prepared.outputs);
        self.in_flight += 1;
        self.stats.max_in_flight = self.stats.max_in_flight.max(self.in_flight);
        self.stats.operations += 1;
        self.stats.submissions += 1;
        command.commit();
        Ok(())
    }
    fn copy_buffer(
        &self,
        command: &CommandBufferRef,
        source: &metal::BufferRef,
        offset: u64,
        target: &metal::BufferRef,
        len: u64,
    ) -> Result<usize> {
        if offset.is_multiple_of(4) && len.is_multiple_of(4) {
            let encoder = command
                .blitCommandEncoder()
                .ok_or_else(|| allocation("Metal copy encoder"))?;
            unsafe {
                encoder.copyFromBuffer_sourceOffset_toBuffer_destinationOffset_size(
                    source,
                    offset as usize,
                    target,
                    0,
                    len as usize,
                );
            }
            encoder.endEncoding();
        } else {
            let encoder = command
                .computeCommandEncoder()
                .ok_or_else(|| allocation("Metal byte-copy encoder"))?;
            encoder.setComputePipelineState(&self.worker.byte_copy);
            let range = [offset, len];
            unsafe {
                encoder.setBuffer_offset_atIndex(Some(source), 0, 0);
                encoder.setBuffer_offset_atIndex(Some(target), 0, 1);
                encoder.setBytes_length_atIndex(
                    NonNull::new(range.as_ptr().cast_mut().cast()).unwrap(),
                    16,
                    2,
                );
            }
            let width = self
                .worker
                .byte_copy
                .maxTotalThreadsPerThreadgroup()
                .min(256);
            encoder.dispatchThreadgroups_threadsPerThreadgroup(
                MTLSize {
                    width: (len as usize).div_ceil(width),
                    height: 1,
                    depth: 1,
                },
                MTLSize {
                    width,
                    height: 1,
                    depth: 1,
                },
            );
            encoder.endEncoding();
        }
        Ok(usize::from(
            !offset.is_multiple_of(4) || !len.is_multiple_of(4),
        ))
    }
    fn bound<'a>(
        &'a self,
        binding: &'a c::BoundResource,
        prepared: &'a Prepared,
    ) -> Option<(&'a Resource, usize)> {
        match binding {
            c::BoundResource::Image(id) => Some((resource(&self.resources, *id), 0)),
            c::BoundResource::Buffer(b) => {
                Some((resource(&self.resources, b.source), b.offset as usize))
            }
            c::BoundResource::Output { output, .. } => Some((&prepared.outputs[*output], 0)),
            _ => None,
        }
    }
    fn bind_compute(
        &self,
        encoder: &ProtocolObject<dyn MTLComputeCommandEncoder>,
        pipeline: &Pipeline,
        bindings: &[c::ResolvedBinding],
        prepared: &Prepared,
    ) {
        for binding in bindings {
            let slot = pipeline.slots[binding.name()];
            match binding.resource() {
                c::BoundResource::Uniform(_) => {
                    if let Slot::Buffer(slot) = slot {
                        unsafe {
                            encoder.setBuffer_offset_atIndex(
                                Some(prepared.uniforms[binding.name()].buffer()),
                                0,
                                slot,
                            );
                        }
                    }
                }
                c::BoundResource::Sampler(_) => {
                    if let Slot::Sampler(slot) = slot {
                        unsafe {
                            encoder.setSamplerState_atIndex(
                                Some(&prepared.samplers[binding.name()]),
                                slot,
                            );
                        }
                    }
                }
                value => {
                    if let Some((resource, offset)) = self.bound(value, prepared) {
                        unsafe {
                            match slot {
                                Slot::Buffer(slot) => encoder.setBuffer_offset_atIndex(
                                    Some(resource.buffer()),
                                    offset,
                                    slot,
                                ),
                                Slot::Texture(slot) => {
                                    encoder.setTexture_atIndex(Some(resource.image()), slot)
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                }
            }
        }
        if let Some(buffer) = &prepared.sizes {
            unsafe {
                encoder.setBuffer_offset_atIndex(Some(buffer.buffer()), 0, SIZE_SLOT);
            }
        }
    }
    fn bind_render(
        &self,
        encoder: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        pipeline: &Pipeline,
        bindings: &[c::ResolvedBinding],
        prepared: &Prepared,
    ) {
        for binding in bindings {
            let slot = pipeline.slots[binding.name()];
            unsafe {
                match binding.resource() {
                    c::BoundResource::Uniform(_) => {
                        if let Slot::Buffer(slot) = slot {
                            let b = prepared.uniforms[binding.name()].buffer();
                            encoder.setVertexBuffer_offset_atIndex(Some(b), 0, slot);
                            encoder.setFragmentBuffer_offset_atIndex(Some(b), 0, slot);
                        }
                    }
                    c::BoundResource::Sampler(_) => {
                        if let Slot::Sampler(slot) = slot {
                            let sampler = &prepared.samplers[binding.name()];
                            encoder.setVertexSamplerState_atIndex(Some(sampler), slot);
                            encoder.setFragmentSamplerState_atIndex(Some(sampler), slot);
                        }
                    }
                    value => {
                        if let Some((resource, offset)) = self.bound(value, prepared) {
                            match slot {
                                Slot::Buffer(slot) => {
                                    encoder.setVertexBuffer_offset_atIndex(
                                        Some(resource.buffer()),
                                        offset,
                                        slot,
                                    );
                                    encoder.setFragmentBuffer_offset_atIndex(
                                        Some(resource.buffer()),
                                        offset,
                                        slot,
                                    );
                                }
                                Slot::Texture(slot) => {
                                    encoder.setVertexTexture_atIndex(Some(resource.image()), slot);
                                    encoder
                                        .setFragmentTexture_atIndex(Some(resource.image()), slot);
                                }
                                _ => unreachable!(),
                            }
                        }
                    }
                }
            }
        }
        if let Some(buffer) = &prepared.sizes {
            unsafe {
                encoder.setVertexBuffer_offset_atIndex(Some(buffer.buffer()), 0, SIZE_SLOT);
                encoder.setFragmentBuffer_offset_atIndex(Some(buffer.buffer()), 0, SIZE_SLOT);
            }
        }
    }
    fn readback(&self, root: c::ResourceId) -> Result<MetalContentImage> {
        let c::ResourceDescriptor::Image(d) =
            &self.graph.nodes()[root.node()].outputs()[root.output()]
        else {
            unreachable!()
        };
        let (mips, len) = mip_layout(d)?;
        let staging = allocate_buffer(&self.worker.device, len)?;
        if staging.allocated_bytes() > self.readback_headroom {
            return Err(failure(
                c::ContentErrorKind::ResourceLimit,
                "actual readback staging exceeds reservation",
            ));
        }
        let source = resource(&self.resources, root).clone();
        let command = self
            .worker
            .queue
            .commandBuffer()
            .ok_or_else(|| allocation("Metal readback command"))?;
        let encoder = command
            .blitCommandEncoder()
            .ok_or_else(|| allocation("Metal readback encoder"))?;
        for (level, mip) in mips.iter().enumerate() {
            unsafe {
                encoder.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage_options(source.image(), 0, level, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: mip.width, height: mip.height, depth: 1 }, staging.buffer(), mip.offset, mip.row, mip.row * mip.height, MTLBlitOption::empty());
            }
        }
        encoder.endEncoding();
        let (tx, rx) = mpsc::sync_channel(1);
        let retained = Mutex::new(Some((source, staging.clone())));
        let block = RcBlock::new(
            move |command: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                let result =
                    objc2::rc::autoreleasepool(|_| command_result(unsafe { command.as_ref() }));
                drop(retained.lock().unwrap().take());
                let _ = tx.send(result);
            },
        );
        unsafe {
            command.addCompletedHandler(RcBlock::as_ptr(&block));
        }
        command.commit();
        rx.recv()
            .map_err(|_| execution("Metal readback completion closed"))??;
        let mut levels = Vec::new();
        let pointer = staging.buffer().contents().as_ptr().cast::<u8>();
        for mip in &mips {
            let mut bytes = Vec::with_capacity(mip.packed_row * mip.height);
            for row in 0..mip.height {
                unsafe {
                    bytes.extend_from_slice(std::slice::from_raw_parts(
                        pointer.add(mip.offset + row * mip.row),
                        mip.packed_row,
                    ));
                }
            }
            levels.push(bytes.into());
        }
        Ok(MetalContentImage {
            descriptor: d.clone(),
            levels,
        })
    }
}
fn command_result(command: &CommandBufferRef) -> Result<()> {
    if command.status() == MTLCommandBufferStatus::Completed {
        Ok(())
    } else {
        Err(execution(format!(
            "Metal command failed: {:?}; {:?}",
            command.status(),
            command.error()
        )))
    }
}
fn filter(filter: c::FilterMode) -> MTLSamplerMinMagFilter {
    match filter {
        c::FilterMode::Nearest => MTLSamplerMinMagFilter::Nearest,
        c::FilterMode::Linear => MTLSamplerMinMagFilter::Linear,
    }
}
fn address(address: c::AddressMode) -> MTLSamplerAddressMode {
    match address {
        c::AddressMode::Clamp => MTLSamplerAddressMode::ClampToEdge,
        c::AddressMode::Repeat => MTLSamplerAddressMode::Repeat,
        c::AddressMode::MirrorRepeat => MTLSamplerAddressMode::MirrorRepeat,
    }
}
fn mtl_size(size: [u32; 3]) -> MTLSize {
    MTLSize {
        width: size[0] as usize,
        height: size[1] as usize,
        depth: size[2] as usize,
    }
}
fn clear_image(command: &CommandBufferRef, texture: &TextureRef, color: [f64; 4]) -> Result<()> {
    let descriptor = RenderPassDescriptor::new();
    let attachment = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
    attachment.setTexture(Some(texture));
    attachment.setLoadAction(MTLLoadAction::Clear);
    attachment.setStoreAction(MTLStoreAction::Store);
    let [red, green, blue, alpha] = color;
    attachment.setClearColor(MTLClearColor {
        red,
        green,
        blue,
        alpha,
    });
    command
        .renderCommandEncoderWithDescriptor(&descriptor)
        .ok_or_else(|| allocation("Metal clear encoder"))?
        .endEncoding();
    Ok(())
}
fn copy_image(command: &CommandBufferRef, source: &TextureRef, target: &TextureRef) -> Result<()> {
    let encoder = command
        .blitCommandEncoder()
        .ok_or_else(|| allocation("Metal image-copy encoder"))?;
    for level in 0..source.mipmapLevelCount() {
        unsafe {
            encoder.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(source, 0, level, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: (source.width() >> level).max(1), height: (source.height() >> level).max(1), depth: 1 }, target, 0, level, MTLOrigin { x: 0, y: 0, z: 0 });
        }
    }
    encoder.endEncoding();
    Ok(())
}
fn upload_image(
    command: &CommandBufferRef,
    staging: &metal::BufferRef,
    target: &TextureRef,
    descriptor: &c::ImageDescriptor,
) -> Result<()> {
    let (mips, _) = mip_layout(descriptor)?;
    let encoder = command
        .blitCommandEncoder()
        .ok_or_else(|| allocation("Metal upload encoder"))?;
    for (level, mip) in mips.iter().enumerate() {
        unsafe {
            encoder.copyFromBuffer_sourceOffset_sourceBytesPerRow_sourceBytesPerImage_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin_options(staging, mip.offset, mip.row, mip.row * mip.height, MTLSize { width: mip.width, height: mip.height, depth: 1 }, target, 0, level, MTLOrigin { x: 0, y: 0, z: 0 }, MTLBlitOption::empty());
        }
    }
    encoder.endEncoding();
    Ok(())
}
