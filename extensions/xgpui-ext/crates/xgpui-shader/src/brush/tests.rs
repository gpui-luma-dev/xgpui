use super::*;
use gpui::{ContentMask, Primitive, PrimitiveBatch, Scene, ShaderDraw, point, px, size};

fn stops() -> Vec<BrushStop> {
    vec![
        BrushStop::new(0., [1., 0., 0., 0.25]),
        BrushStop::new(1., [0., 0., 1., 0.75]),
    ]
}
fn bounds() -> Bounds<Pixels> {
    Bounds::new(point(px(8.), px(8.)), size(px(80.), px(40.)))
}
fn draw(brush: &Brush, quad: &PaintQuad, border: bool, scale: f32) -> ShaderDraw {
    let instance = if border {
        brush.border_instance(quad)
    } else {
        brush.instance(quad.bounds, quad.corner_radii)
    }
    .unwrap();
    ShaderDraw {
        path_mask: None,
        glyph_mask: None,
        order: 0,
        bounds: quad.bounds.scale(scale),
        content_mask: ContentMask {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.))).scale(scale),
        },
        opacity: 1.,
        scale_factor: scale,
        program: brush.program.scene_program(),
        data: brush.program.pack(&instance).unwrap(),
    }
}
fn quad() -> PaintQuad {
    gpui::quad(
        bounds(),
        Corners::all(px(8.)),
        gpui::transparent_black(),
        gpui::Edges::all(px(4.)),
        gpui::transparent_black(),
        BorderStyle::Solid,
    )
}

#[test]
fn validates_brush_values_and_geometry() {
    assert!(Brush::solid([f32::NAN; 4]).is_err());
    assert!(Brush::solid([2.; 4]).is_err());
    assert!(Brush::linear(f32::INFINITY, stops()).is_err());
    assert!(Brush::linear(0., vec![]).is_err());
    assert!(Brush::linear(0., vec![stops()[0]; 4]).is_err());
    assert!(Brush::linear(0., vec![stops()[1], stops()[0]]).is_err());
    assert!(Brush::linear(0., vec![BrushStop::new(-0.1, [0.; 4]), stops()[1]]).is_err());
    assert!(Brush::radial([f32::NAN, 0.], [0.5; 2], stops()).is_err());
    assert!(Brush::radial([0.5; 2], [0., 0.5], stops()).is_err());
    assert!(Brush::radial([0.5; 2], [f32::INFINITY; 2], stops()).is_err());
    let brush = Brush::linear(-270., stops()).unwrap();
    assert!((brush.parameters.gradient[0] - 1.).abs() < 0.00001);
    assert!(brush.instance(bounds(), Corners::all(px(-1.))).is_err());
    let mut quad = quad();
    quad.border_style = BorderStyle::Dashed;
    assert!(brush.border_instance(&quad).is_err());
    quad.border_style = BorderStyle::Solid;
    quad.border_widths.top = px(-1.);
    assert!(brush.border_instance(&quad).is_err());
    quad.border_widths.top = px(f32::NAN);
    assert!(brush.border_instance(&quad).is_err());
}

#[test]
fn portable_abi_batches_different_brushes_and_replays() {
    assert_eq!(std::mem::size_of::<BrushInstance>(), 128);
    let brushes = [
        Brush::solid([1.; 4]).unwrap(),
        Brush::linear(90., stops()).unwrap(),
        Brush::radial([0.5; 2], [0.5; 2], stops()).unwrap(),
        Brush::freeform(vec![
            crate::BrushPoint::new([0.2, 0.2], [1., 0., 0., 1.]),
            crate::BrushPoint::new([0.8, 0.8], [0., 0., 1., 1.]),
        ])
        .unwrap(),
        Brush::mesh([
            [1., 0., 0., 1.],
            [0., 1., 0., 1.],
            [0., 0., 1., 1.],
            [1.; 4],
        ])
        .unwrap(),
    ];
    let mut scene = Scene::default();
    for brush in &brushes {
        assert_eq!(brush.program.id(), brushes[0].program.id());
        scene.insert_primitive(Primitive::Shader(draw(brush, &quad(), false, 1.)));
    }
    scene.finish();
    assert_eq!(scene.batches().count(), 1);
    assert!(
        matches!(scene.batches().next().unwrap(), PrimitiveBatch::Shaders { range, .. } if range == (0..5))
    );
    assert!(
        scene.custom_draws.is_empty(),
        "brushes must not allocate producer outputs/coverage masks"
    );
    let mut replay = Scene::default();
    replay.replay(0..5, &scene);
    replay.finish();
    assert_eq!(replay.shader_draws.len(), 5);
    assert_eq!(replay.shader_draws[1].data, scene.shader_draws[1].data);
}

