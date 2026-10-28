//! Explicit bounded SDR backdrop effect; children remain in the normal paint pass.
use anyhow::{Context, Result, ensure};
use gpui::*;
use std::sync::{Arc, OnceLock};

/// A separable Gaussian backdrop filter. Radius is in logical pixels (0..=32).
/// Retained source commands can reuse the captured and filtered pixels.
#[derive(Clone)]
pub struct BackdropBlur {
    radius: Pixels,
    pub(crate) program: Arc<SceneShaderProgram>,
}
impl BackdropBlur {
    /// Construct an opt-in filter. Zero radius is a no-op.
    pub fn new(radius: Pixels) -> Result<Self> {
        let value = f32::from(radius);
        ensure!(
            value.is_finite() && (0. ..=32.).contains(&value),
            "backdrop radius must be in [0,32] logical pixels"
        );
        let program = shared_program()?;
        Ok(Self { radius, program })
    }
    /// Record a filter before the surface's own background and children.
    pub fn paint(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        radii: Corners<Pixels>,
        opacity: f32,
    ) {
        window.paint_backdrop_blur(bounds, radii, self.radius, opacity, self.program.clone());
    }
}
pub(crate) fn shared_program() -> Result<Arc<SceneShaderProgram>> {
    static PROGRAM: OnceLock<Arc<SceneShaderProgram>> = OnceLock::new();
    if let Some(program) = PROGRAM.get() {
        return Ok(program.clone());
    }
    let program = Arc::new(compile()?);
    Ok(PROGRAM.get_or_init(|| program).clone())
}

fn compile() -> Result<SceneShaderProgram> {
    let source = include_str!("backdrop.wgsl");
    let module = naga::front::wgsl::parse_str(source)
        .map_err(|e| anyhow::anyhow!(e.emit_to_string(source)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)?;
    let mut msl = naga::back::msl::Options {
        lang_version: (2, 1),
        fake_missing_bindings: false,
        ..Default::default()
    };
    for entry in &module.entry_points {
        let mut resources = naga::back::msl::EntryPointResources::default();
        for binding in [0, 1, 3] {
            resources.resources.insert(
                naga::ResourceBinding { group: 0, binding },
                naga::back::msl::BindTarget {
                    buffer: (binding == 0).then_some(1),
                    texture: if binding == 1 {
                        Some(0)
                    } else if binding == 3 {
                        Some(1)
                    } else {
                        None
                    },
                    sampler: None,
                    ..Default::default()
                },
            );
        }
        msl.per_entry_point_map
            .insert(entry.name.clone(), resources);
    }
    let (metal, reflection) =
        naga::back::msl::write_string(&module, &info, &msl, &Default::default())?;
    let names: Vec<_> = reflection
        .entry_point_names
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let mut options = naga::back::hlsl::Options {
        shader_model: naga::back::hlsl::ShaderModel::V5_0,
        fake_missing_bindings: false,
        ..Default::default()
    };
    for binding in [0, 1, 3] {
        options.binding_map.insert(
            naga::ResourceBinding { group: 0, binding },
            naga::back::hlsl::BindTarget {
                register: if binding == 3 { 1 } else { 0 },
                ..Default::default()
            },
        );
    }
    let mut hlsl = String::new();
    let reflection = naga::back::hlsl::Writer::new(&mut hlsl, &options, &Default::default())
        .write(&module, &info, None)
        .context("backdrop HLSL")?;
    let hnames: Vec<_> = reflection
        .entry_point_names
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("{e:?}"))?;
    for entry in &module.entry_points {
        naga::back::spv::write_vec(
            &module,
            &info,
            &Default::default(),
            Some(&naga::back::spv::PipelineOptions {
                shader_stage: entry.stage,
                entry_point: entry.name.clone(),
            }),
        )
        .context("backdrop SPIR-V translation")?;
    }
    Ok(SceneShaderProgram::new(
        source.into(),
        (metal, names[0].clone(), names[1].clone()),
        (hlsl, hnames[0].clone(), hnames[1].clone()),
    ))
}
