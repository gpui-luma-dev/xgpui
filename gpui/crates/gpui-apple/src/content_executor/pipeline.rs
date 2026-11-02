use super::*;
use naga::back::msl;
use objc2_foundation::NSString;

pub(super) const SIZE_SLOT: usize = 30;
pub(super) const VERTEX_BASE: u32 = 16;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) enum PipelineKey {
    Render {
        program: c::PortableProgram,
        format: c::PixelFormat,
        blend: c::PassBlend,
        streams: Vec<(u32, c::VertexLayout)>,
    },
    Compute(c::PortableProgram),
}
impl PipelineKey {
    pub(super) fn of(node: &c::ContentNode) -> Option<Self> {
        match node.operation() {
            c::ContentOperation::Render(pass) => {
                let c::ResourceDescriptor::Image(d) = &node.outputs()[0] else {
                    unreachable!()
                };
                let streams = match pass.geometry() {
                    c::ResolvedGeometry::Mesh { vertices, .. } => vertices
                        .iter()
                        .map(|v| (v.slot, v.layout.clone()))
                        .collect(),
                    _ => Vec::new(),
                };
                Some(Self::Render {
                    program: pass.program().clone(),
                    format: d.format(),
                    blend: pass.blend(),
                    streams,
                })
            }
            c::ContentOperation::Compute(pass) => Some(Self::Compute(pass.program().clone())),
            _ => None,
        }
    }
}
pub(super) enum PipelineState {
    Render(RenderPipelineState),
    Compute(ComputePipelineState),
}
#[derive(Clone, Copy)]
pub(super) enum Slot {
    Buffer(usize),
    Texture(usize),
    Sampler(usize),
}
pub(super) struct Pipeline {
    pub(super) state: PipelineState,
    pub(super) slots: HashMap<Arc<str>, Slot>,
    pub(super) workgroup_memory: Vec<usize>,
    pub(super) sizes: Vec<Option<Arc<str>>>,
    pub(super) vertices: Vec<u32>,
    pub(super) workgroup: [u32; 3],
}
impl Worker {
    pub(super) fn pipeline(
        &mut self,
        key: PipelineKey,
        stats: &mut MetalContentStats,
    ) -> Result<std::rc::Rc<Pipeline>> {
        if let Some(pipeline) = self.pipelines.get(&key) {
            return Ok(pipeline.clone());
        }
        let pipeline = std::rc::Rc::new(compile(&self.device, &key)?);
        if self.pipelines.len() >= self.options.pipeline_cache_entries {
            let remove = self.pipelines.keys().next().cloned().unwrap();
            self.pipelines.remove(&remove);
        }
        self.pipelines.insert(key, pipeline.clone());
        stats.pipeline_compilations += 1;
        Ok(pipeline)
    }
}
fn compile(device: &DeviceRef, key: &PipelineKey) -> Result<Pipeline> {
    let (program, streams) = match key {
        PipelineKey::Render {
            program, streams, ..
        } => (program, streams.as_slice()),
        PipelineKey::Compute(program) => (program, &[][..]),
    };
    let mut buffers = 0usize;
    let mut textures = 0usize;
    let mut samplers = 0usize;
    let mut slots = HashMap::new();
    let mut resources = msl::EntryPointResources {
        sizes_buffer: Some(SIZE_SLOT as u8),
        ..Default::default()
    };
    for binding in program.bindings() {
        let mut target = msl::BindTarget::default();
        let slot = match binding.kind() {
            c::ProgramBindingKind::Uniform(_) | c::ProgramBindingKind::StorageBuffer { .. } => {
                target.buffer = Some(buffers as u8);
                let slot = Slot::Buffer(buffers);
                buffers += 1;
                slot
            }
            c::ProgramBindingKind::SampledImage(_) | c::ProgramBindingKind::StorageImage { .. } => {
                target.texture = Some(textures as u8);
                let slot = Slot::Texture(textures);
                textures += 1;
                slot
            }
            c::ProgramBindingKind::Sampler => {
                target.sampler = Some(msl::BindSamplerTarget::Resource(samplers as u8));
                let slot = Slot::Sampler(samplers);
                samplers += 1;
                slot
            }
        };
        target.mutable = matches!(
            binding.kind(),
            c::ProgramBindingKind::StorageBuffer {
                access: c::StorageAccess::ReadWrite | c::StorageAccess::WriteOnly,
                ..
            } | c::ProgramBindingKind::StorageImage {
                access: c::StorageAccess::ReadWrite | c::StorageAccess::WriteOnly,
                ..
            }
        );
        resources.resources.insert(
            naga::ResourceBinding {
                group: binding.address().group,
                binding: binding.address().binding,
            },
            target,
        );
        slots.insert(Arc::from(binding.name()), slot);
    }
    if buffers > VERTEX_BASE as usize || textures > 31 || samplers > 16 {
        return Err(failure(
            c::ContentErrorKind::UnsupportedCapability,
            "program exceeds direct Metal buffer/texture/sampler slots",
        ));
    }
    let module = naga::front::wgsl::parse_str(program.wgsl())
        .map_err(|e| execution(e.emit_to_string(program.wgsl())))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| execution(e.to_string()))?;
    let sizes = module
        .global_variables
        .iter()
        .filter(|(_, var)| {
            module.types[var.ty]
                .inner
                .needs_host_buffer_byte_size(&module.types)
        })
        .map(|(_, var)| {
            var.name
                .as_deref()
                .filter(|name| program.binding(name).is_some())
                .map(Arc::from)
        })
        .collect();
    let vertices: Vec<_> = streams.iter().map(|(slot, _)| *slot).collect();
    // Naga emits used workgroup globals as dynamic threadgroup arguments in
    // declaration order. Metal requires their lengths on every compute encoder.
    let mut workgroup_memory = Vec::new();
    if let Some(entry) = program.compute_entry() {
        let index = module
            .entry_points
            .iter()
            .position(|ep| ep.name == entry && ep.stage == naga::ShaderStage::Compute)
            .unwrap();
        let entry_info = info.get_entry_point(index);
        for (handle, var) in module.global_variables.iter() {
            if var.space == naga::AddressSpace::WorkGroup && !entry_info[handle].is_empty() {
                workgroup_memory.push(
                    (module.types[var.ty].inner.size(module.to_ctx()) as usize)
                        .next_multiple_of(16),
                );
            }
        }
    }
    let state = match key {
        PipelineKey::Compute(_) => {
            let entry = program.compute_entry().unwrap();
            let function = translate(
                device,
                &module,
                &info,
                entry,
                naga::ShaderStage::Compute,
                &resources,
                &[],
            )?;
            let state = device
                .newComputePipelineStateWithFunction_error(&function)
                .map_err(|e| execution(format!("Metal compute pipeline: {e}")))?;
            let group = program.workgroup_size().unwrap();
            let count = u64::from(group[0]) * u64::from(group[1]) * u64::from(group[2]);
            if count > state.maxTotalThreadsPerThreadgroup() as u64
                || state.staticThreadgroupMemoryLength() + workgroup_memory.iter().sum::<usize>()
                    > device.maxThreadgroupMemoryLength()
            {
                return Err(failure(
                    c::ContentErrorKind::UnsupportedCapability,
                    "compiled compute pipeline exceeds native threadgroup limits",
                ));
            }
            PipelineState::Compute(state)
        }
        PipelineKey::Render { format, blend, .. } => {
            let (vertex, fragment) = program.render_entries().unwrap();
            let vertex = translate(
                device,
                &module,
                &info,
                vertex,
                naga::ShaderStage::Vertex,
                &resources,
                streams,
            )?;
            let fragment = translate(
                device,
                &module,
                &info,
                fragment,
                naga::ShaderStage::Fragment,
                &resources,
                &[],
            )?;
            let descriptor = RenderPipelineDescriptor::new();
            descriptor.setVertexFunction(Some(&vertex));
            descriptor.setFragmentFunction(Some(&fragment));
            let color = unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) };
            color.setPixelFormat(pixel_format(*format));
            if *blend == c::PassBlend::SourceOver {
                color.setBlendingEnabled(true);
                color.setSourceRGBBlendFactor(MTLBlendFactor::One);
                color.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
                color.setSourceAlphaBlendFactor(MTLBlendFactor::One);
                color.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
            }
            PipelineState::Render(
                device
                    .newRenderPipelineStateWithDescriptor_error(&descriptor)
                    .map_err(|e| execution(format!("Metal render pipeline: {e}")))?,
            )
        }
    };
    Ok(Pipeline {
        state,
        slots,
        workgroup_memory,
        sizes,
        vertices,
        workgroup: program.workgroup_size().unwrap_or([1; 3]),
    })
}
fn translate(
    device: &DeviceRef,
    module: &naga::Module,
    info: &naga::valid::ModuleInfo,
    entry: &str,
    stage: naga::ShaderStage,
    resources: &msl::EntryPointResources,
    streams: &[(u32, c::VertexLayout)],
) -> Result<Function> {
    let check = naga::proc::BoundsCheckPolicy::ReadZeroSkipWrite;
    let mut options = msl::Options {
        lang_version: (2, 1),
        fake_missing_bindings: false,
        bounds_check_policies: naga::proc::BoundsCheckPolicies {
            index: check,
            buffer: check,
            image_load: check,
            ..Default::default()
        },
        ..Default::default()
    };
    options
        .per_entry_point_map
        .insert(entry.to_string(), resources.clone());
    let mappings = streams
        .iter()
        .map(|(slot, layout)| msl::VertexBufferMapping {
            id: VERTEX_BASE + *slot,
            stride: layout.stride,
            step_mode: match layout.step {
                c::VertexStep::Vertex => msl::VertexBufferStepMode::ByVertex,
                c::VertexStep::Instance => msl::VertexBufferStepMode::ByInstance,
            },
            attributes: layout
                .attributes
                .iter()
                .map(|a| msl::AttributeMapping {
                    shader_location: a.location,
                    offset: a.offset,
                    format: vertex_format(a.ty),
                })
                .collect(),
        })
        .collect();
    let pipeline = msl::PipelineOptions {
        entry_point: Some((stage, entry.to_string())),
        vertex_pulling_transform: true,
        vertex_buffer_mappings: mappings,
        ..Default::default()
    };
    let (source, translation) = msl::write_string(module, info, &options, &pipeline)
        .map_err(|e| execution(format!("MSL translation: {e}")))?;
    let name = translation
        .entry_point_names
        .into_iter()
        .next()
        .ok_or_else(|| execution("MSL translation produced no entry"))?
        .map_err(|e| execution(format!("MSL entry: {e}")))?;
    let compile = CompileOptions::new();
    compile.setLanguageVersion(MTLLanguageVersion::Version2_1);
    compile.setFastMathEnabled(false);
    let library = device
        .newLibraryWithSource_options_error(&NSString::from_str(&source), Some(&compile))
        .map_err(|e| execution(format!("Metal shader compilation: {e}\n{source}")))?;
    library
        .newFunctionWithName(&NSString::from_str(&name))
        .ok_or_else(|| execution("translated Metal function is missing"))
}
fn vertex_format(ty: c::ShaderValueType) -> naga_types::VertexFormat {
    use naga_types::VertexFormat::*;
    match ty {
        c::ShaderValueType::Float(1) => Float32,
        c::ShaderValueType::Float(2) => Float32x2,
        c::ShaderValueType::Float(3) => Float32x3,
        c::ShaderValueType::Float(4) => Float32x4,
        c::ShaderValueType::Sint(1) => Sint32,
        c::ShaderValueType::Sint(2) => Sint32x2,
        c::ShaderValueType::Sint(3) => Sint32x3,
        c::ShaderValueType::Sint(4) => Sint32x4,
        c::ShaderValueType::Uint(1) => Uint32,
        c::ShaderValueType::Uint(2) => Uint32x2,
        c::ShaderValueType::Uint(3) => Uint32x3,
        c::ShaderValueType::Uint(4) => Uint32x4,
        _ => unreachable!("validated vertex type"),
    }
}
pub(super) fn compile_byte_copy(device: &DeviceRef) -> Result<ComputePipelineState> {
    let source = "#include <metal_stdlib>\nusing namespace metal;\nkernel void copy_bytes(device const uchar* source [[buffer(0)]], device uchar* dest [[buffer(1)]], constant ulong2& range [[buffer(2)]], uint id [[thread_position_in_grid]]) { if (ulong(id) < range.y) dest[id] = source[range.x + ulong(id)]; }";
    let library = device
        .newLibraryWithSource_options_error(
            &NSString::from_str(source),
            Some(&CompileOptions::new()),
        )
        .map_err(|e| execution(format!("byte-copy helper: {e}")))?;
    let function = library
        .newFunctionWithName(&NSString::from_str("copy_bytes"))
        .ok_or_else(|| execution("byte-copy helper missing"))?;
    device
        .newComputePipelineStateWithFunction_error(&function)
        .map_err(|e| execution(format!("byte-copy pipeline: {e}")))
}

