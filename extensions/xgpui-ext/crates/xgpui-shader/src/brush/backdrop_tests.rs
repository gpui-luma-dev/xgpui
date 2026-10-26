use super::*;
use crate::BackdropBlur;
use gpui::{BackdropDraw, DevicePixels, PlatformHeadlessRenderer, Quad, ScaledPixels};

fn backdrop(radius: f32, opacity: f32) -> BackdropDraw {
    let full = Bounds::new(point(px(0.), px(0.)), size(px(96.), px(64.))).scale(1.);
    let surface = Bounds::new(point(px(16.), px(12.)), size(px(64.), px(40.))).scale(1.);
    BackdropDraw {
        order: 0,
        bounds: full,
        content_mask: ContentMask { bounds: full },
        surface_bounds: surface,
        clip: ContentMask { bounds: full },
        corner_radii: Corners::all(ScaledPixels(8.)),
        radius,
        opacity,
        program: BackdropBlur::new(px(16.)).unwrap().program,
    }
}
fn rect(scene: &mut Scene, x: f32, y: f32, w: f32, h: f32, color: gpui::Hsla) {
    let bounds = Bounds::new(point(px(x), px(y)), size(px(w), px(h))).scale(1.);
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: color.into(),
        ..Default::default()
    });
}
#[test]
fn backdrop_validation_caps_and_replay_barriers() {
    for value in [f32::NAN, -1., 33., f32::INFINITY] {
        assert!(BackdropBlur::new(px(value)).is_err());
    }
    assert!(BackdropBlur::new(px(0.)).is_ok());
    let extent = size(DevicePixels(96), DevicePixels(64));
    let d = backdrop(16., 1.);
    let plan = d.plan(extent).unwrap();
    assert_eq!(plan.origin, [0, 0]);
    assert_eq!(plan.size, [96, 64]);
    assert_eq!(plan.reduced, [24, 16]);
    for radius in [0., 129., f32::NAN] {
        let mut d = d.clone();
        d.radius = radius;
        assert!(d.plan(extent).is_err());
    }
    let mut large = d.clone();
    large.surface_bounds.size.width = ScaledPixels(2048.);
    assert!(
        large
            .plan(size(DevicePixels(4096), DevicePixels(4096)))
            .is_err()
    );
    let mut scene = Scene::default();
    scene.push_layer(d.content_mask.bounds);
    rect(&mut scene, 0., 0., 96., 64., gpui::black());
    scene.insert_primitive(Primitive::Backdrop(d));
    rect(&mut scene, 30., 20., 8., 8., gpui::white());
    scene.pop_layer();
    let operations = scene.len();
    scene.finish();
    assert!(scene.quads[0].order < scene.backdrops[0].order);
    assert!(scene.backdrops[0].order < scene.quads[1].order);
    let mut replay = Scene::default();
    replay.replay(0..operations, &scene);
    replay.finish();
    assert_eq!(
        scene.batches().map(|b| b.label()).collect::<Vec<_>>(),
        replay.batches().map(|b| b.label()).collect::<Vec<_>>()
    );
}
#[cfg(feature = "wgpu")]
#[test]
fn backdrop_pixels_blur_clip_refresh_and_leave_children_sharp() {
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
    for renderer in &mut renderers {
        for inverse in [false, true, false] {
            let mut scene = Scene::default();
            for i in 0..24 {
                rect(
                    &mut scene,
                    i as f32 * 4.,
                    0.,
                    4.,
                    64.,
                    if (i % 2 == 0) ^ inverse {
                        gpui::white()
                    } else {
                        gpui::black()
                    },
                );
            }
            let mut filter = backdrop(16., 1.);
            filter.clip.bounds.size.width = ScaledPixels(64.);
            scene.insert_primitive(Primitive::Backdrop(filter));
            rect(&mut scene, 38., 28., 4., 4., gpui::rgb(0xff0000).into());
            scene.finish();
            let image = renderer
                .render_scene_to_image(&scene, size(DevicePixels(96), DevicePixels(64)))
                .unwrap();
            let middle = image.get_pixel(32, 24).0;
            assert!(
                (90..=165).contains(&middle[0]),
                "blurred stripes: {middle:?}"
            );
            assert!(
                image.get_pixel(39, 29).0[0] > 250 && image.get_pixel(39, 29).0[1] < 3,
                "child stays sharp"
            );
            for (x, y) in [(4, 4), (16, 12), (72, 24)] {
                let expected = if ((x / 4) % 2 == 0) ^ inverse { 255 } else { 0 };
                assert!(
                    image.get_pixel(x, y).0[0].abs_diff(expected) <= 1,
                    "outside/round/clip at {x},{y}"
                );
            }
            // A uniform next frame must not retain the prior stripe capture.
            let mut solid = Scene::default();
            rect(&mut solid, 0., 0., 96., 64., gpui::rgb(0x204080).into());
            solid.insert_primitive(Primitive::Backdrop(backdrop(16., 1.)));
            solid.finish();
            let image = renderer
                .render_scene_to_image(&solid, size(DevicePixels(96), DevicePixels(64)))
                .unwrap();
            for (a, b) in image
                .get_pixel(40, 24)
                .0
                .into_iter()
                .zip([32u8, 64, 128, 255])
            {
                assert!(a.abs_diff(b) <= 2);
            }
        }
        let mut excess = Scene::default();
        for _ in 0..5 {
            excess.insert_primitive(Primitive::Backdrop(backdrop(16., 1.)));
        }
        excess.finish();
        assert!(
            renderer
                .render_scene_to_image(&excess, size(DevicePixels(96), DevicePixels(64)))
                .is_err()
        );
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn backdrop_checkerboard_matches_separable_reference() {
    // Double-precision reference, including bilinear reconstruction and each
    // intermediate's UNORM rounding. Both axes vary to catch a missing pass.
    type Image = Vec<Vec<[f64; 4]>>;
    fn bilinear(image: &Image, x: f64, y: f64) -> [f64; 4] {
        let x0 = x.floor();
        let y0 = y.floor();
        let fx = x - x0;
        let fy = y - y0;
        let at = |x: f64, y: f64| {
            image[(y as i32).clamp(0, image.len() as i32 - 1) as usize]
                [(x as i32).clamp(0, image[0].len() as i32 - 1) as usize]
        };
        let a = at(x0, y0);
        let b = at(x0 + 1., y0);
        let c = at(x0, y0 + 1.);
        let d = at(x0 + 1., y0 + 1.);
        std::array::from_fn(|i| {
            (a[i] * (1. - fx) + b[i] * fx) * (1. - fy) + (c[i] * (1. - fx) + d[i] * fx) * fy
        })
    }
    fn filter(image: &Image, horizontal: bool) -> Image {
        let (w, h) = (24, 16);
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| {
                        let position = [
                            (x as f64 + 0.5) / w as f64 * image[0].len() as f64 - 0.5,
                            (y as f64 + 0.5) / h as f64 * image.len() as f64 - 0.5,
                        ];
                        let mut color = [0.; 4];
                        let mut total = 0.;
                        for tap in -4i32..=4 {
                            let weight = (-0.5 * (tap as f64 * 0.75).powi(2)).exp();
                            let sample = bilinear(
                                image,
                                position[0] + if horizontal { tap as f64 * 4. } else { 0. },
                                position[1] + if horizontal { 0. } else { tap as f64 },
                            );
                            for i in 0..4 {
                                color[i] += sample[i] * weight;
                            }
                            total += weight;
                        }
                        color.map(|c| (c / total * 255.).round() / 255.)
                    })
                    .collect()
            })
            .collect()
    }
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
    for renderer in &mut renderers {
        for opacity in [0., 0.4, 1.] {
            let mut scene = Scene::default();
            for y in 0..8 {
                for x in 0..12 {
                    rect(
                        &mut scene,
                        x as f32 * 8.,
                        y as f32 * 8.,
                        8.,
                        8.,
                        if (x + y) % 2 == 0 {
                            gpui::rgb(0xff2000).into()
                        } else {
                            gpui::rgb(0x0040ff).into()
                        },
                    );
                }
            }
            scene.finish();
            let extent = size(DevicePixels(96), DevicePixels(64));
            let raw = renderer.render_scene_to_image(&scene, extent).unwrap();
            let original: Image = (0..64)
                .map(|y| {
                    (0..96)
                        .map(|x| raw.get_pixel(x, y).0.map(|v| v as f64 / 255.))
                        .collect()
                })
                .collect();
            let blurred = filter(&filter(&original, true), false);
            scene.insert_primitive(Primitive::Backdrop(backdrop(16., opacity)));
            scene.finish();
            let actual = renderer.render_scene_to_image(&scene, extent).unwrap();
            for y in 22..42 {
                for x in 26..70 {
                    let smooth = bilinear(
                        &blurred,
                        (x as f64 + 0.5) / 4. - 0.5,
                        (y as f64 + 0.5) / 4. - 0.5,
                    );
                    for channel in 0..4 {
                        let expected = (original[y][x][channel] * (1. - opacity as f64)
                            + smooth[channel] * opacity as f64)
                            * 255.;
                        assert!(
                            (actual.get_pixel(x as u32, y as u32).0[channel] as f64 - expected)
                                .abs()
                                <= 2.,
                            "{x},{y} channel {channel}, strength {opacity}"
                        );
                    }
                }
            }
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn painted_backdrop_obeys_opacity_and_precedes_own_background() {
    use gpui::*;
    use std::sync::{Arc, Mutex};
    struct Recorder {
        inner: gpui_wgpu::WgpuHeadlessRenderer,
        scene: Arc<Mutex<(Vec<BackdropDraw>, Vec<Quad>)>>,
    }
    impl PlatformHeadlessRenderer for Recorder {
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.inner.sprite_atlas()
        }
        fn render_scene(&mut self, scene: &Scene, extent: Size<DevicePixels>) -> Result<()> {
            *self.scene.lock().unwrap() = (scene.backdrops.clone(), scene.quads.clone());
            self.inner.render_scene(scene, extent)
        }
        fn render_scene_to_image(
            &mut self,
            scene: &Scene,
            extent: Size<DevicePixels>,
        ) -> Result<image::RgbaImage> {
            *self.scene.lock().unwrap() = (scene.backdrops.clone(), scene.quads.clone());
            self.inner.render_scene_to_image(scene, extent)
        }
    }
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(black())
                .opacity(0.5)
                .child(
                    crate::painted()
                        .w(px(64.))
                        .h(px(48.))
                        .rounded(px(8.))
                        .opacity(0.5)
                        .backdrop_blur(BackdropBlur::new(px(16.)).unwrap())
                        .bg(rgb(0xff0000))
                        .child(div().size(px(8.)).bg(rgb(0x00ff00))),
                )
                .child(
                    crate::painted()
                        .size(px(8.))
                        .backdrop_blur(BackdropBlur::new(px(0.)).unwrap()),
                )
        }
    }
    let captured = Arc::new(Mutex::new((Vec::<BackdropDraw>::new(), Vec::<Quad>::new())));
    let output = captured.clone();
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(NoopTextSystem::new()),
        Arc::new(()),
        move || {
            Ok(Some(Box::new(Recorder {
                inner: gpui_wgpu::WgpuHeadlessRenderer::new()?,
                scene: output.clone(),
            })))
        },
    );
    let window = cx
        .open_window(size(px(96.), px(64.)), |_, cx| cx.new(|_| View))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    cx.capture_screenshot(window.into()).unwrap();
    let scene = captured.lock().unwrap();
    assert_eq!(scene.0.len(), 1, "zero radius creates no pass");
    assert!((scene.0[0].opacity - 0.25).abs() < 0.00001);
    assert!(
        scene
            .1
            .iter()
            .filter(|q| q.background.as_solid().is_some_and(|c| c.s > 0.9))
            .all(|q| q.order > scene.0[0].order)
    );
}

