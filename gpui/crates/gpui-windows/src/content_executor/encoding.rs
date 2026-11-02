use super::*;
use std::collections::HashSet;
use translation::Slot;
type Resources = Vec<Option<Vec<Arc<Resource>>>>;
fn resource(resources: &Resources, id: c::ResourceId) -> &Arc<Resource> {
    &resources[id.node()].as_ref().unwrap()[id.output()]
}
fn padded(bytes: u64) -> u64 {
    bytes.div_ceil(4) * 4
}
fn buffer_charge(bytes: u64) -> u64 {
    bytes.div_ceil(4096) * 4096
}
fn image_charge(d: &c::ImageDescriptor) -> u64 {
    let bytes: u64 = (0..d.mip_levels())
        .map(|l| {
            u64::from((d.extent().width() >> l).max(1) * d.format().bytes_per_pixel() as u32)
                .div_ceil(256)
                * 256
                * u64::from((d.extent().height() >> l).max(1))
        })
        .sum();
    bytes.div_ceil(65536) * 65536
}
fn charge(d: &c::ResourceDescriptor) -> u64 {
    match d {
        c::ResourceDescriptor::Image(d) => image_charge(d),
        c::ResourceDescriptor::Buffer(d) => buffer_charge(d.byte_len()),
    }
}
fn buffer(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    bytes: u64,
    uniform: bool,
    data: Option<&[u8]>,
) -> Result<Arc<Resource>> {
    let len = if uniform {
        bytes.div_ceil(16) * 16
    } else {
        padded(bytes)
    };
    // Upload directly when aligned. A padded upload needs a CPU copy to define
    // its tail; no-data storage buffers are cleared on the recording context.
    let padded_data = data.filter(|data| data.len() != len as usize).map(|data| {
        let mut bytes = vec![0; len as usize];
        bytes[..data.len()].copy_from_slice(data);
        bytes
    });
    let initial = data.map(|data| D3D11_SUBRESOURCE_DATA {
        pSysMem: padded_data.as_deref().unwrap_or(data).as_ptr().cast(),
        ..Default::default()
    });
    let desc = D3D11_BUFFER_DESC {
        ByteWidth: len as u32,
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: if uniform {
            D3D11_BIND_CONSTANT_BUFFER.0 as u32
        } else {
            (D3D11_BIND_SHADER_RESOURCE.0
                | D3D11_BIND_UNORDERED_ACCESS.0
                | D3D11_BIND_VERTEX_BUFFER.0
                | D3D11_BIND_INDEX_BUFFER.0) as u32
        },
        MiscFlags: if uniform {
            0
        } else {
            D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32
        },
        ..Default::default()
    };
    let mut result = None;
    unsafe {
        device
            .CreateBuffer(
                &desc,
                initial.as_ref().map(|data| data as *const _),
                Some(&mut result),
            )
            .map_err(allocation)?;
    }
    let resource = Arc::new(Resource::Buffer {
        buffer: result.ok_or_else(|| allocation("missing portable buffer"))?,
        charge: buffer_charge(len),
    });
    if data.is_none() {
        // All no-data callers create storage/raw buffers, never uniforms.
        unsafe {
            ctx.ClearUnorderedAccessViewUint(&uav(device, &resource)?, &[0; 4]);
        }
    }
    Ok(resource)
}

