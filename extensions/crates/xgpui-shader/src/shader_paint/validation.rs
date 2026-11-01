use super::*;
use anyhow::{Context as _, Result, ensure};
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
        module.entry_points.len() == 1
            && module.entry_points.iter().all(|entry| matches!(
                (entry.name.as_str(), entry.stage),
                ("paint_fragment", ShaderStage::Fragment)
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
        for (_, expression) in function.expressions.iter() {
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
    naga::valid::Validator::new(
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
    // Shared portable reflection validates scalar/vector/matrix uniform layouts.
    // Backend translation and native compilation belong exclusively to the host.
    let portable = PortableProgram::fullscreen(
        source,
        "paint_fragment",
        interpretation(),
        ImageContracts::new(),
    )?;
    portable.uniform_layout("parameters")?;
    Ok(Program { portable })
}

fn user_location(line: u32, user: &str) -> String {
    let prefix = PRELUDE.lines().count() as u32 + 1;
    if line > prefix && line <= prefix + user.lines().count() as u32 {
        format!("user WGSL line {}: ", line - prefix)
    } else {
        String::new()
    }
}