trait CacheRenderer: PlatformHeadlessRenderer {
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats;
}
#[cfg(feature = "wgpu")]
impl CacheRenderer for gpui_wgpu::WgpuHeadlessRenderer {
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats {
        self.backdrop_cache_stats()
    }
}
#[cfg(target_os = "macos")]
impl CacheRenderer for gpui_apple::metal_renderer::MetalHeadlessRenderer {
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats {
        self.backdrop_cache_stats()
    }
}
#[cfg(target_os = "windows")]
impl CacheRenderer for gpui_windows::Dx11HeadlessRenderer {
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats {
        self.backdrop_cache_stats()
    }
}
#[cfg(feature = "wgpu")]
fn cache_renderers() -> Vec<Box<dyn CacheRenderer>> {
    let mut result: Vec<Box<dyn CacheRenderer>> =
        vec![Box::new(gpui_wgpu::WgpuHeadlessRenderer::new().unwrap())];
    #[cfg(target_os = "macos")]
    result.push(Box::new(
        gpui_apple::metal_renderer::MetalHeadlessRenderer::new(),
    ));
    #[cfg(target_os = "windows")]
    if std::env::var("XGPUI_BRUSH_NATIVE").as_deref() == Ok("1") {
        result.push(Box::new(gpui_windows::Dx11HeadlessRenderer::new().unwrap()));
    }
    result
}