fn image(
    device: &ID3D11Device,
    ctx: &ID3D11DeviceContext,
    d: &c::ImageDescriptor,
    storage: bool,
    upload: Option<&[u8]>,
) -> Result<Arc<Resource>> {
    // D3D copies the caller's packed mip data during creation; avoid a second
    // CPU image and initialize no-data textures explicitly on the GPU below.
    let mut initial = vec![];
    if let Some(upload) = upload {
        let mut offset = 0;
        for l in 0..d.mip_levels() {
            let w = (d.extent().width() >> l).max(1);
            let h = (d.extent().height() >> l).max(1);
            let row = w * d.format().bytes_per_pixel() as u32;
            initial.push(D3D11_SUBRESOURCE_DATA {
                pSysMem: upload[offset..].as_ptr().cast(),
                SysMemPitch: row,
                ..Default::default()
            });
            offset += row as usize * h as usize;
        }
    }
    let desc = D3D11_TEXTURE2D_DESC {
        Width: d.extent().width(),
        Height: d.extent().height(),
        MipLevels: d.mip_levels(),
        ArraySize: 1,
        Format: format(d.format()),
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET.0
            | D3D11_BIND_SHADER_RESOURCE.0
            | if storage {
                D3D11_BIND_UNORDERED_ACCESS.0
            } else {
                0
            }) as u32,
        ..Default::default()
    };
    let mut t = None;
    unsafe {
        device
            .CreateTexture2D(&desc, upload.map(|_| initial.as_ptr()), Some(&mut t))
            .map_err(allocation)?;
    }
    let resource = Arc::new(Resource::Image {
        texture: t.ok_or_else(|| allocation("missing portable texture"))?,
        charge: image_charge(d),
    });
    if upload.is_none() {
        let alpha = f32::from(d.interpretation().alpha == c::AlphaMode::Opaque);
        for mip in 0..d.mip_levels() {
            unsafe {
                ctx.ClearRenderTargetView(
                    &rtv(device, resource.texture(), mip)?,
                    &[0., 0., 0., alpha],
                );
            }
        }
    }
    Ok(resource)
}

