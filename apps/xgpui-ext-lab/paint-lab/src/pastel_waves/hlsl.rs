//! producer.wgsl translated to HLSL shader model 5.0 for the D3D11 producer.
//! Compiled on every desktop target so the translation is tested everywhere;
//! only Windows compiles the result with D3DCompile.
use anyhow::{Context as _, Result};

/// HLSL source and the generated entry names of each producer stage.
pub(super) struct ProducerHlsl {
    pub source: String,
    pub compute: String,
    pub vertex: String,
    pub fragment: String,
}

/// Bindings keep their WGSL numbers as registers: b0 (uniforms), u1 (columns,
/// compute) and t2 (columns, fragment). Shader model 5.0 has no register spaces.
pub(super) fn producer_hlsl() -> Result<ProducerHlsl> {
    use naga::{
        back::hlsl,
        valid::{Capabilities, ValidationFlags, Validator},
    };
    let source = super::producer_source();
    let module = naga::front::wgsl::parse_str(&source)
        .map_err(|error| anyhow::anyhow!("{}", error.emit_to_string(&source)))?;
    let info = Validator::new(ValidationFlags::all(), Capabilities::empty())
        .validate(&module)
        .context("pastel waves producer validation")?;
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
    let mut hlsl = String::new();
    let reflection = hlsl::Writer::new(
        &mut hlsl,
        &options,
        &hlsl::PipelineOptions { entry_point: None },
    )
    .write(&module, &info, None)
    .context("pastel waves HLSL translation")?;
    let entry = |name: &str| -> Result<String> {
        let index = module
            .entry_points
            .iter()
            .position(|entry| entry.name == name)
            .with_context(|| format!("missing entry point {name}"))?;
        reflection.entry_point_names[index]
            .clone()
            .map_err(|error| anyhow::anyhow!("HLSL entry point {name}: {error:?}"))
    };
    Ok(ProducerHlsl {
        compute: entry("pastel_columns")?,
        vertex: entry("pastel_vertex")?,
        fragment: entry("pastel_fragment")?,
        source: hlsl,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn producer_translates_to_shader_model_5_with_wgsl_registers() -> Result<()> {
        let hlsl = producer_hlsl()?;
        for entry in [&hlsl.compute, &hlsl.vertex, &hlsl.fragment] {
            assert!(hlsl.source.contains(&format!("{entry}(")), "{entry}");
        }
        assert!(hlsl.source.contains("register(b0)"));
        assert!(
            hlsl.source
                .contains("RWByteAddressBuffer columns_out : register(u1)")
        );
        assert!(
            hlsl.source
                .contains("ByteAddressBuffer columns : register(t2)")
        );
        assert!(hlsl.source.contains("[numthreads(64, 1, 1)]"));
        assert!(!hlsl.source.contains("space"));
        Ok(())
    }
}