#[cfg(feature = "wgpu")]
#[test]
fn backdrop_cache_reuses_retained_pixels_and_invalidates_source_filter_and_viewport() {
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in cache_renderers() {
        let mut source = Scene::default();
        for i in 0..24 {
            rect(
                &mut source,
                i as f32 * 4.,
                0.,
                4.,
                64.,
                if i % 2 == 0 {
                    gpui::white()
                } else {
                    gpui::black()
                },
            );
        }
        source.finish();
        let make = |draw: BackdropDraw, foreground: gpui::Hsla| {
            let mut scene = Scene::default();
            scene.replay(0..source.len(), &source);
            scene.insert_primitive(Primitive::Backdrop(draw));
            rect(&mut scene, 38., 28., 4., 4., foreground);
            scene.finish();
            scene
        };
        let first = make(backdrop(16., 1.), gpui::rgb(0xff0000).into());
        let image = renderer.render_scene_to_image(&first, extent).unwrap();
        assert_eq!(
            renderer.backdrop_stats(),
            gpui::BackdropCacheStats {
                captures: 1,
                hits: 0
            }
        );
        // A newly recorded boundary and foreground must not invalidate retained source pixels.
        let next = make(backdrop(16., 1.), gpui::rgb(0x00ff00).into());
        let reused = renderer.render_scene_to_image(&next, extent).unwrap();
        assert_eq!(image.get_pixel(32, 24), reused.get_pixel(32, 24));
        assert_eq!(reused.get_pixel(39, 29).0, [0, 255, 0, 255]);
        assert_eq!(
            renderer.backdrop_stats(),
            gpui::BackdropCacheStats {
                captures: 1,
                hits: 1
            }
        );
        // Composition changes use the original source plus the same filtered result.
        let mut composition = backdrop(16., 0.5);
        composition.corner_radii = Corners::all(ScaledPixels(0.));
        composition.clip.bounds.size.width = ScaledPixels(64.);
        let adjusted = make(composition.clone(), gpui::black());
        let actual = renderer.render_scene_to_image(&adjusted, extent).unwrap();
        assert_eq!(
            renderer.backdrop_stats(),
            gpui::BackdropCacheStats {
                captures: 1,
                hits: 2
            }
        );
        let mut fresh = cache_renderers().remove(0);
        let expected = fresh.render_scene_to_image(&adjusted, extent).unwrap();
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            assert!(a.0.into_iter().zip(b.0).all(|(a, b)| a.abs_diff(b) <= 2));
        }
        // Radius, movement and viewport each invalidate even if allocation size stays equal.
        for (draw, size) in [
            (backdrop(12., 1.), extent),
            (
                {
                    let mut d = backdrop(12., 1.);
                    d.surface_bounds.origin.x = ScaledPixels(17.);
                    d
                },
                extent,
            ),
            (
                backdrop(12., 1.),
                gpui::size(DevicePixels(97), DevicePixels(64)),
            ),
        ] {
            let scene = make(draw, gpui::black());
            let before = renderer.backdrop_stats();
            renderer.render_scene_to_image(&scene, size).unwrap();
            assert_eq!(renderer.backdrop_stats().captures, before.captures + 1);
            assert_eq!(renderer.backdrop_stats().hits, before.hits);
        }
        let mut changed = Scene::default();
        rect(&mut changed, 0., 0., 96., 64., gpui::rgb(0x204080).into());
        changed.insert_primitive(Primitive::Backdrop(backdrop(16., 1.)));
        changed.finish();
        let before = renderer.backdrop_stats();
        let actual = renderer.render_scene_to_image(&changed, extent).unwrap();
        assert_eq!(actual.get_pixel(40, 24).0, [32, 64, 128, 255]);
        assert_eq!(renderer.backdrop_stats().captures, before.captures + 1);
        renderer.render_scene_to_image(&changed, extent).unwrap();
        assert_eq!(renderer.backdrop_stats().hits, before.hits + 1);
    }
}

