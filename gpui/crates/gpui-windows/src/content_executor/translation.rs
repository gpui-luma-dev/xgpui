//! CPU-only WGSL to shader-model-5 HLSL translation, also tested on non-Windows hosts.
use gpui::content as c;
use naga::back::hlsl;
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, c::ContentError>;
fn fail(message: impl Into<String>) -> c::ContentError {
    c::ContentError::new(c::ContentErrorKind::ShaderValidation, message)
}
#[derive(Clone, Copy, Debug)]
pub enum Slot {
    Uniform(u32),
    Read(u32),
    Write(u32),
    Sampler(u32),
}
pub struct Translation {
    pub source: String,
    pub entry: String,
    pub slots: BTreeMap<String, Slot>,
}
pub fn translate(
    program: &c::PortableProgram,
    stage: naga::ShaderStage,
    entry: &str,
) -> Result<Translation> {
    let module = naga::front::wgsl::parse_str(program.wgsl())
        .map_err(|e| fail(e.emit_to_string(program.wgsl())))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| fail(e.to_string()))?;
    let mut options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_0,
        fake_missing_bindings: false,
        restrict_indexing: true,
        zero_initialize_workgroup_memory: true,
        ..Default::default()
    };
    if stage != naga::ShaderStage::Fragment {
        options.special_constants_binding = Some(hlsl::BindTarget {
            register: 13,
            ..Default::default()
        });
    }
    let (mut uniforms, mut reads, mut writes, mut samplers) = (0, 0, 0, 0);
    let mut slots = BTreeMap::new();
    for (_, global) in module.global_variables.iter() {
        let Some(binding) = global.binding else {
            continue;
        };
        let (slot, register) = match global.space {
            naga::AddressSpace::Uniform => {
                let i = uniforms;
                uniforms += 1;
                (Slot::Uniform(i), i)
            }
            naga::AddressSpace::Storage { access }
                if access.contains(naga::StorageAccess::STORE) =>
            {
                let i = writes;
                writes += 1;
                (Slot::Write(i), i)
            }
            naga::AddressSpace::Storage { .. } => {
                let i = reads;
                reads += 1;
                (Slot::Read(i), i)
            }
            _ => match module.types[global.ty].inner {
                naga::TypeInner::Sampler { .. } => {
                    let i = samplers;
                    samplers += 1;
                    (Slot::Sampler(i), i)
                }
                naga::TypeInner::Image {
                    class: naga::ImageClass::Storage { access, .. },
                    ..
                } if access.contains(naga::StorageAccess::STORE) => {
                    let i = writes;
                    writes += 1;
                    (Slot::Write(i), i)
                }
                _ => {
                    let i = reads;
                    reads += 1;
                    (Slot::Read(i), i)
                }
            },
        };
        if matches!(slot, Slot::Sampler(_)) {
            options.sampler_buffer_binding_map.insert(
                hlsl::SamplerIndexBufferKey {
                    group: binding.group,
                },
                hlsl::BindTarget {
                    register: 127,
                    ..Default::default()
                },
            );
        }
        options.binding_map.insert(
            binding,
            hlsl::BindTarget {
                register,
                space: 0,
                restrict_indexing: true,
                ..Default::default()
            },
        );
        if let Some(name) = &global.name {
            slots.insert(name.clone(), slot);
        }
    }
    if uniforms
        > if stage == naga::ShaderStage::Fragment {
            14
        } else {
            13
        }
        || reads > 128
        || writes > 8
        || samplers > 16
    {
        return Err(c::ContentError::new(
            c::ContentErrorKind::UnsupportedCapability,
            "program exceeds D3D11 shader-model-5 resource slots",
        ));
    }
    let pipeline = hlsl::PipelineOptions {
        entry_point: Some((stage, entry.into())),
    };
    let mut source = String::new();
    let reflection = hlsl::Writer::new(&mut source, &options, &pipeline)
        .write(&module, &info, None)
        .map_err(|e| fail(e.to_string()))?;
    let entry = reflection
        .entry_point_names
        .first()
        .ok_or_else(|| fail("missing generated HLSL entry"))?
        .as_ref()
        .map_err(|e| fail(format!("{e:?}")))?
        .clone();
    let source = direct_samplers(&source)?.replace(
        "ConstantBuffer<NagaConstants> _NagaConstants: register(b13);",
        "cbuffer NagaDrawOrDispatch : register(b13) { NagaConstants _NagaConstants; };",
    );
    Ok(Translation {
        source,
        entry,
        slots,
    })
}

// Naga 30 emits DX12 sampler heaps even for SM5. Portable programs permit
// individual ordinary samplers, so lower those generated static aliases to
// direct D3D11 sampler registers. The generated index is BindTarget.register
// from options.binding_map, not a stage-local index. Keep Naga's emitted alias
// name: it may differ from the WGSL name after identifier sanitization.
// No user WGSL or application API is rewritten.
fn direct_samplers(source: &str) -> Result<String> {
    let mut buffers = std::collections::HashSet::new();
    let mut lines = vec![];
    for line in source.lines() {
        let t = line.trim();
        if let Some(alias) = t.strip_prefix("static const SamplerState ") {
            let (name, rhs) = alias
                .split_once(" = ")
                .ok_or_else(|| fail("invalid generated sampler alias"))?;
            let (_, indices) = rhs
                .split_once('[')
                .ok_or_else(|| fail("invalid generated sampler heap"))?;
            let (buffer, index) = indices
                .split_once('[')
                .ok_or_else(|| fail("invalid generated sampler indices"))?;
            let (register, _) = index
                .split_once(']')
                .ok_or_else(|| fail("invalid generated sampler register"))?;
            let register: u32 = register
                .parse()
                .map_err(|_| fail("sampler binding arrays are not supported by D3D11"))?;
            buffers.insert(buffer.to_string());
            lines.push(format!("SamplerState {name} : register(s{register});"));
        } else if !t.starts_with("SamplerState nagaSamplerHeap[")
            && !t.starts_with("SamplerComparisonState nagaComparisonSamplerHeap[")
        {
            lines.push(line.to_string());
        }
    }
    lines.retain(|line| {
        !buffers.iter().any(|name| {
            line.trim()
                .starts_with(&format!("StructuredBuffer<uint> {name} :"))
        })
    });
    let result = lines.join("\n");
    if result.contains("nagaSamplerHeap[") || result.contains("nagaComparisonSamplerHeap[") {
        return Err(fail("unlowered D3D11 sampler heap"));
    }
    Ok(result)
}
