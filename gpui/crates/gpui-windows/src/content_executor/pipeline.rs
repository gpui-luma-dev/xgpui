use super::*;
use translation::Slot;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct PipelineKey {
    program: c::PortableProgram,
    streams: Vec<c::VertexLayout>,
    blend: c::PassBlend,
}
impl PipelineKey {
    pub fn of(node: &c::ContentNode) -> Option<Self> {
        match node.operation() {
            c::ContentOperation::Render(p) => Some(Self {
                program: p.program().clone(),
                streams: match p.geometry() {
                    c::ResolvedGeometry::Mesh { vertices, .. } => {
                        vertices.iter().map(|v| v.layout.clone()).collect()
                    }
                    _ => vec![],
                },
                blend: p.blend(),
            }),
            c::ContentOperation::Compute(p) => Some(Self {
                program: p.program().clone(),
                streams: vec![],
                blend: c::PassBlend::Replace,
            }),
            _ => None,
        }
    }
}
pub(super) struct Pipeline {
    pub slots: std::collections::BTreeMap<String, Slot>,
    pub vertex: Option<ID3D11VertexShader>,
    pub fragment: Option<ID3D11PixelShader>,
    pub compute: Option<ID3D11ComputeShader>,
    pub input: Option<ID3D11InputLayout>,
    pub blend: Option<ID3D11BlendState>,
    pub raster: Option<ID3D11RasterizerState>,
}
pub(super) fn format(f: c::PixelFormat) -> DXGI_FORMAT {
    match f {
        c::PixelFormat::Rgba8 => DXGI_FORMAT_R8G8B8A8_UNORM,
        c::PixelFormat::Bgra8 => DXGI_FORMAT_B8G8R8A8_UNORM,
        c::PixelFormat::Rgba16Float => DXGI_FORMAT_R16G16B16A16_FLOAT,
        c::PixelFormat::R8 => DXGI_FORMAT_R8_UNORM,
    }
}
fn vertex_format(t: c::ShaderValueType) -> DXGI_FORMAT {
    use c::ShaderValueType::*;
    match t {
        Float(1) => DXGI_FORMAT_R32_FLOAT,
        Float(2) => DXGI_FORMAT_R32G32_FLOAT,
        Float(3) => DXGI_FORMAT_R32G32B32_FLOAT,
        Float(4) => DXGI_FORMAT_R32G32B32A32_FLOAT,
        Sint(1) => DXGI_FORMAT_R32_SINT,
        Sint(2) => DXGI_FORMAT_R32G32_SINT,
        Sint(3) => DXGI_FORMAT_R32G32B32_SINT,
        Sint(4) => DXGI_FORMAT_R32G32B32A32_SINT,
        Uint(1) => DXGI_FORMAT_R32_UINT,
        Uint(2) => DXGI_FORMAT_R32G32_UINT,
        Uint(3) => DXGI_FORMAT_R32G32B32_UINT,
        Uint(4) => DXGI_FORMAT_R32G32B32A32_UINT,
        _ => unreachable!(),
    }
}
fn code(
    p: &c::PortableProgram,
    stage: naga::ShaderStage,
    entry: &str,
) -> Result<(Vec<u8>, translation::Translation)> {
    let t = translation::translate(p, stage, entry)?;
    let profile = match stage {
        naga::ShaderStage::Vertex => "vs_5_0",
        naga::ShaderStage::Fragment => "ps_5_0",
        _ => "cs_5_0",
    };
    let bytes = crate::custom_draw::compile(&t.source, &t.entry, profile)
        .map_err(|e| error(c::ContentErrorKind::ShaderValidation, e.to_string()))?;
    Ok((bytes, t))
}
impl Worker {
    pub fn pipeline(
        &mut self,
        key: PipelineKey,
        stats: &mut Dx11ContentStats,
    ) -> Result<Arc<Pipeline>> {
        if let Some(p) = self.pipelines.get(&key) {
            let pipeline = p.clone();
            self.pipeline_order.retain(|cached| cached != &key);
            self.pipeline_order.push_back(key);
            return Ok(pipeline);
        }
        for binding in key.program.bindings() {
            if matches!(
                binding.kind(),
                c::ProgramBindingKind::StorageImage {
                    access: c::StorageAccess::ReadOnly | c::StorageAccess::ReadWrite,
                    ..
                }
            ) {
                return Err(error(
                    c::ContentErrorKind::UnsupportedCapability,
                    "D3D11 portable typed storage textures support write-only access; sampled reads use separate immutable inputs",
                ));
            }
        }
        let mut p = Pipeline {
            slots: Default::default(),
            vertex: None,
            fragment: None,
            compute: None,
            input: None,
            blend: None,
            raster: None,
        };
        unsafe {
            if let Some(entry) = key.program.compute_entry() {
                let (bytes, t) = code(&key.program, naga::ShaderStage::Compute, entry)?;
                self.device
                    .CreateComputeShader(&bytes, None, Some(&mut p.compute))
                    .map_err(allocation)?;
                p.slots = t.slots;
            } else {
                let (bytes, t) = code(
                    &key.program,
                    naga::ShaderStage::Vertex,
                    key.program.render_entries().unwrap().0,
                )?;
                self.device
                    .CreateVertexShader(&bytes, None, Some(&mut p.vertex))
                    .map_err(allocation)?;
                p.slots = t.slots;
                let mut attributes = vec![];
                for (slot, layout) in key.streams.iter().enumerate() {
                    for a in &layout.attributes {
                        attributes.push(D3D11_INPUT_ELEMENT_DESC {
                            SemanticName: windows::core::s!("LOC"),
                            SemanticIndex: a.location,
                            Format: vertex_format(a.ty),
                            InputSlot: slot as u32,
                            AlignedByteOffset: a.offset,
                            InputSlotClass: if layout.step == c::VertexStep::Instance {
                                D3D11_INPUT_PER_INSTANCE_DATA
                            } else {
                                D3D11_INPUT_PER_VERTEX_DATA
                            },
                            InstanceDataStepRate: u32::from(layout.step == c::VertexStep::Instance),
                        });
                    }
                }
                if !attributes.is_empty() {
                    self.device
                        .CreateInputLayout(&attributes, &bytes, Some(&mut p.input))
                        .map_err(|e| error(c::ContentErrorKind::ShaderValidation, e.to_string()))?;
                }
                let (bytes, t) = code(
                    &key.program,
                    naga::ShaderStage::Fragment,
                    key.program.render_entries().unwrap().1,
                )?;
                self.device
                    .CreatePixelShader(&bytes, None, Some(&mut p.fragment))
                    .map_err(allocation)?;
                p.slots.extend(t.slots);
                let mut desc = D3D11_BLEND_DESC::default();
                let r = &mut desc.RenderTarget[0];
                r.RenderTargetWriteMask = D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8;
                r.SrcBlend = D3D11_BLEND_ONE;
                r.DestBlend = D3D11_BLEND_ZERO;
                r.BlendOp = D3D11_BLEND_OP_ADD;
                r.SrcBlendAlpha = D3D11_BLEND_ONE;
                r.DestBlendAlpha = D3D11_BLEND_ZERO;
                r.BlendOpAlpha = D3D11_BLEND_OP_ADD;
                if key.blend == c::PassBlend::SourceOver {
                    r.BlendEnable = true.into();
                    r.SrcBlend = D3D11_BLEND_ONE;
                    r.DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
                    r.BlendOp = D3D11_BLEND_OP_ADD;
                    r.SrcBlendAlpha = D3D11_BLEND_ONE;
                    r.DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
                    r.BlendOpAlpha = D3D11_BLEND_OP_ADD;
                }
                self.device
                    .CreateBlendState(&desc, Some(&mut p.blend))
                    .map_err(allocation)?;
                self.device
                    .CreateRasterizerState(
                        &D3D11_RASTERIZER_DESC {
                            FillMode: D3D11_FILL_SOLID,
                            CullMode: D3D11_CULL_NONE,
                            DepthClipEnable: true.into(),
                            ..Default::default()
                        },
                        Some(&mut p.raster),
                    )
                    .map_err(allocation)?;
            }
        }
        let p = Arc::new(p);
        if self.pipelines.len() >= self.options.pipeline_cache_entries {
            if let Some(oldest) = self.pipeline_order.pop_front() {
                self.pipelines.remove(&oldest);
            }
        }
        self.pipeline_order.push_back(key.clone());
        self.pipelines.insert(key, p.clone());
        stats.pipeline_compilations += 1;
        Ok(p)
    }
}
