mod gradient_checks;
#[cfg(feature = "portable-shaders")]
mod portable_checks;
#[cfg(test)]
mod preparation_checks;
use super::*;
use std::time::Instant;

fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn base() -> Scene {
    let mut scene = Scene::default();
    let bounds = rect(0., 0., 1024., 320.);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: black().into(),
        ..Default::default()
    });
    scene
}
fn record(
    bounds: Bounds<ScaledPixels>,
    content: Arc<dyn CustomContent>,
    coverage: Option<CustomCoverage>,
) -> CustomDraw {
    CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask {
            bounds: rect(0., 0., 1024., 320.),
        },
        corner_radii: Corners::all(ScaledPixels(24.)),
        opacity: 1.,
        content,
        coverage,
    }
}
fn border() -> CustomCoverage {
    CustomCoverage::Border {
        widths: Edges {
            top: ScaledPixels(9.),
            right: ScaledPixels(17.),
            bottom: ScaledPixels(13.),
            left: ScaledPixels(6.),
        },
        style: BorderStyle::Dashed,
    }
}
fn path(bounds: Bounds<ScaledPixels>, stroke: bool) -> Path<ScaledPixels> {
    shape(
        Bounds::new(bounds.origin.map(|p| px(p.0)), bounds.size.map(|p| px(p.0))),
        stroke,
    )
    .scale(1.)
}
fn demo_scene(content: Arc<dyn CustomContent>) -> Scene {
    let mut scene = base();
    for index in 0..3 {
        let bounds = rect(8. + index as f32 * 336., 24., 320., 240.);
        let coverage = match index {
            0 => None,
            1 => Some(border()),
            _ => Some(CustomCoverage::Path(Arc::new(path(bounds, false)))),
        };
        scene.insert_primitive(record(bounds, content.clone(), coverage));
    }
    scene.finish();
    scene
}

pub fn run(
    effect: Effect,
    frames: usize,
    png: Option<&std::path::Path>,
    wgpu: bool,
    portable_shader: bool,
) -> Result<()> {
    let mut renderer: Box<dyn PlatformHeadlessRenderer> = if wgpu {
        #[cfg(feature = "wgpu")]
        {
            Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)
        }
        #[cfg(not(feature = "wgpu"))]
        {
            anyhow::bail!("rebuild paint-lab with --features wgpu");
        }
    } else {
        gpui_platform::current_headless_renderer()?.expect("native renderer")
    };
    verify(renderer.as_mut())?;
    let mesh = Arc::new(MeshGradient::new(grid(0.)?, 18)?);
    let analytic = Arc::new(Gradient::new(
        effect
            .kind(0.)
            .unwrap_or(GradientKind::Linear { angle: 90. }),
        gradient_stops(),
    )?);
    let content: Arc<dyn CustomContent> = if portable_shader {
        #[cfg(feature = "portable-shaders")]
        {
            let program = crate::portable::program()?;
            Arc::new(crate::portable::paint(&program, 0.15, 1.)?)
        }
        #[cfg(not(feature = "portable-shaders"))]
        {
            anyhow::bail!("rebuild with --features portable-shaders");
        }
    } else if effect == Effect::Mesh {
        mesh.clone()
    } else {
        analytic.clone()
    };
    let scene = demo_scene(content);
    let start = Instant::now();
    let mut image = None;
    for _ in 0..frames {
        image = Some(
            renderer.render_scene_to_image(&scene, size(DevicePixels(1024), DevicePixels(320)))?,
        );
    }
    if let Some(png) = png {
        image.unwrap().save(png)?;
    }
    println!(
        "{}",
        serde_json::json!({"frames":frames,"cpu_wall_readback_ms":start.elapsed().as_secs_f64()*1000.,"effect":if portable_shader {"Portable WGSL"}else{effect.label()},"source_generations":if portable_shader {None}else if effect==Effect::Mesh {Some(mesh.preparations())}else{Some(analytic.preparations())},"pipeline_compilations":if portable_shader {None}else if effect==Effect::Mesh {Some(mesh.pipeline_compilations())}else{Some(analytic.pipeline_compilations())},"checks":"native coverage, alpha, clip mapping, replay, resize, source sharing","timing_scope":"CPU wall including GPU wait and readback; not GPU-only time or FPS"})
    );
    Ok(())
}

