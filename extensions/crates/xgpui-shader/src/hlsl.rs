//! WGSL to shader model 5.0; native D3D11 owns the final driver compilation.
use anyhow::Result;
pub(crate) fn compile_wgsl(source: &str) -> Result<(String, String, String)> {
    use naga::{
        back::hlsl,
        valid::{Capabilities, ValidationFlags, Validator},
    };
    let module = naga::front::wgsl::parse_str(source)?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty()).validate(&module)?;
    let mut options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_0,
        fake_missing_bindings: false,
        ..Default::default()
    };
    for (_, global) in module.global_variables.iter() {
        if let Some(binding) = global.binding {
            options.binding_map.insert(
                binding,
                hlsl::BindTarget {
                    register: binding.binding,
                    ..Default::default()
                },
            );
        }
    }
    let mut source = String::new();
    let reflection = hlsl::Writer::new(
        &mut source,
        &options,
        &hlsl::PipelineOptions { entry_point: None },
    )
    .write(&module, &info, None)?;
    let entry = |stage| -> Result<String> {
        let index = module
            .entry_points
            .iter()
            .position(|e| e.stage == stage)
            .ok_or_else(|| anyhow::anyhow!("missing shader stage"))?;
        reflection.entry_point_names[index]
            .clone()
            .map_err(|e| anyhow::anyhow!("HLSL entry point: {e:?}"))
    };
    Ok((
        source,
        entry(naga::ShaderStage::Vertex)?,
        entry(naga::ShaderStage::Fragment)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_hlsl_exports_both_stages_with_explicit_constant_bindings() -> Result<()> {
        for source in [
            include_str!("wgpu/mesh.wgsl"),
            include_str!("wgpu/gradient.wgsl"),
        ] {
            let (hlsl, vertex, fragment) = compile_wgsl(source)?;
            assert!(hlsl.contains(&format!("{vertex}(")));
            assert!(hlsl.contains(&format!("{fragment}(")));
            assert!(hlsl.contains("register(b0)"));
            assert!(hlsl.contains("SV_VertexID"));
            assert!(!hlsl.contains("register(t0)"));
        }
        Ok(())
    }
}
