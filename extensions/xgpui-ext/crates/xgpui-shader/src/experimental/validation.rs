use super::*;
use naga::{ScalarKind, ShaderStage, TypeInner};

const PRELUDE: &str = include_str!("prelude.wgsl");
const ENTRYPOINTS: &str = include_str!("entrypoints.wgsl");

pub(super) fn compile(user: &str) -> Result<Program> {
    ensure!(
        user.len() <= 64 * 1024,
        "portable shader source exceeds 64 KiB"
    );
    let source = format!("{PRELUDE}\n{user}\n{ENTRYPOINTS}");
    let module = naga::front::wgsl::parse_str(&source).map_err(|error| {
        let location = error
            .location(&source)
            .map(|location| user_location(location.line_number, user))
            .unwrap_or_default();
        anyhow::anyhow!(
            "{location}{}\n{}",
            error.message(),
            error.emit_to_string(&source)
        )
    })?;
    ensure!(
        module.entry_points.len() == 2
            && module.entry_points.iter().all(|entry| matches!(
                (entry.name.as_str(), entry.stage),
                ("paint_vertex", ShaderStage::Vertex) | ("paint_fragment", ShaderStage::Fragment)
            )),
        "user entry points are not supported"
    );
    ensure!(
        module.overrides.is_empty(),
        "shader overrides are not supported"
    );
    ensure!(
        module.global_variables.len() == 2
            && module
                .global_variables
                .iter()
                .all(|(_, v)| matches!(v.name.as_deref(), Some("parameters" | "xgpui_geometry"))),
        "user resources and global variables are not supported"
    );
    for (_, function) in module.functions.iter() {
        check_block(&function.body)?;
        for (_, expression) in function.expressions.iter() {
            ensure!(
                !matches!(expression, naga::Expression::Derivative { .. }),
                "derivatives are not supported in portable paint v1"
            );
            if let naga::Expression::GlobalVariable(handle) = expression {
                ensure!(
                    module.global_variables[*handle].name.as_deref() == Some("parameters"),
                    "extension-owned geometry is not a user binding; use PaintInput"
                );
            }
        }
    }
    let function = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some("paint"))
        .map(|(_, f)| f)
        .context("expected fn paint(input: PaintInput) -> vec4<f32>")?;
    ensure!(
        function.arguments.len() == 1
            && module.types[function.arguments[0].ty].name.as_deref() == Some("PaintInput"),
        "paint must take exactly one PaintInput"
    );
    ensure!(
        function.result.as_ref().is_some_and(|r| matches!(
            module.types[r.ty].inner,
            TypeInner::Vector {
                size: naga::VectorSize::Quad,
                scalar: naga::Scalar {
                    kind: ScalarKind::Float,
                    width: 4
                }
            }
        )),
        "paint must return vec4<f32>"
    );
    let parameters = module
        .global_variables
        .iter()
        .find(|(_, v)| v.name.as_deref() == Some("parameters"))
        .unwrap()
        .1;
    let TypeInner::Struct { members, span } = &module.types[parameters.ty].inner else {
        bail!("Parameters must be a struct");
    };
    ensure!(
        !members.is_empty() && members.len() <= 32 && *span <= 1024,
        "Parameters must have 1..=32 fields and fit in 1024 bytes"
    );
    let mut fields = Vec::new();
    for member in members {
        let components = match module.types[member.ty].inner {
            TypeInner::Scalar(naga::Scalar {
                kind: ScalarKind::Float,
                width: 4,
            }) => 1,
            TypeInner::Vector {
                size,
                scalar:
                    naga::Scalar {
                        kind: ScalarKind::Float,
                        width: 4,
                    },
            } => size as usize,
            _ => bail!(
                "parameter `{}` must be f32 or vec2/3/4<f32>",
                member.name.as_deref().unwrap_or("?")
            ),
        };
        fields.push(Field {
            name: member.name.clone().context("unnamed parameter")?,
            offset: member.offset as usize,
            components,
        });
    }
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| {
        let location = error
            .spans()
            .map(|(span, _)| user_location(span.location(&source).line_number, user))
            .find(|line| !line.is_empty())
            .unwrap_or_default();
        anyhow::anyhow!(
            "{location}portable WGSL validation: {}",
            error.emit_to_string(&source)
        )
    })?;
    let mut msl = naga::back::msl::Options {
        lang_version: (2, 1),
        fake_missing_bindings: false,
        ..Default::default()
    };
    for entry in &module.entry_points {
        let mut resources = naga::back::msl::EntryPointResources::default();
        for binding in 0..2 {
            resources.resources.insert(
                naga::ResourceBinding { group: 0, binding },
                naga::back::msl::BindTarget {
                    buffer: Some(binding as u8),
                    ..Default::default()
                },
            );
        }
        msl.per_entry_point_map
            .insert(entry.name.clone(), resources);
    }
    let (metal_source, translation) =
        naga::back::msl::write_string(&module, &info, &msl, &Default::default())
            .context("portable MSL translation")?;
    let metal_entries: Vec<_> = translation
        .entry_point_names
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("portable MSL entry point: {e:?}"))?;
    #[cfg(not(target_os = "macos"))]
    let _ = (&metal_source, &metal_entries);
    // Validate translation targets up front; platform driver execution is still
    // required before claiming native DirectX/Vulkan support.
    for entry in &module.entry_points {
        let options = naga::back::spv::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
        };
        naga::back::spv::write_vec(&module, &info, &Default::default(), Some(&options))
            .context("portable SPIR-V translation")?;
        let mut options = naga::back::hlsl::Options {
            shader_model: naga::back::hlsl::ShaderModel::V5_0,
            fake_missing_bindings: false,
            ..Default::default()
        };
        for binding in 0..2 {
            options.binding_map.insert(
                naga::ResourceBinding { group: 0, binding },
                naga::back::hlsl::BindTarget {
                    register: binding,
                    ..Default::default()
                },
            );
        }
        let pipeline = naga::back::hlsl::PipelineOptions {
            entry_point: Some((entry.stage, entry.name.clone())),
        };
        let mut text = String::new();
        let reflection = naga::back::hlsl::Writer::new(&mut text, &options, &pipeline)
            .write(&module, &info, None)
            .context("portable HLSL translation")?;
        ensure!(
            reflection.entry_point_names.iter().all(|name| name.is_ok()),
            "portable HLSL entry point translation failed"
        );
    }
    Ok(Program {
        #[cfg(feature = "wgpu")]
        source,
        fields,
        uniform_size: (*span as usize).next_multiple_of(16),
        #[cfg(target_os = "macos")]
        msl: metal_source,
        #[cfg(target_os = "macos")]
        metal_entries: [metal_entries[0].clone(), metal_entries[1].clone()],
        #[cfg(target_os = "macos")]
        metal_pipeline: Default::default(),
        #[cfg(feature = "wgpu")]
        wgpu: Default::default(),
    })
}

fn check_block(block: &naga::Block) -> Result<()> {
    for statement in block.iter() {
        match statement {
            naga::Statement::Kill => bail!("discard is not supported in portable paint v1"),
            naga::Statement::Loop { .. } => bail!("loops are not supported in portable paint v1"),
            naga::Statement::Block(block) => check_block(block)?,
            naga::Statement::If { accept, reject, .. } => {
                check_block(accept)?;
                check_block(reject)?;
            }
            naga::Statement::Switch { cases, .. } => {
                for case in cases {
                    check_block(&case.body)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn user_location(line: u32, user: &str) -> String {
    let prefix = PRELUDE.lines().count() as u32 + 1;
    if line > prefix && line <= prefix + user.lines().count() as u32 {
        format!("user WGSL line {}: ", line - prefix)
    } else {
        String::new()
    }
}