fn verify(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let extent = size(DevicePixels(1024), DevicePixels(320));
    gradient_checks::verify(renderer)?;
    #[cfg(feature = "portable-shaders")]
    portable_checks::verify(renderer)?;
    // Native white geometry is an independent reference for border and path coverage.
    for stroke in [false, true] {
        let mut native = base();
        let mut custom = base();
        let white_source = Arc::new(SolidColor::new([1., 1., 1., 1.])?);
        let bounds = rect(40., 30., 320., 240.);
        let CustomCoverage::Border { widths, style } = border() else {
            unreachable!()
        };
        native.insert_primitive(Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: transparent_black().into(),
            border_color: white(),
            corner_radii: Corners::all(ScaledPixels(24.)),
            border_widths: widths,
            border_style: style,
            ..Default::default()
        });
        custom.insert_primitive(record(bounds, white_source.clone(), Some(border())));
        let bounds = rect(420., 30., 320., 240.);
        let mut geometry = path(bounds, stroke);
        geometry.content_mask = ContentMask { bounds };
        geometry.color = white().into();
        native.insert_primitive(geometry.clone());
        custom.insert_primitive(record(
            bounds,
            white_source.clone(),
            Some(CustomCoverage::Path(Arc::new(geometry))),
        ));
        native.finish();
        custom.finish();
        let expected = renderer.render_scene_to_image(&native, extent)?;
        let actual = renderer.render_scene_to_image(&custom, extent)?;
        let mut mismatches = 0;
        for (a, b) in expected.pixels().zip(actual.pixels()) {
            if a.0.iter().zip(b.0).any(|(a, b)| a.abs_diff(b) > 3) {
                mismatches += 1;
            }
        }

        ensure!(
            mismatches == 0,
            "native/custom coverage differs at {mismatches} pixels (stroke={stroke})"
        );
        ensure!(
            white_source.preparations() == 1,
            "source not shared across coverage types"
        );
        let mut replay = Scene::default();
        replay.replay(0..custom.len(), &custom);
        replay.finish();
        ensure!(
            renderer.render_scene_to_image(&replay, extent)?.as_raw() == actual.as_raw(),
            "paint replay differs"
        );
    }
    // Straight source alpha and inherited opacity multiply once through all masks.
    let half = Arc::new(SolidColor::new([1., 1., 1., 0.5])?);
    let mut scene = demo_scene(half);
    // Modify retained records and their replay operations consistently by rebuilding.
    let mut translucent = base();
    for draw in &mut scene.custom_draws {
        draw.opacity = 0.5;
        translucent.insert_primitive(draw.clone());
    }
    translucent.finish();
    let image = renderer.render_scene_to_image(&translucent, extent)?;
    for (x, y) in [(160, 140), (400, 27), (840, 150)] {
        ensure!(
            image.get_pixel(x, y).0[0].abs_diff(64) <= 3,
            "alpha/opacity wrong at {x},{y}: {:?}",
            image.get_pixel(x, y)
        );
    }
    // The private native-background adapter must undo premultiplication before
    // the straight-alpha paint compositor, including non-solid border paints.
    #[derive(Debug)]
    struct NativeBackground(Background);
    impl CustomContent for NativeBackground {
        fn prepare(
            &self,
            _: &dyn std::any::Any,
            _: Size<DevicePixels>,
        ) -> Result<Box<dyn std::any::Any>> {
            Ok(Box::new(self.0))
        }
    }
    let gradient = linear_gradient(
        90.,
        linear_color_stop(hsla(0., 0., 1., 0.5), 0.),
        linear_color_stop(hsla(0., 0., 1., 0.5), 1.),
    );
    let mut adapted = base();
    for draw in &translucent.custom_draws {
        let mut draw = draw.clone();
        draw.content = Arc::new(NativeBackground(gradient));
        adapted.insert_primitive(draw);
    }
    adapted.finish();
    let adapted = renderer.render_scene_to_image(&adapted, extent)?;
    for (x, y) in [(160, 140), (400, 27), (840, 150)] {
        ensure!(
            adapted.get_pixel(x, y).0[0].abs_diff(64) <= 3,
            "native paint alpha was applied twice"
        );
    }
    // A nonuniform source must sample the same coordinates after clipping.
    let mesh = Arc::new(MeshGradient::new(grid(0.)?, 18)?);
    let full = demo_scene(mesh.clone());
    let expected = renderer.render_scene_to_image(&full, extent)?;
    let mut clipped = base();
    for draw in &full.custom_draws {
        let mut draw = draw.clone();
        draw.content_mask.bounds = rect(draw.bounds.origin.x.0 + 100., 0., 220., 320.);
        clipped.insert_primitive(draw);
    }
    clipped.finish();
    let actual = renderer.render_scene_to_image(&clipped, extent)?;
    for (x, y) in [(150, 130), (350 + 100, 27), (820, 150)] {
        ensure!(
            expected.get_pixel(x, y) == actual.get_pixel(x, y),
            "clipping remapped paint at {x},{y}"
        );
    }
    ensure!(actual.get_pixel(40, 130).0 == [0, 0, 0, 255], "clip leaked");
    ensure!(mesh.preparations() == 1, "clip regenerated source");
    let mut resized = base();
    resized.insert_primitive(record(
        rect(20., 20., 160., 120.),
        mesh.clone(),
        Some(border()),
    ));
    resized.finish();
    renderer.render_scene_to_image(&resized, extent)?;
    ensure!(
        mesh.preparations() == 2,
        "resize failed to regenerate source"
    );
    // Runtime paints fail serialization explicitly; native paints round-trip.
    let paint = Paint::Custom(mesh);
    ensure!(
        serde_json::to_string(&paint).is_err(),
        "runtime paint serialized silently"
    );
    let native: Paint = white().into();
    let decoded: Paint = serde_json::from_str(&serde_json::to_string(&native)?)?;
    ensure!(decoded == native, "native paint serialization changed");
    Ok(())
}
#[cfg(test)]
#[::core::prelude::v1::test]
fn native_extension_paints_match_native_geometry() -> Result<()> {
    let mut renderer = gpui_platform::current_headless_renderer()?.expect("native renderer");
    verify(renderer.as_mut())
}