fn rtv(device: &ID3D11Device, t: &ID3D11Texture2D, mip: u32) -> Result<ID3D11RenderTargetView> {
    let mut view = None;
    let desc = D3D11_RENDER_TARGET_VIEW_DESC {
        Format: DXGI_FORMAT_UNKNOWN,
        ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
        Anonymous: D3D11_RENDER_TARGET_VIEW_DESC_0 {
            Texture2D: D3D11_TEX2D_RTV { MipSlice: mip },
        },
    };
    unsafe {
        device
            .CreateRenderTargetView(t, Some(&desc), Some(&mut view))
            .map_err(allocation)?;
    }
    view.ok_or_else(|| allocation("missing render target view"))
}
fn srv_image(device: &ID3D11Device, t: &ID3D11Texture2D) -> Result<ID3D11ShaderResourceView> {
    let mut v = None;
    unsafe {
        device
            .CreateShaderResourceView(t, None, Some(&mut v))
            .map_err(allocation)?;
    }
    v.ok_or_else(|| allocation("missing sampled image view"))
}
fn srv_buffer(
    device: &ID3D11Device,
    b: &ID3D11Buffer,
    offset: u64,
    len: u64,
) -> Result<ID3D11ShaderResourceView> {
    let desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
        Format: DXGI_FORMAT_R32_TYPELESS,
        ViewDimension: D3D_SRV_DIMENSION_BUFFEREX,
        Anonymous: D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
            BufferEx: D3D11_BUFFEREX_SRV {
                FirstElement: (offset / 4) as u32,
                NumElements: (padded(len) / 4) as u32,
                Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
            },
        },
    };
    let mut v = None;
    unsafe {
        device
            .CreateShaderResourceView(b, Some(&desc), Some(&mut v))
            .map_err(allocation)?;
    }
    v.ok_or_else(|| allocation("missing storage view"))
}
fn uav(device: &ID3D11Device, r: &Resource) -> Result<ID3D11UnorderedAccessView> {
    let mut view = None;
    unsafe {
        match r {
            Resource::ExternalImage { .. } => {
                return Err(error(
                    c::ContentErrorKind::InvalidDescriptor,
                    "external images are read-only",
                ));
            }
            Resource::Image { texture, .. } => device
                .CreateUnorderedAccessView(texture, None, Some(&mut view))
                .map_err(allocation)?,
            Resource::Buffer { buffer, .. } => {
                let mut b = D3D11_BUFFER_DESC::default();
                buffer.GetDesc(&mut b);
                let desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
                    Format: DXGI_FORMAT_R32_TYPELESS,
                    ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                    Anonymous: D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                        Buffer: D3D11_BUFFER_UAV {
                            FirstElement: 0,
                            NumElements: b.ByteWidth / 4,
                            Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
                        },
                    },
                };
                device
                    .CreateUnorderedAccessView(buffer, Some(&desc), Some(&mut view))
                    .map_err(allocation)?;
            }
        }
    }
    view.ok_or_else(|| allocation("missing output view"))
}
fn sampler(device: &ID3D11Device, s: c::Sampler) -> Result<ID3D11SamplerState> {
    let bit = |f| u32::from(f == c::FilterMode::Linear);
    let filter = D3D11_FILTER(
        (bit(s.min_filter) << 4
            | bit(s.mag_filter) << 2
            | bit(s.mip_filter.unwrap_or(c::FilterMode::Nearest))) as i32,
    );
    let address = |a| match a {
        c::AddressMode::Clamp => D3D11_TEXTURE_ADDRESS_CLAMP,
        c::AddressMode::Repeat => D3D11_TEXTURE_ADDRESS_WRAP,
        c::AddressMode::MirrorRepeat => D3D11_TEXTURE_ADDRESS_MIRROR,
    };
    let desc = D3D11_SAMPLER_DESC {
        Filter: filter,
        AddressU: address(s.address[0]),
        AddressV: address(s.address[1]),
        AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
        ComparisonFunc: D3D11_COMPARISON_NEVER,
        MaxLOD: if s.mip_filter.is_some() { f32::MAX } else { 0. },
        ..Default::default()
    };
    let mut v = None;
    unsafe {
        device
            .CreateSamplerState(&desc, Some(&mut v))
            .map_err(allocation)?;
    }
    v.ok_or_else(|| allocation("missing sampler"))
}
pub(super) struct Flight {
    node: usize,
    query: ID3D11Query,
    _lease: c::SubmissionLease<Arc<Resource>>,
}
struct Run<'a> {
    worker: &'a Worker,
    graph: Arc<c::ContentGraph>,
    tracker: c::ExecutionTracker<Arc<Resource>>,
    resources: Resources,
    pipelines: Vec<Option<Arc<Pipeline>>>,
    flights: VecDeque<Flight>,
    stats: Dx11ContentStats,
}
impl Worker {
    pub fn execute(
        &mut self,
        graph: Arc<c::ContentGraph>,
        limits: Dx11ContentLimits,
        cancelled: &AtomicBool,
        readback: bool,
    ) -> Result<Dx11ContentFrame> {
        if unsafe { self.device.GetFeatureLevel() }.0 < D3D_FEATURE_LEVEL_11_0.0 {
            return Err(error(
                c::ContentErrorKind::UnsupportedCapability,
                "portable D3D11 execution requires feature level 11.0",
            ));
        }
        if self.recording_failed.load(Ordering::Acquire) {
            return Err(execution(
                "D3D11 recorder could not discard a failed recording",
            ));
        }
        if !self.retire_quarantine() {
            return Err(execution(
                "previous D3D11 work has not retired after a completion timeout",
            ));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(error(
                c::ContentErrorKind::Cancelled,
                "D3D11 content canceled before execution",
            ));
        }
        // Validate every adapter/device/descriptor before recording any prefix.
        for node in graph.nodes() {
            if let c::ContentOperation::ExternalImage(image) = node.operation() {
                let lease = image.acquire()?;
                let frame = lease
                    .backing::<crate::custom_draw::dxgi::DxgiFrame>()
                    .ok_or_else(|| {
                        error(
                            c::ContentErrorKind::UnsupportedCapability,
                            "unsupported D3D11 external backing",
                        )
                    })?;
                frame.import_external(&self.device, image.descriptor())?;
            }
        }
        let byte_copy_before = self.byte_copy_compilations.load(Ordering::Relaxed);
        let mut stats = Dx11ContentStats {
            worker_thread: Some("dx11-portable-content".into()),
            ..Default::default()
        };
        let mut variants = HashSet::new();
        for n in graph.nodes() {
            if let Some(k) = PipelineKey::of(n) {
                variants.insert(k);
            }
        }
        if variants.len() > self.options.pipeline_cache_entries {
            return Err(error(
                c::ContentErrorKind::ResourceLimit,
                "graph exceeds pipeline cache bound",
            ));
        }
        let mut pipelines = vec![];
        for n in graph.nodes() {
            pipelines.push(match PipelineKey::of(n) {
                Some(k) => Some(self.pipeline(k, &mut stats)?),
                None => None,
            });
        }
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
                "readback exceeds byte budget",
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
            .map(|n| {
                let mut total = n.outputs().iter().map(charge).sum::<u64>();
                let bindings = match n.operation() {
                    c::ContentOperation::Render(p) => {
                        if let c::ResolvedGeometry::Mesh {
                            vertices,
                            index,
                            vertex_count,
                            instances,
                            ..
                        } = p.geometry()
                        {
                            for v in vertices {
                                let count = if v.layout.step == c::VertexStep::Vertex {
                                    *vertex_count
                                } else {
                                    instances.end
                                };
                                total +=
                                    buffer_charge(u64::from(count) * u64::from(v.layout.stride));
                            }
                            if let Some((b, _)) = index {
                                total += buffer_charge(b.byte_len);
                            }
                        }
                        p.bindings()
                    }
                    c::ContentOperation::Compute(p) => p.bindings(),
                    _ => &[],
                };
                for b in bindings {
                    if let c::BoundResource::Uniform(data) = b.resource() {
                        total += buffer_charge(data.len() as u64);
                    }
                } // Byte-copy constants and possible temporary bounded slices.
                total + 4096 * (bindings.len() as u64 + 2)
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
            flights: VecDeque::new(),
            stats,
        };
        let outcome = run.run(&limits, cancelled);
        let drain = run.drain();
        outcome?;
        drain?;
        let mut owners = HashMap::new();
        let mut gpu = vec![];
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
        stats.byte_copy_compilations =
            (self.byte_copy_compilations.load(Ordering::Relaxed) - byte_copy_before) as usize;
        drop(run.resources);
        drop(run.tracker);
        let mut images = vec![];
        if readback {
            for image in &gpu {
                images.push(self.read_image(image, &mut stats)?);
            }
            gpu.clear();
        }
        Ok(Dx11ContentFrame { images, gpu, stats })
    }
    fn read_image(
        &self,
        image: &GpuImage,
        stats: &mut Dx11ContentStats,
    ) -> Result<Dx11ContentImage> {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            image.texture().GetDesc(&mut desc);
        }
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
        desc.MiscFlags = 0;
        let mut staging = None;
        unsafe {
            self.device
                .CreateTexture2D(&desc, None, Some(&mut staging))
                .map_err(allocation)?;
        }
        let staging = staging.ok_or_else(|| allocation("missing readback texture"))?;
        let q = query(&self.device)?;
        unsafe {
            self.deferred.ClearState();
            self.deferred.CopyResource(&staging, image.texture());
            self.deferred.End(&q);
        }
        self.submit()?;
        wait(&self.device, &self.context, &q)?;
        let mut levels = vec![];
        for l in 0..desc.MipLevels {
            let mut map = D3D11_MAPPED_SUBRESOURCE::default();
            unsafe {
                self.context
                    .Map(&staging, l, D3D11_MAP_READ, 0, Some(&mut map))
                    .map_err(execution)?;
            }
            let row = (desc.Width >> l).max(1) * image.descriptor.format().bytes_per_pixel() as u32;
            let h = (desc.Height >> l).max(1);
            let mut bytes = Vec::with_capacity((row * h) as usize);
            for y in 0..h {
                unsafe {
                    bytes.extend_from_slice(std::slice::from_raw_parts(
                        map.pData.cast::<u8>().add((y * map.RowPitch) as usize),
                        row as usize,
                    ));
                }
            }
            unsafe {
                self.context.Unmap(&staging, l);
            }
            levels.push(bytes);
        }
        stats.readbacks += 1;
        Ok(Dx11ContentImage {
            descriptor: image.descriptor.clone(),
            levels,
        })
    }
    fn finish(&self) -> Result<ID3D11CommandList> {
        let mut commands = None;
        unsafe {
            self.deferred
                .FinishCommandList(false, Some(&mut commands))
                .map_err(execution)?;
        }
        commands.ok_or_else(|| execution("missing command list"))
    }
    fn execute_commands(&self, commands: &ID3D11CommandList) -> Result<()> {
        unsafe {
            self.context.ExecuteCommandList(commands, true);
            self.context.Flush();
            self.device.GetDeviceRemovedReason().map_err(execution)?;
        }
        Ok(())
    }
    fn submit(&self) -> Result<()> {
        let commands = self.finish()?;
        self.execute_commands(&commands)
    }
    fn copy_buffer(
        &self,
        src: &ID3D11Buffer,
        offset: u64,
        dst: &ID3D11Buffer,
        len: u64,
        owners: &mut Vec<Arc<Resource>>,
    ) -> Result<()> {
        if offset.is_multiple_of(4) && len.is_multiple_of(4) {
            let b = D3D11_BOX {
                left: offset as u32,
                right: (offset + len) as u32,
                top: 0,
                bottom: 1,
                front: 0,
                back: 1,
            };
            unsafe {
                self.deferred
                    .CopySubresourceRegion(dst, 0, 0, 0, 0, src, 0, Some(&b));
            }
            return Ok(());
        }
        let shader = {
            let mut cached = self.byte_copy_shader.lock().unwrap();
            if cached.is_none() {
                let code=crate::custom_draw::compile("ByteAddressBuffer src:register(t0); RWByteAddressBuffer dst:register(u0); cbuffer Params:register(b0){uint offset;uint len;uint2 pad;} [numthreads(64,1,1)] void copy_bytes(uint3 id:SV_DispatchThreadID){uint start=id.x*4;if(start>=len)return;uint word=0;for(uint i=0;i<4;i++){if(start+i<len){uint at=offset+start+i;word|=((src.Load((at/4)*4)>>((at%4)*8))&255)<<(i*8);}}dst.Store(start,word);}","copy_bytes","cs_5_0").map_err(execution)?;
                let mut shader = None;
                unsafe {
                    self.device
                        .CreateComputeShader(&code, None, Some(&mut shader))
                        .map_err(allocation)?;
                }
                let shader = shader.ok_or_else(|| allocation("missing byte copy shader"))?;
                self.byte_copy_compilations.fetch_add(1, Ordering::Relaxed);
                *cached = Some(shader);
            }
            cached.as_ref().unwrap().clone()
        };
        let mut desc = D3D11_BUFFER_DESC::default();
        unsafe {
            src.GetDesc(&mut desc);
        }
        let view = srv_buffer(&self.device, src, 0, u64::from(desc.ByteWidth))?;
        let temporary = Resource::Buffer {
            buffer: dst.clone(),
            charge: 0,
        };
        let output = uav(&self.device, &temporary)?;
        let values = [offset as u32, len as u32, 0, 0]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        let params = buffer(&self.device, &self.deferred, 16, true, Some(&values))?;
        unsafe {
            self.deferred.CSSetShader(&shader, None);
            self.deferred.CSSetShaderResources(0, Some(&[Some(view)]));
            self.deferred
                .CSSetUnorderedAccessViews(0, 1, Some([Some(output)].as_ptr()), None);
            self.deferred
                .CSSetConstantBuffers(0, Some(&[Some(params.buffer().clone())]));
            self.deferred
                .Dispatch((len.div_ceil(4) as u32).div_ceil(64), 1, 1);
            self.deferred.CSSetShader(None, None);
            self.deferred.CSSetShaderResources(0, Some(&[None]));
            self.deferred
                .CSSetUnorderedAccessViews(0, 1, Some([None].as_ptr()), None);
        }
        owners.push(params);
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
    fn receive(&mut self) -> Result<()> {
        let flight = self
            .flights
            .pop_front()
            .ok_or_else(|| execution("D3D11 admission stalled"))?;
        let result = wait(&self.worker.device, &self.worker.context, &flight.query);
        let node = flight.node;
        if result.is_err() && unsafe { self.worker.device.GetDeviceRemovedReason() }.is_ok() {
            self.worker.quarantine.lock().unwrap().push(flight);
        } else {
            drop(flight);
        }
        self.tracker.completed(node, result.is_ok())?;
        self.retire();
        result
    }
    fn drain(&mut self) -> Result<()> {
        let mut error = None;
        while !self.flights.is_empty() {
            if let Err(e) = self.receive() {
                error = Some(e);
            }
        }
        error.map_or(Ok(()), Err)
    }
    fn run(&mut self, _limits: &Dx11ContentLimits, cancelled: &AtomicBool) -> Result<()> {
        for id in 0..self.graph.nodes().len() {
            while let Some(f) = self.flights.front() {
                if !complete(&self.worker.device, &self.worker.context, &f.query)? {
                    break;
                }
                self.receive()?;
            }
            #[cfg(feature = "test-support")]
            if let Some((node, gate)) = &_limits.pause_before_node {
                if *node == id {
                    gate.enter(cancelled);
                }
            }
            if cancelled.load(Ordering::Acquire) {
                return Err(error(
                    c::ContentErrorKind::Cancelled,
                    "D3D11 content canceled; submitted prefix retires",
                ));
            }
            #[cfg(feature = "test-support")]
            if _limits.fail_before_node == Some(id) {
                return Err(execution("injected portable failure before operation"));
            }
            loop {
                match self.tracker.try_admit(id)? {
                    c::Admission::Admitted => break,
                    c::Admission::Deferred(_) => self.receive()?,
                }
            }
            while self.flights.len() >= self.worker.options.queue_capacity {
                self.receive()?;
            }
            self.stats.peak_reserved_bytes = self
                .stats
                .peak_reserved_bytes
                .max(self.tracker.live_bytes());
            if let Err(e) = self.encode(id, _limits) {
                if self.tracker.state(id)? == c::ExecutionState::Encoding {
                    // Finish and drop the unsubmitted command list before
                    // returning its owners and budget to the execution tracker.
                    if self.worker.finish().is_err() {
                        self.worker.recording_failed.store(true, Ordering::Release);
                    }
                    self.tracker.discarded(id)?;
                }
                self.retire();
                return Err(e);
            }
        }
        Ok(())
    }
    fn bind(
        &self,
        p: &Pipeline,
        bindings: &[c::ResolvedBinding],
        outputs: &[Arc<Resource>],
        owners: &mut Vec<Arc<Resource>>,
    ) -> Result<()> {
        let d = &self.worker.device;
        let ctx = &self.worker.deferred;
        for binding in bindings {
            let slot = p
                .slots
                .get(binding.name())
                .ok_or_else(|| execution("missing reflected HLSL binding"))?;
            unsafe {
                match (slot, binding.resource()) {
                    (Slot::Uniform(i), c::BoundResource::Uniform(bytes)) => {
                        let b = buffer(d, ctx, bytes.len() as u64, true, Some(bytes))?;
                        let values = [Some(b.buffer().clone())];
                        ctx.VSSetConstantBuffers(*i, Some(&values));
                        ctx.PSSetConstantBuffers(*i, Some(&values));
                        ctx.CSSetConstantBuffers(*i, Some(&values));
                        owners.push(b);
                    }
                    (Slot::Sampler(i), c::BoundResource::Sampler(s)) => {
                        let values = [Some(sampler(d, *s)?)];
                        ctx.VSSetSamplers(*i, Some(&values));
                        ctx.PSSetSamplers(*i, Some(&values));
                        ctx.CSSetSamplers(*i, Some(&values));
                    }
                    (Slot::Read(i), r) => {
                        let view = match r {
                            c::BoundResource::Image(id) => {
                                srv_image(d, resource(&self.resources, *id).texture())?
                            }
                            c::BoundResource::Buffer(b) => srv_buffer(
                                d,
                                resource(&self.resources, b.source).buffer(),
                                b.offset,
                                b.byte_len,
                            )?,
                            _ => return Err(execution("invalid read binding")),
                        };
                        let values = [Some(view)];
                        ctx.VSSetShaderResources(*i, Some(&values));
                        ctx.PSSetShaderResources(*i, Some(&values));
                        ctx.CSSetShaderResources(*i, Some(&values));
                    }
                    (Slot::Write(i), c::BoundResource::Output { output, .. }) => {
                        let values = [Some(uav(d, &outputs[*output])?)];
                        ctx.CSSetUnorderedAccessViews(*i, 1, Some(values.as_ptr()), None);
                    }
                    _ => return Err(execution("incompatible HLSL resource binding")),
                }
            }
        }
        Ok(())
    }
    fn encode(&mut self, id: usize, _limits: &Dx11ContentLimits) -> Result<()> {
        let node = &self.graph.nodes()[id];
        let worker = self.worker;
        let d = &worker.device;
        let ctx = &worker.deferred;
        unsafe {
            ctx.ClearState();
        }
        let mut owners = vec![];
        let outputs = node
            .outputs()
            .iter()
            .map(|desc| match desc {
                c::ResourceDescriptor::Image(desc)
                    if matches!(node.operation(), c::ContentOperation::ExternalImage(_)) =>
                {
                    let c::ContentOperation::ExternalImage(external) = node.operation() else {
                        unreachable!()
                    };
                    let lease = external.acquire()?;
                    let frame = lease
                        .backing::<crate::custom_draw::dxgi::DxgiFrame>()
                        .ok_or_else(|| {
                            error(
                                c::ContentErrorKind::UnsupportedCapability,
                                "unsupported D3D11 external backing",
                            )
                        })?;
                    let texture = frame.import_external(d, desc)?;
                    Ok(Arc::new(Resource::ExternalImage {
                        texture,
                        charge: charge(&c::ResourceDescriptor::Image(desc.clone())),
                        _lease: lease,
                    }))
                }
                c::ResourceDescriptor::Image(desc) => image(
                    d,
                    ctx,
                    desc,
                    matches!(node.operation(), c::ContentOperation::Compute(_)),
                    match node.operation() {
                        c::ContentOperation::UploadImage(bytes) => Some(bytes.as_ref()),
                        _ => None,
                    },
                ),
                c::ResourceDescriptor::Buffer(desc) => buffer(
                    d,
                    ctx,
                    desc.byte_len(),
                    false,
                    match node.operation() {
                        c::ContentOperation::UploadBuffer(bytes) => Some(bytes.as_ref()),
                        _ => None,
                    },
                ),
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
        match node.operation() {
            c::ContentOperation::Solid(color) => {
                let c::ResourceDescriptor::Image(desc) = &node.outputs()[0] else {
                    unreachable!()
                };
                for mip in 0..desc.mip_levels() {
                    unsafe {
                        ctx.ClearRenderTargetView(
                            &rtv(d, outputs[0].texture(), mip)?,
                            &color.rgba(),
                        );
                    }
                }
            }
            c::ContentOperation::ExternalImage(_) => {}
            c::ContentOperation::UploadImage(_)
            | c::ContentOperation::UploadBuffer(_)
            | c::ContentOperation::ZeroBuffer => {}
            c::ContentOperation::CopyImage { source } => unsafe {
                ctx.CopyResource(
                    outputs[0].texture(),
                    resource(&self.resources, *source).texture(),
                );
            },
            c::ContentOperation::CopyBuffer { source, offset } => worker.copy_buffer(
                resource(&self.resources, *source).buffer(),
                *offset,
                outputs[0].buffer(),
                node.outputs()[0].byte_len(),
                &mut owners,
            )?,
            c::ContentOperation::Compute(pass) => {
                for b in pass.bindings() {
                    if let c::BoundResource::Output {
                        output,
                        initial: Some(initial),
                    } = b.resource()
                    {
                        match initial {
                            c::InitialResource::Image(src) => unsafe {
                                ctx.CopyResource(
                                    outputs[*output].texture(),
                                    resource(&self.resources, *src).texture(),
                                );
                            },
                            c::InitialResource::Buffer(src) => worker.copy_buffer(
                                resource(&self.resources, src.source).buffer(),
                                src.offset,
                                outputs[*output].buffer(),
                                src.byte_len,
                                &mut owners,
                            )?,
                        }
                    }
                }
                let p = self.pipelines[id].as_ref().unwrap();
                self.bind(p, pass.bindings(), &outputs, &mut owners)?;
                let [x, y, z] = pass.dispatch();
                let params = [x, y, z, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                let params = buffer(d, ctx, 16, true, Some(&params))?;
                unsafe {
                    ctx.CSSetConstantBuffers(13, Some(&[Some(params.buffer().clone())]));
                }
                owners.push(params);
                unsafe {
                    ctx.CSSetShader(p.compute.as_ref(), None);
                    ctx.Dispatch(x, y, z);
                }
            }
            c::ContentOperation::Render(pass) => {
                let c::ResourceDescriptor::Image(desc) = &node.outputs()[0] else {
                    unreachable!()
                };
                let target = rtv(d, outputs[0].texture(), 0)?;
                match pass.load() {
                    c::ResolvedLoad::Clear(bits) => unsafe {
                        ctx.ClearRenderTargetView(&target, &bits.map(f32::from_bits));
                    },
                    c::ResolvedLoad::Load(src) => unsafe {
                        ctx.CopyResource(
                            outputs[0].texture(),
                            resource(&self.resources, *src).texture(),
                        );
                    },
                }
                let p = self.pipelines[id].as_ref().unwrap();
                self.bind(p, pass.bindings(), &outputs, &mut owners)?;
                unsafe {
                    ctx.VSSetShader(p.vertex.as_ref(), None);
                    ctx.PSSetShader(p.fragment.as_ref(), None);
                    ctx.IASetInputLayout(p.input.as_ref());
                    ctx.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                    ctx.RSSetState(p.raster.as_ref());
                    ctx.RSSetViewports(Some(&[D3D11_VIEWPORT {
                        Width: desc.extent().width() as f32,
                        Height: desc.extent().height() as f32,
                        MaxDepth: 1.,
                        ..Default::default()
                    }]));
                    ctx.OMSetRenderTargets(Some(&[Some(target)]), None);
                    ctx.OMSetBlendState(p.blend.as_ref(), None, u32::MAX);
                }
                let (first_vertex, first_instance) = match pass.geometry() {
                    c::ResolvedGeometry::Fullscreen => (0, 0),
                    c::ResolvedGeometry::Mesh {
                        index,
                        elements,
                        instances,
                        ..
                    } => (
                        if index.is_some() { 0 } else { elements.start },
                        instances.start,
                    ),
                };
                let params = [first_vertex, first_instance, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_le_bytes)
                    .collect::<Vec<_>>();
                let params = buffer(d, ctx, 16, true, Some(&params))?;
                unsafe {
                    ctx.VSSetConstantBuffers(13, Some(&[Some(params.buffer().clone())]));
                }
                owners.push(params);
                match pass.geometry() {
                    c::ResolvedGeometry::Fullscreen => unsafe {
                        ctx.Draw(3, 0);
                    },
                    c::ResolvedGeometry::Mesh {
                        vertices,
                        index,
                        vertex_count,
                        elements,
                        instances,
                    } => {
                        for (slot, v) in vertices.iter().enumerate() {
                            let count = if v.layout.step == c::VertexStep::Vertex {
                                *vertex_count
                            } else {
                                instances.end
                            };
                            let len = u64::from(count) * u64::from(v.layout.stride);
                            let bounded = buffer(d, ctx, len, false, None)?;
                            worker.copy_buffer(
                                resource(&self.resources, v.buffer.source).buffer(),
                                v.buffer.offset,
                                bounded.buffer(),
                                len,
                                &mut owners,
                            )?;
                            unsafe {
                                ctx.IASetVertexBuffers(
                                    slot as u32,
                                    1,
                                    Some([Some(bounded.buffer().clone())].as_ptr()),
                                    Some(&v.layout.stride),
                                    Some(&0),
                                );
                            }
                            owners.push(bounded);
                        }
                        if let Some((b, format)) = index {
                            let bounded = buffer(d, ctx, b.byte_len, false, None)?;
                            worker.copy_buffer(
                                resource(&self.resources, b.source).buffer(),
                                b.offset,
                                bounded.buffer(),
                                b.byte_len,
                                &mut owners,
                            )?;
                            unsafe {
                                ctx.IASetIndexBuffer(
                                    bounded.buffer(),
                                    match format {
                                        c::IndexFormat::Uint16 => DXGI_FORMAT_R16_UINT,
                                        c::IndexFormat::Uint32 => DXGI_FORMAT_R32_UINT,
                                    },
                                    0,
                                );
                                ctx.DrawIndexedInstanced(
                                    elements.end - elements.start,
                                    instances.end - instances.start,
                                    elements.start,
                                    0,
                                    instances.start,
                                );
                            }
                            owners.push(bounded);
                        } else {
                            unsafe {
                                ctx.DrawInstanced(
                                    elements.end - elements.start,
                                    instances.end - instances.start,
                                    elements.start,
                                    instances.start,
                                );
                            }
                        }
                    }
                }
            }
        }
        #[cfg(feature = "test-support")]
        if _limits.fail_after_record == Some(id) {
            return Err(execution("injected failure after portable recording"));
        }
        let q = query(d)?;
        unsafe {
            ctx.ClearState();
            ctx.End(&q);
        }
        let commands = worker.finish()?;
        for owner in outputs.iter().cloned().chain(owners) {
            self.tracker.retain_owner(id, owner)?;
        }
        self.resources[id] = Some(outputs);
        let lease = self.tracker.submitted(id)?;
        self.flights.push_back(Flight {
            node: id,
            query: q,
            _lease: lease,
        });
        worker.execute_commands(&commands)?;
        self.stats.operations += 1;
        self.stats.submissions += 1;
        self.stats.max_in_flight = self.stats.max_in_flight.max(self.flights.len());
        Ok(())
    }
}

impl Worker {
    pub(super) fn retire_quarantine(&self) -> bool {
        self.quarantine.lock().unwrap().retain(|flight| {
            matches!(
                complete(&self.device, &self.context, &flight.query),
                Ok(false)
            )
        });
        self.quarantine.lock().unwrap().is_empty()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        while !self.retire_quarantine() || !self.retire_sampling() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
