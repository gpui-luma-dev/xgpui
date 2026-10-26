use super::*;
use anyhow::{Context, bail};
use naga::{ScalarKind, TypeInner};
const PRELUDE: &str = "struct ShaderInput { uv: vec2<f32>, position: vec2<f32>, size: vec2<f32>, scale_factor: f32, }";

pub(super) fn compile<T: ShaderInstance>(
    user: &str,
    encoding: ShaderOutputEncoding,
) -> Result<SceneShaderProgram> {
    compile_impl::<T>(user, encoding, false, false)
}
pub(super) fn compile_glyph<T: ShaderInstance>(user: &str) -> Result<SceneShaderProgram> {
    compile_impl::<T>(user, ShaderOutputEncoding::EncodedSrgb, true, false)
}
pub(super) fn compile_path<T: ShaderInstance>(user: &str) -> Result<SceneShaderProgram> {
    compile_impl::<T>(user, ShaderOutputEncoding::EncodedSrgb, false, true)
}
fn compile_impl<T: ShaderInstance>(
    user: &str,
    encoding: ShaderOutputEncoding,
    glyph: bool,
    path: bool,
) -> Result<SceneShaderProgram> {
    ensure!(user.len() <= 64 * 1024, "shader source exceeds 64 KiB");
    let user_source = format!("{PRELUDE}\n{user}");
    let parsed = naga::front::wgsl::parse_str(&user_source).map_err(|e| {
        let line = e
            .location(&user_source)
            .map(|v| v.line_number.saturating_sub(1));
        anyhow::anyhow!(
            "shader source line {}: {}\n{}",
            line.unwrap_or(0),
            e,
            e.emit_to_string(&user_source)
        )
    })?;
    ensure!(
        parsed.entry_points.is_empty()
            && parsed.global_variables.is_empty()
            && parsed.overrides.is_empty(),
        "declare only types, constants and helper functions; host owns entry points and resources"
    );
    let (_, ty) = parsed
        .types
        .iter()
        .find(|(_, t)| t.name.as_deref() == Some("Parameters"))
        .context("expected struct Parameters")?;
    let TypeInner::Struct { members, span } = &ty.inner else {
        bail!("Parameters must be a struct")
    };
    ensure!(
        !members.is_empty() && *span <= 128,
        "Parameters must contain fields and fit in 128 bytes"
    );
    ensure!(
        *span as usize == std::mem::size_of::<T>(),
        "Rust instance size {} differs from WGSL stride {}",
        std::mem::size_of::<T>(),
        span
    );
    ensure!(
        members.len() == T::FIELDS.len(),
        "Rust/WGSL member count differs"
    );
    let mut args = Vec::new();
    for (member, field) in members.iter().zip(T::FIELDS) {
        let components = match parsed.types[member.ty].inner {
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
            _ => bail!("Parameters supports only f32 and vec2/3/4<f32>"),
        };
        ensure!(
            member.name.as_deref() == Some(field.name)
                && member.offset as usize == field.offset
                && components == field.components,
            "Rust/WGSL layout mismatch for {}: expected offset {}, {} components",
            field.name,
            member.offset,
            components
        );
        let values: Vec<_> = (0..components)
            .map(|i| {
                let word = member.offset as usize / 4 + i;
                format!("input.data{}[{}]", word / 4, word % 4)
            })
            .collect();
        args.push(if components == 1 {
            values[0].clone()
        } else {
            format!("vec{components}<f32>({})", values.join(","))
        });
    }
    let attributes = (0..8)
        .map(|i| format!("@location({}) data{i}: vec4<f32>,", i + 3))
        .collect::<String>();
    let varying = (0..8)
        .map(|i| {
            format!(
                "@location({}) @interpolate(flat) data{i}: vec4<f32>,",
                i + 4
            )
        })
        .collect::<String>();
    let copy = (0..8)
        .map(|i| format!("out.data{i} = instance.data{i};"))
        .collect::<String>();
    let output = if encoding == ShaderOutputEncoding::LinearDisplayP3 {
        // pad is the host composition-space selector: 0=P3, 1=scRGB.
        r#"var rgb = color.rgb;
        if host.pad != 0u {
            rgb = vec3(1.2249401763 * rgb.r - 0.2249401763 * rgb.g,
                -0.0420569547 * rgb.r + 1.0420569547 * rgb.g,
                -0.0196375546 * rgb.r - 0.0786360456 * rgb.g + 1.0982736002 * rgb.b);
        }
        return vec4(rgb * alpha, alpha);"#
    } else {
        "return vec4(select(color.rgb, color.rgb * alpha, host.premultiplied != 0u), alpha);"
    };
    let glyph_resource = if glyph || path {
        "@group(1) @binding(0) var glyph_atlas: texture_2d<f32>;"
    } else {
        ""
    };
    let glyph_sample = if path {
        "let path_sample = textureLoad(glyph_atlas,vec2<i32>(input.screen.xy),0).a;"
    } else if glyph {
        "let glyph_sample = textureLoad(glyph_atlas, vec2<i32>(glyph_texel(ShaderInput(input.local, input.local * input.extent, input.extent, input.misc.y), parameters)), 0);"
    } else {
        ""
    };
    let glyph_coverage = if path {
        "path_sample"
    } else if glyph {
        "select(glyph_sample.r, glyph_sample.a, host.pad == 2u)"
    } else {
        "1."
    };
    let source = format!(
        r#"
{PRELUDE}
{user}
struct HostGlobals {{ viewport: vec2<f32>, premultiplied: u32, pad: u32, }}
@group(0) @binding(0) var<uniform> host: HostGlobals;
{glyph_resource}
struct HostInstance {{ @location(0) bounds: vec4<f32>, @location(1) clip: vec4<f32>, @location(2) misc: vec4<f32>, {attributes} }}
struct HostVarying {{
    @builtin(position) screen: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) extent: vec2<f32>,
    @location(2) @interpolate(flat) clip: vec4<f32>,
    @location(3) @interpolate(flat) misc: vec2<f32>,
    {varying}
}}
@vertex fn shader_vertex(@builtin(vertex_index) vertex: u32, instance: HostInstance) -> HostVarying {{
    let vertices = array<vec2<f32>, 6>(vec2(0.,0.), vec2(1.,0.), vec2(0.,1.), vec2(0.,1.), vec2(1.,0.), vec2(1.,1.));
    let uv = vertices[vertex];
    let screen = instance.bounds.xy + uv * instance.bounds.zw;
    var out: HostVarying;
    out.screen = vec4(screen / host.viewport * vec2(2.,-2.) + vec2(-1.,1.), 0., 1.);
    out.local = uv; out.extent = instance.bounds.zw / instance.misc.y;
    out.clip = instance.clip; out.misc = instance.misc.xy;
    {copy}
    return out;
}}
@fragment fn shader_fragment(input: HostVarying) -> @location(0) vec4<f32> {{
    if (any(input.screen.xy < input.clip.xy) || any(input.screen.xy >= input.clip.zw)) {{ discard; }}
    let parameters = Parameters({parameters});
    let color = clamp(shade(ShaderInput(input.local, input.local * input.extent, input.extent, input.misc.y), parameters), vec4(0.), vec4(1.));
    {glyph_sample}
    let alpha = color.a * input.misc.x * {glyph_coverage};
    {output}
}}
"#,
        parameters = args.join(",")
    );
    let module = naga::front::wgsl::parse_str(&source)
        .map_err(|e| anyhow::anyhow!("{}", e.emit_to_string(&source)))?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .context("instanced shader validation")?;
    let mut msl = naga::back::msl::Options {
        lang_version: (2, 1),
        fake_missing_bindings: false,
        ..Default::default()
    };
    for entry in &module.entry_points {
        let mut resources = naga::back::msl::EntryPointResources::default();
        resources.resources.insert(
            naga::ResourceBinding {
                group: 0,
                binding: 0,
            },
            naga::back::msl::BindTarget {
                buffer: Some(1),
                ..Default::default()
            },
        );
        if glyph || path {
            resources.resources.insert(
                naga::ResourceBinding {
                    group: 1,
                    binding: 0,
                },
                naga::back::msl::BindTarget {
                    texture: Some(0),
                    ..Default::default()
                },
            );
        }
        msl.per_entry_point_map
            .insert(entry.name.clone(), resources);
    }
    let (metal, reflection) =
        naga::back::msl::write_string(&module, &info, &msl, &Default::default())
            .context("MSL translation")?;
    let names: Vec<_> = reflection
        .entry_point_names
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("MSL entry point: {e:?}"))?;
    let mut options = naga::back::hlsl::Options {
        shader_model: naga::back::hlsl::ShaderModel::V5_0,
        fake_missing_bindings: false,
        ..Default::default()
    };
    options.binding_map.insert(
        naga::ResourceBinding {
            group: 0,
            binding: 0,
        },
        naga::back::hlsl::BindTarget {
            register: 0,
            ..Default::default()
        },
    );
    if glyph || path {
        options.binding_map.insert(
            naga::ResourceBinding {
                group: 1,
                binding: 0,
            },
            naga::back::hlsl::BindTarget {
                register: 0,
                ..Default::default()
            },
        );
    }
    let mut hlsl = String::new();
    let reflection = naga::back::hlsl::Writer::new(&mut hlsl, &options, &Default::default())
        .write(&module, &info, None)
        .context("HLSL translation")?;
    let hlsl_names: Vec<_> = reflection
        .entry_point_names
        .into_iter()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| anyhow::anyhow!("HLSL entry point: {e:?}"))?;
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
        .context("SPIR-V translation")?;
    }
    let program = SceneShaderProgram::new_with_output_encoding(
        source,
        (metal, names[0].clone(), names[1].clone()),
        (hlsl, hlsl_names[0].clone(), hlsl_names[1].clone()),
        encoding,
    );
    Ok(if glyph {
        program.with_glyph_mask()
    } else if path {
        program.with_path_mask()
    } else {
        program
    })
}