#[cfg(all(test, feature = "wgpu"))]
#[::core::prelude::v1::test]
fn wgpu_extension_paints_match_native_geometry() -> Result<()> {
    let mut renderer = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    verify(&mut renderer)
}

#[cfg(all(
    test,
    feature = "wgpu",
    any(target_os = "macos", target_os = "windows")
))]
#[::core::prelude::v1::test]
fn wgpu_mesh_matches_native_interior_samples() -> Result<()> {
    let mut metal = gpui_platform::current_headless_renderer()?.expect("native renderer");
    let mut portable = gpui_wgpu::WgpuHeadlessRenderer::new()?;
    for time in [0., 0.35, 0.8] {
        let mesh = Arc::new(MeshGradient::new(grid(time)?, 18)?);
        let bounds = rect(0., 0., 160., 120.);
        let mut scene = base();
        let mut draw = record(bounds, mesh, None);
        draw.corner_radii = Corners::default();
        scene.insert_primitive(draw);
        scene.finish();
        let extent = size(DevicePixels(160), DevicePixels(120));
        let expected = metal.render_scene_to_image(&scene, extent)?;
        let actual = portable.render_scene_to_image(&scene, extent)?;
        for y in (12..108).step_by(12) {
            for x in (16..144).step_by(16) {
                let expected = expected.get_pixel(x, y).0;
                let actual = actual.get_pixel(x, y).0;
                ensure!(
                    actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3),
                    "mesh parity at t={time}, {x},{y}: {actual:?} vs {expected:?}"
                );
            }
        }
    }
    Ok(())
}