#[test]
fn backdrop_cache_keys_include_prior_boundaries_and_reject_live_external_content() {
    use gpui::{BackdropCache, CustomDraw};
    let extent = size(DevicePixels(96), DevicePixels(64));
    let mut scene = Scene::default();
    rect(&mut scene, 0., 0., 96., 64., gpui::white());
    scene.insert_primitive(Primitive::Backdrop(backdrop(8., 1.)));
    scene.insert_primitive(Primitive::Backdrop(backdrop(16., 1.)));
    scene.finish();
    let keys = scene.backdrop_cache_keys(extent).unwrap();
    assert_eq!(keys[0].revision.len(), 1);
    assert_eq!(keys[1].revision.len(), 2);
    let mut cache = BackdropCache::default();
    cache.prepare(keys.clone(), &[0]);
    cache.start(0);
    cache.start(1);
    cache.commit(&[0, 1]);
    cache.prepare(keys.clone(), &[0]);
    assert!(cache.hit(0) && cache.hit(1));
    cache.prepare(keys.clone(), &[1]);
    assert!(
        !cache.hit(0) && !cache.hit(1),
        "clear/render settings changed"
    );
    cache.commit(&[0, 1]);
    cache.invalidate(0);
    cache.prepare(keys.clone(), &[1]);
    assert!(
        !cache.hit(0) && cache.hit(1),
        "reallocated texture invalidates its key"
    );
    cache.start(0); // Failed frame: no commit.
    cache.prepare(keys, &[1]);
    assert!(!cache.hit(0));
    let mut live = Scene::default();
    let d = backdrop(16., 1.);
    live.insert_primitive(CustomDraw {
        order: 0,
        bounds: d.bounds,
        content_mask: d.content_mask,
        corner_radii: Corners::default(),
        opacity: 1.,
        content: std::sync::Arc::new(crate::SolidColor::new([1.; 4]).unwrap()),
        coverage: None,
    });
    live.insert_primitive(Primitive::Backdrop(d));
    live.finish();
    assert!(!live.backdrop_cache_keys(extent).unwrap()[0].reusable);
}