// CPU reference: interpolate associated channels, then compose over opaque black.
fn reference(stops: &[BrushStop], t: f32, opacity: f32) -> [u8; 3] {
    let mut a = stops[0];
    let mut b = a;
    for stop in stops {
        if stop.offset <= t {
            a = *stop;
            b = a;
        } else {
            b = *stop;
            break;
        }
    }
    let f = if b.offset > a.offset {
        ((t - a.offset) / (b.offset - a.offset)).clamp(0., 1.)
    } else {
        0.
    };
    std::array::from_fn(|i| {
        ((a.color[i] * a.color[3] * (1. - f) + b.color[i] * b.color[3] * f) * opacity * 255.)
            .round() as u8
    })
}

#[cfg(feature = "wgpu")]
#[test]
fn brush_wgpu_pixels_match_cpu_reference_and_native_coverage() {
    use gpui::{DevicePixels, PlatformHeadlessRenderer, Quad};
    let mut renderers: Vec<Box<dyn PlatformHeadlessRenderer>> =
        vec![Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap())];
    #[cfg(target_os = "macos")]
    renderers.push(Box::new(
        gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
    ));
    #[cfg(target_os = "windows")]
    if std::env::var("XGPUI_BRUSH_NATIVE").as_deref() == Ok("1") {
        renderers.push(Box::new(gpui_windows::Dx11HeadlessRenderer::new().unwrap()));
    }
    let colors = stops();
    let hard = vec![
        BrushStop::new(0.4, [1., 0., 0., 1.]),
        BrushStop::new(0.4, [0., 1., 0., 0.5]),
        BrushStop::new(0.8, [0., 0., 1., 1.]),
    ];
    let transparent = vec![
        BrushStop::new(0., [1., 0., 0., 0.]),
        BrushStop::new(1., [0., 0., 1., 0.01]),
    ];
    let close = vec![
        BrushStop::new(0.5, [1., 0., 0., 1.]),
        BrushStop::new(0.50000006, [0., 0., 1., 1.]),
    ];
    for (backend, renderer) in renderers.iter_mut().enumerate() {
        for scale in [1., 1.5, 2.] {
            let extent = size(
                DevicePixels((120. * scale) as i32),
                DevicePixels((80. * scale) as i32),
            );
            let backdrop = || {
                let mut scene = Scene::default();
                scene.insert_primitive(Quad {
                    bounds: Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.)))
                        .scale(scale),
                    content_mask: ContentMask {
                        bounds: Bounds::new(point(px(0.), px(0.)), size(px(120.), px(80.)))
                            .scale(scale),
                    },
                    background: gpui::black().into(),
                    ..Default::default()
                });
                scene
            };
            if scale == 1. {
                // Odd logical dimensions put a pixel center exactly at t=0.5.
                // Three coincident stops must select the last without division by zero.
                let brush = Brush::linear(
                    90.,
                    vec![
                        BrushStop::new(0.5, [1., 0., 0., 1.]),
                        BrushStop::new(0.5, [0., 1., 0., 1.]),
                        BrushStop::new(0.5, [0., 0., 1., 1.]),
                    ],
                )
                .unwrap();
                let mut shape = quad();
                shape.bounds.size = size(px(81.), px(39.));
                let mut scene = backdrop();
                scene.insert_primitive(Primitive::Shader(draw(&brush, &shape, false, scale)));
                scene.finish();
                let image = renderer.render_scene_to_image(&scene, extent).unwrap();
                assert_eq!(&image.get_pixel(48, 27).0[..3], &[0, 0, 255]);
                assert_eq!(&image.get_pixel(47, 27).0[..3], &[255, 0, 0]);
            }
            for stops in [&colors, &hard, &transparent, &close] {
                for radial in [false, true] {
                    let brush = if radial {
                        Brush::radial([0.5; 2], [0.5; 2], stops.clone())
                    } else {
                        Brush::linear(90., stops.clone())
                    }
                    .unwrap();
                    let mut scene = backdrop();
                    let mut record = draw(&brush, &quad(), false, scale);
                    record.opacity = 0.5;
                    record.content_mask.bounds.size.width = gpui::ScaledPixels(70. * scale);
                    scene.insert_primitive(Primitive::Shader(record));
                    scene.finish();
                    let image = renderer.render_scene_to_image(&scene, extent).unwrap();
                    for (x, y) in [(24., 24.), (40., 30.), (60., 36.)] {
                        let ix = (x * scale) as u32;
                        let iy = (y * scale) as u32;
                        let uv = [
                            ((ix as f32 + 0.5) / scale - 8.) / 80.,
                            ((iy as f32 + 0.5) / scale - 8.) / 40.,
                        ];
                        let t = if radial {
                            (((uv[0] - 0.5) / 0.5).powi(2) + ((uv[1] - 0.5) / 0.5).powi(2)).sqrt()
                        } else {
                            uv[0]
                        };
                        let expected = reference(stops, t, 0.5);
                        let pixel = image.get_pixel(ix, iy).0;
                        for i in 0..3 {
                            assert!(
                                pixel[i].abs_diff(expected[i]) <= 2,
                                "scale={scale} radial={radial} t={t} pixel={pixel:?} expected={expected:?}"
                            );
                        }
                    }
                    assert_eq!(
                        &image
                            .get_pixel((76. * scale) as u32, (30. * scale) as u32)
                            .0[..3],
                        &[0, 0, 0],
                        "clip must not remap the brush"
                    );
                }
            }
            // Compare the complete image to native solid fills/borders, including
            // unequal widths, zero-width sides, rounded edges and fractional DPI.
            for border in [false, true] {
                let brush = Brush::solid([1.; 4]).unwrap();
                let mut shape = quad();
                shape.corner_radii = Corners {
                    top_left: px(14.),
                    top_right: px(5.),
                    bottom_right: px(9.),
                    bottom_left: px(2.),
                };
                shape.border_widths = gpui::Edges {
                    top: px(0.),
                    right: px(6.),
                    bottom: px(3.),
                    left: px(2.),
                };
                let mut scene = backdrop();
                scene.insert_primitive(Primitive::Shader(draw(&brush, &shape, border, scale)));
                scene.finish();
                let actual = renderer.render_scene_to_image(&scene, extent).unwrap();
                let mut native = backdrop();
                native.insert_primitive(Quad {
                    bounds: shape.bounds.scale(scale),
                    content_mask: draw(&brush, &shape, border, scale).content_mask,
                    corner_radii: shape
                        .corner_radii
                        .map(|r| gpui::ScaledPixels(f32::from(r) * scale)),
                    border_widths: if border {
                        shape
                            .border_widths
                            .map(|w| gpui::ScaledPixels(f32::from(w) * scale))
                    } else {
                        Default::default()
                    },
                    border_color: gpui::white(),
                    background: if border {
                        gpui::transparent_black().into()
                    } else {
                        gpui::white().into()
                    },
                    ..Default::default()
                });
                native.finish();
                let expected = renderer.render_scene_to_image(&native, extent).unwrap();
                for (index, (a, b)) in actual.pixels().zip(expected.pixels()).enumerate() {
                    for i in 0..3 {
                        assert!(
                            a[i].abs_diff(b[i]) <= 2,
                            "native coverage backend={backend} border={border} scale={scale} xy=({},{}): {a:?} != {b:?}",
                            index % actual.width() as usize,
                            index / actual.width() as usize
                        );
                    }
                }
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn brush_surface_preserves_clipping_opacity_and_child_order() {
    use gpui::*;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Captured {
        brushes: Vec<ShaderDraw>,
        quads: Vec<Quad>,
        custom_count: usize,
    }
    struct Recorder {
        inner: gpui_wgpu::WgpuHeadlessRenderer,
        captured: Arc<Mutex<Captured>>,
    }
    impl PlatformHeadlessRenderer for Recorder {
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.inner.sprite_atlas()
        }
        fn render_scene(&mut self, scene: &Scene, extent: Size<DevicePixels>) -> Result<()> {
            *self.captured.lock().unwrap() = Captured {
                brushes: scene.shader_draws.clone(),
                quads: scene.quads.clone(),
                custom_count: scene.custom_draws.len(),
            };
            self.inner.render_scene(scene, extent)
        }
        fn render_scene_to_image(
            &mut self,
            scene: &Scene,
            extent: Size<DevicePixels>,
        ) -> Result<image::RgbaImage> {
            *self.captured.lock().unwrap() = Captured {
                brushes: scene.shader_draws.clone(),
                quads: scene.quads.clone(),
                custom_count: scene.custom_draws.len(),
            };
            self.inner.render_scene_to_image(scene, extent)
        }
    }
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().bg(black()).child(
                div()
                    .w(px(80.))
                    .h(px(100.))
                    .opacity(0.5)
                    .overflow_hidden()
                    .child(
                        crate::painted()
                            .w(px(64.))
                            .h(px(48.))
                            .ml(px(24.))
                            .mt(px(24.))
                            .rounded(px(8.))
                            .border_8()
                            .opacity(0.5)
                            .overflow_hidden()
                            .bg_brush(Brush::solid([1., 0., 0., 1.]).unwrap())
                            .border_brush(Brush::solid([0., 0., 1., 1.]).unwrap())
                            .child(
                                canvas(
                                    |_, _, _| (),
                                    |_, (), window, _| {
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                point(px(0.), px(0.)),
                                                size(px(128.), px(100.)),
                                            ),
                                            rgb(0x00ff00),
                                        ))
                                    },
                                )
                                .size_full(),
                            ),
                    ),
            )
        }
    }
    let captured = Arc::new(Mutex::new(Captured::default()));
    let output = captured.clone();
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(NoopTextSystem::new()),
        Arc::new(()),
        move || {
            Ok(Some(Box::new(Recorder {
                inner: gpui_wgpu::WgpuHeadlessRenderer::new()?,
                captured: output.clone(),
            })))
        },
    );
    let window = cx
        .open_window(size(px(128.), px(100.)), |_, cx| cx.new(|_| View))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let pixel = |x: f32, y: f32| image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
    assert!(
        pixel(28., 48.)[2].abs_diff(64) <= 2,
        "border opacity: {:?}",
        pixel(28., 48.)
    );
    assert!(
        pixel(40., 48.)[1].abs_diff(64) <= 2,
        "child opacity: {:?}",
        pixel(40., 48.)
    );
    assert_eq!(&pixel(90., 48.)[..3], &[0, 0, 0], "ancestor clip");
    let result = captured.lock().unwrap();
    assert_eq!(
        result.custom_count, 0,
        "brushes must not use offscreen custom producers"
    );
    assert_eq!(result.brushes.len(), 2);
    let background = &result.brushes[0];
    let border = &result.brushes[1];
    assert_eq!(background.opacity, 0.25);
    assert_eq!(border.opacity, 0.25);
    let child = result
        .quads
        .iter()
        .find(|q| {
            q.background
                .as_solid()
                .is_some_and(|c| c.s > 0.99 && (c.h - 1. / 3.).abs() < 0.01)
        })
        .unwrap();
    assert!(background.order < child.order && child.order < border.order);
    assert_eq!(
        child.content_mask.bounds,
        Bounds::new(point(px(32.), px(32.)), size(px(48.), px(32.))).scale(scale)
    );
}

#[path = "noise_tests.rs"]
mod noise_tests;

#[path = "text_tests.rs"]
mod text_tests;

#[path = "spatial_tests.rs"]
mod spatial_tests;

#[path = "backdrop_tests.rs"]
mod backdrop_tests;

#[path = "layer_tests.rs"]
mod layer_tests;

#[path = "path_tests.rs"]
mod path_tests;