pub(super) fn compile_nv12(device: &DeviceRef) -> Result<RenderPipelineState> {
    let source = r#"#include <metal_stdlib>
using namespace metal;
struct Nv12Vertex {float4 position [[position]];float2 uv;};
vertex Nv12Vertex nv12_vertex(uint id [[vertex_id]]) {float2 uv=float2((id<<1)&2,id&2); return {float4(uv*float2(2.,-2.)+float2(-1.,1.),0.,1.),uv};}
fragment float4 nv12_fragment(Nv12Vertex in [[stage_in]],texture2d<float> y [[texture(0)]],texture2d<float> uv [[texture(1)]],constant float4* rows [[buffer(0)]]) {
    constexpr sampler s(coord::normalized,address::clamp_to_edge,filter::linear);
    float4 v=float4(y.sample(s,in.uv).r,uv.sample(s,in.uv).rg,1.);
    return float4(clamp(float3(dot(rows[0],v),dot(rows[1],v),dot(rows[2],v)),0.,1.),1.);
}"#;
    let library = device
        .newLibraryWithSource_options_error(
            &NSString::from_str(source),
            Some(&CompileOptions::new()),
        )
        .map_err(|e| execution(format!("NV12 converter: {e}")))?;
    let vertex = library
        .newFunctionWithName(&NSString::from_str("nv12_vertex"))
        .ok_or_else(|| execution("missing NV12 vertex"))?;
    let fragment = library
        .newFunctionWithName(&NSString::from_str("nv12_fragment"))
        .ok_or_else(|| execution("missing NV12 fragment"))?;
    let descriptor = RenderPipelineDescriptor::new();
    descriptor.setVertexFunction(Some(&vertex));
    descriptor.setFragmentFunction(Some(&fragment));
    unsafe { descriptor.colorAttachments().objectAtIndexedSubscript(0) }
        .setPixelFormat(MTLPixelFormat::RGBA16Float);
    device
        .newRenderPipelineStateWithDescriptor_error(&descriptor)
        .map_err(|e| execution(format!("NV12 conversion pipeline: {e}")))
}