#[cfg(feature = "wgpu")]
#[test]
fn backdrop_cache_refreshes_live_producers_and_chained_filters() {
    #[derive(Debug)]
    struct LiveColor(std::sync::Mutex<[f32; 4]>);
    impl gpui::CustomContent for LiveColor {
        fn prepare(
            &self,
            backend: &dyn std::any::Any,
            size: gpui::Size<DevicePixels>,
        ) -> Result<Box<dyn std::any::Any>> {
            gpui::CustomContent::prepare(
                &crate::SolidColor::new(*self.0.lock().unwrap())?,
                backend,
                size,
            )
        }
    }
    let extent = size(DevicePixels(96), DevicePixels(64));
    for mut renderer in cache_renderers() {
        let live = std::sync::Arc::new(LiveColor(std::sync::Mutex::new([0., 0., 1., 1.])));
        let mut scene = Scene::default();
        let d = backdrop(16., 1.);
        scene.insert_primitive(gpui::CustomDraw {
            order: 0,
            bounds: d.bounds,
            content_mask: d.content_mask,
            corner_radii: Corners::default(),
            opacity: 1.,
            content: live.clone(),
            coverage: None,
        });
        scene.insert_primitive(Primitive::Backdrop(d));
        scene.finish();
        for expected in [[0u8, 0, 255, 255], [255, 0, 0, 255]] {
            *live.0.lock().unwrap() = expected.map(|v| v as f32 / 255.);
            let before = renderer.backdrop_stats();
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            assert_eq!(
                image.get_pixel(40, 24).0,
                expected,
                "live source must refresh on replay"
            );
            assert_eq!(renderer.backdrop_stats().captures, before.captures + 1);
            assert_eq!(renderer.backdrop_stats().hits, before.hits);
        }
        let mut source = Scene::default();
        rect(&mut source, 0., 0., 48., 64., gpui::white());
        rect(&mut source, 48., 0., 48., 64., gpui::black());
        source.finish();
        let prefix = |radius| {
            let mut prefix = Scene::default();
            prefix.replay(0..source.len(), &source);
            prefix.insert_primitive(Primitive::Backdrop(backdrop(radius, 1.)));
            prefix.finish();
            prefix
        };
        let first = prefix(8.);
        let append = |prefix: &Scene| {
            let mut result = Scene::default();
            result.replay(0..prefix.len(), prefix);
            result.insert_primitive(Primitive::Backdrop(backdrop(16., 1.)));
            result.finish();
            result
        };
        renderer
            .render_scene_to_image(&append(&first), extent)
            .unwrap();
        let before = renderer.backdrop_stats();
        renderer
            .render_scene_to_image(&append(&first), extent)
            .unwrap();
        assert_eq!(renderer.backdrop_stats().hits, before.hits + 2);
        let before = renderer.backdrop_stats();
        renderer
            .render_scene_to_image(&append(&prefix(12.)), extent)
            .unwrap();
        assert_eq!(
            renderer.backdrop_stats().captures,
            before.captures + 2,
            "upstream filter changes invalidate downstream filtered pixels"
        );
    }
}
