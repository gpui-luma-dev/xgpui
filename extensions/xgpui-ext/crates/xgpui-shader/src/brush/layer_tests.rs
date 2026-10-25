use super::*;
use crate::{LayerEffect, backdrop};
use gpui::{
    DevicePixels, LayerCacheStats, LayerCapture, PlatformHeadlessRenderer, Quad, ScaledPixels,
};
use std::sync::Arc;
fn viewport() -> ContentMask<ScaledPixels> {
    ContentMask {
        bounds: Bounds::new(point(px(0.), px(0.)), size(px(96.), px(64.))).scale(1.),
    }
}
fn capture(opacity: f32) -> Arc<LayerCapture> {
    Arc::new(LayerCapture {
        clip: None,
        blur_quality: gpui::LayerBlurQuality::Adaptive,
        blur_radius: ScaledPixels(0.),
        blend_mode: gpui::LayerBlendMode::Normal,
        bounds: Bounds::new(point(px(8.), px(8.)), size(px(80.), px(48.))).scale(1.),
        opacity,
        saturation: 1.,
        brightness: 1.,
        contrast: 1.,
        invert: 0.,
        program: backdrop::shared_program().unwrap(),
    })
}
fn rect(scene: &mut Scene, x: f32, y: f32, w: f32, h: f32, color: gpui::Hsla) {
    scene.insert_primitive(Quad {
        bounds: Bounds::new(point(px(x), px(y)), size(px(w), px(h))).scale(1.),
        content_mask: viewport(),
        background: color.into(),
        ..Default::default()
    });
}
fn scene(nested: bool, translucent: bool) -> Scene {
    let mut scene = Scene::default();
    rect(&mut scene, 0., 0., 96., 64., gpui::white());
    let outer = capture(0.5);
    scene.begin_layer_capture(outer.clone(), viewport());
    rect(
        &mut scene,
        16.,
        16.,
        40.,
        32.,
        gpui::rgba(if translucent { 0xff000080 } else { 0xff0000ff }).into(),
    );
    let inner = capture(0.5);
    if nested {
        scene.begin_layer_capture(inner.clone(), viewport());
    }
    rect(
        &mut scene,
        40.,
        16.,
        40.,
        32.,
        gpui::rgba(if translucent { 0x0000ff80 } else { 0x0000ffff }).into(),
    );
    if nested {
        scene.end_layer_capture(inner, viewport());
    }
    scene.end_layer_capture(outer, viewport());
    scene.finish();
    scene
}
#[test]
fn layer_validation_replay_revisions_and_limits() {
    for v in [f32::NAN, -0.1, 1.1, f32::INFINITY] {
        assert!(LayerEffect::opacity(v).is_err());
    }
    assert!(LayerEffect::opacity(0.).is_ok());
    assert!(LayerEffect::opacity(1.).is_ok());
    let source = scene(true, false);
    let extent = size(DevicePixels(96), DevicePixels(64));
    let plans = source.layer_plans(extent).unwrap();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].origin, [8, 8]);
    assert_eq!(plans[0].size, [80, 48]);
    assert!(plans.iter().all(|p| p.reusable));
    let mut replay = Scene::default();
    replay.replay(0..source.len(), &source);
    replay.finish();
    let reused = replay.layer_plans(extent).unwrap();
    assert!(plans.iter().zip(&reused).all(|(a, b)| a.matches(b)));
    let repainted = scene(true, false).layer_plans(extent).unwrap();
    assert!(plans.iter().zip(&repainted).all(|(a, b)| !a.matches(b)));
    let mut unclosed = Scene::default();
    unclosed.begin_layer_capture(capture(1.), viewport());
    unclosed.finish();
    assert!(unclosed.layer_plans(extent).is_err());
    let mut crossed = Scene::default();
    crossed.begin_layer_capture(capture(1.), viewport());
    crossed.end_layer_capture(capture(1.), viewport());
    crossed.finish();
    assert!(crossed.layer_plans(extent).is_err());
    let mut excess = Scene::default();
    for _ in 0..5 {
        let c = capture(1.);
        excess.begin_layer_capture(c.clone(), viewport());
        excess.end_layer_capture(c, viewport());
    }
    excess.finish();
    assert!(excess.layer_plans(extent).is_err());
    let mut escaped = Scene::default();
    let c = capture(1.);
    escaped.begin_layer_capture(c.clone(), viewport());
    rect(&mut escaped, 0., 0., 96., 64., gpui::white());
    escaped.end_layer_capture(c, viewport());
    escaped.finish();
    assert!(escaped.layer_plans(extent).is_err());
    let mut backdrop_inside = Scene::default();
    let c = capture(1.);
    backdrop_inside.begin_layer_capture(c.clone(), viewport());
    backdrop_inside.insert_primitive(Primitive::Backdrop(gpui::BackdropDraw {
        order: 0,
        bounds: viewport().bounds,
        content_mask: viewport(),
        surface_bounds: c.bounds,
        clip: viewport(),
        corner_radii: Corners::default(),
        radius: 16.,
        opacity: 1.,
        program: backdrop::shared_program().unwrap(),
    }));
    backdrop_inside.end_layer_capture(c, viewport());
    backdrop_inside.finish();
    assert!(backdrop_inside.layer_plans(extent).is_err());
    let full = ContentMask {
        bounds: Bounds::new(point(px(0.), px(0.)), size(px(2048.), px(2048.))).scale(1.),
    };
    let large = Arc::new(LayerCapture {
        clip: None,
        blur_quality: gpui::LayerBlurQuality::Adaptive,
        blur_radius: ScaledPixels(0.),
        blend_mode: gpui::LayerBlendMode::Normal,
        bounds: full.bounds,
        opacity: 1.,
        saturation: 1.,
        brightness: 1.,
        contrast: 1.,
        invert: 0.,
        program: backdrop::shared_program().unwrap(),
    });
    let mut oversized = Scene::default();
    oversized.begin_layer_capture(large.clone(), full);
    oversized.end_layer_capture(large, full);
    oversized.finish();
    assert!(
        oversized
            .layer_plans(size(DevicePixels(2048), DevicePixels(2048)))
            .is_err()
    );
}
trait TestRenderer: PlatformHeadlessRenderer {
    fn stats(&self) -> LayerCacheStats;
    fn effect_texture_count(&self) -> usize;
    fn layer_texture_bytes(&self) -> u64;
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats;
}
#[cfg(feature = "wgpu")]
impl TestRenderer for gpui_wgpu::WgpuHeadlessRenderer {
    fn stats(&self) -> LayerCacheStats {
        self.layer_cache_stats()
    }
    fn effect_texture_count(&self) -> usize {
        self.effect_texture_count()
    }
    fn layer_texture_bytes(&self) -> u64 {
        self.layer_texture_bytes()
    }
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats {
        self.backdrop_cache_stats()
    }
}
#[cfg(target_os = "macos")]
impl TestRenderer for gpui_apple::metal_renderer::MetalHeadlessRenderer {
    fn stats(&self) -> LayerCacheStats {
        self.layer_cache_stats()
    }
    fn effect_texture_count(&self) -> usize {
        self.effect_texture_count()
    }
    fn layer_texture_bytes(&self) -> u64 {
        self.layer_texture_bytes()
    }
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats {
        self.backdrop_cache_stats()
    }
}
#[cfg(target_os = "windows")]
impl TestRenderer for gpui_windows::Dx11HeadlessRenderer {
    fn stats(&self) -> LayerCacheStats {
        self.layer_cache_stats()
    }
    fn effect_texture_count(&self) -> usize {
        self.effect_texture_count()
    }
    fn layer_texture_bytes(&self) -> u64 {
        self.layer_texture_bytes()
    }
    fn backdrop_stats(&self) -> gpui::BackdropCacheStats {
        self.backdrop_cache_stats()
    }
}
#[cfg(feature = "wgpu")]
fn renderers() -> Vec<Box<dyn TestRenderer>> {
    let mut result: Vec<Box<dyn TestRenderer>> =
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
fn pixel(image: &image::RgbaImage, x: u32, y: u32, expected: [u8; 4]) {
    let actual = image.get_pixel(x, y).0;
    for (a, b) in actual.into_iter().zip(expected) {
        assert!(a.abs_diff(b) <= 2, "{x},{y}: {actual:?} != {expected:?}");
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn group_opacity_isolates_overlap_and_nested_groups() {
    for mut renderer in renderers() {
        let extent = size(DevicePixels(96), DevicePixels(64));
        for (nested, translucent, overlap) in [
            (false, false, [128, 128, 255, 255]),
            (true, false, [191, 128, 191, 255]),
            (false, true, [191, 159, 223, 255]),
        ] {
            let scene = scene(nested, translucent);
            let image = renderer.render_scene_to_image(&scene, extent).unwrap();
            pixel(&image, 48, 24, overlap);
            pixel(&image, 0, 0, [255; 4]);
            pixel(&image, 10, 10, [255; 4]);
            if !translucent {
                pixel(&image, 24, 24, [255, 128, 128, 255]);
            }
        }
    }
}
#[cfg(feature = "wgpu")]
#[test]
fn cached_group_reuses_pixels_over_a_live_destination_and_invalidates() {
    for mut renderer in renderers() {
        let extent = size(DevicePixels(96), DevicePixels(64));
        let original = scene(true, false);
        renderer.render_scene_to_image(&original, extent).unwrap();
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 2,
                hits: 0
            }
        );
        let mut replay = Scene::default();
        rect(&mut replay, 0., 0., 96., 64., gpui::black());
        replay.replay(1..original.len(), &original);
        replay.finish();
        let image = renderer.render_scene_to_image(&replay, extent).unwrap();
        pixel(&image, 48, 24, [64, 0, 64, 255]);
        pixel(&image, 24, 24, [128, 0, 0, 255]);
        pixel(&image, 10, 10, [0, 0, 0, 255]);
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 2,
                hits: 1
            },
            "a parent hit skips nested draws"
        );
        let mut changed = Scene::default();
        rect(&mut changed, 0., 0., 96., 64., gpui::black());
        let c = capture(0.5);
        changed.begin_layer_capture(c.clone(), viewport());
        rect(&mut changed, 16., 16., 64., 32., gpui::rgb(0x00ff00).into());
        changed.end_layer_capture(c, viewport());
        changed.finish();
        let image = renderer.render_scene_to_image(&changed, extent).unwrap();
        pixel(&image, 48, 24, [0, 128, 0, 255]);
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 3,
                hits: 1
            }
        );
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn group_draws_native_paths_brushes_and_external_content_in_order() {
    use gpui::{CustomDraw, PathBuilder};
    #[derive(Debug)]
    struct LiveColor(std::sync::Mutex<[f32; 4]>);
    impl gpui::CustomContent for LiveColor {
        fn prepare(
            &self,
            backend: &dyn std::any::Any,
            size: gpui::Size<DevicePixels>,
        ) -> Result<Box<dyn std::any::Any>> {
            let rgba = *self.0.lock().unwrap();
            gpui::CustomContent::prepare(&crate::SolidColor::new(rgba)?, backend, size)
        }
    }
    for mut renderer in renderers() {
        let live = Arc::new(LiveColor(std::sync::Mutex::new([0., 1., 0., 1.])));
        let mut scene = Scene::default();
        rect(&mut scene, 0., 0., 96., 64., gpui::white());
        let c = capture(0.5);
        scene.begin_layer_capture(c.clone(), viewport());
        let mut builder = PathBuilder::fill();
        builder.move_to(point(px(16.), px(16.)));
        builder.line_to(point(px(56.), px(16.)));
        builder.line_to(point(px(56.), px(40.)));
        builder.line_to(point(px(16.), px(40.)));
        builder.close();
        let mut path = builder.build().unwrap().scale(1.);
        path.color = gpui::rgb(0xff0000).into();
        path.content_mask = viewport();
        scene.insert_primitive(path);
        let mut record = draw(
            &Brush::solid([0., 0., 1., 1.]).unwrap(),
            &gpui::quad(
                Bounds::new(point(px(40.), px(16.)), size(px(40.), px(24.))),
                Corners::default(),
                gpui::transparent_black(),
                gpui::Edges::default(),
                gpui::transparent_black(),
                BorderStyle::Solid,
            ),
            false,
            1.,
        );
        record.content_mask = viewport();
        scene.insert_primitive(Primitive::Shader(record));
        let external_bounds = Bounds::new(point(px(24.), px(40.)), size(px(48.), px(8.))).scale(1.);
        scene.insert_primitive(CustomDraw {
            order: 0,
            bounds: external_bounds,
            content_mask: viewport(),
            corner_radii: Corners::default(),
            opacity: 1.,
            content: live.clone(),
            coverage: None,
        });
        scene.end_layer_capture(c, viewport());
        scene.finish();
        assert!(
            !scene
                .layer_plans(size(DevicePixels(96), DevicePixels(64)))
                .unwrap()[0]
                .reusable
        );
        for frame in 0..2 {
            if frame == 1 {
                *live.0.lock().unwrap() = [1., 0., 0., 1.];
            }
            let image = renderer
                .render_scene_to_image(&scene, size(DevicePixels(96), DevicePixels(64)))
                .unwrap();
            pixel(&image, 24, 24, [255, 128, 128, 255]);
            pixel(&image, 48, 24, [128, 128, 255, 255]);
            pixel(
                &image,
                40,
                44,
                if frame == 0 {
                    [128, 255, 128, 255]
                } else {
                    [255, 128, 128, 255]
                },
            );
        }
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 2,
                hits: 0
            },
            "external producers conservatively refresh on replay"
        );
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn painted_layer_scopes_ancestor_opacity_clips_overflow_and_matches_group_semantics() {
    use gpui::*;
    struct View;
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let colors = || {
                canvas(
                    |_, _, _| (),
                    |bounds, (), window, _| {
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin + point(px(16.), px(16.)),
                                size(px(40.), px(32.)),
                            ),
                            rgb(0xff0000),
                        ));
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin + point(px(40.), px(16.)),
                                size(px(64.), px(32.)),
                            ),
                            rgb(0x0000ff),
                        ));
                    },
                )
                .size_full()
            };
            div()
                .size_full()
                .bg(white())
                .flex()
                .child(div().w(px(96.)).h(px(64.)).opacity(0.25).child(colors()))
                .child(
                    div().w(px(96.)).h(px(64.)).opacity(0.5).child(
                        crate::painted()
                            .w(px(80.))
                            .h(px(64.))
                            .layer_effect(LayerEffect::opacity(0.5).unwrap())
                            .child(colors()),
                    ),
                )
        }
    }
    let mut cx =
        HeadlessAppContext::with_platform(Arc::new(NoopTextSystem::new()), Arc::new(()), || {
            Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)))
        });
    let window = cx
        .open_window(size(px(192.), px(80.)), |_, cx| cx.new(|_| View))
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    let at =
        |x: f32, y: f32, expected| pixel(&image, (x * scale) as u32, (y * scale) as u32, expected);
    at(48., 24., [191, 143, 207, 255]);
    at(144., 24., [191, 191, 255, 255]);
    at(120., 24., [255, 191, 191, 255]);
    at(184., 24., [255; 4]);
}

#[cfg(feature = "wgpu")]
#[test]
fn group_opacity_changes_reuse_unchanged_child_commands() {
    for mut renderer in renderers() {
        let mut original = Scene::default();
        rect(&mut original, 0., 0., 96., 64., gpui::white());
        let c = capture(0.5);
        original.begin_layer_capture(c.clone(), viewport());
        let from = original.len();
        rect(
            &mut original,
            16.,
            16.,
            64.,
            32.,
            gpui::rgb(0x0000ff).into(),
        );
        let to = original.len();
        original.end_layer_capture(c, viewport());
        original.finish();
        let extent = size(DevicePixels(96), DevicePixels(64));
        renderer.render_scene_to_image(&original, extent).unwrap();
        let mut changed = Scene::default();
        rect(&mut changed, 0., 0., 96., 64., gpui::white());
        let c = capture(0.25);
        changed.begin_layer_capture(c.clone(), viewport());
        changed.replay(from..to, &original);
        changed.end_layer_capture(c, viewport());
        changed.finish();
        let image = renderer.render_scene_to_image(&changed, extent).unwrap();
        pixel(&image, 48, 24, [191, 191, 255, 255]);
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 1,
                hits: 1
            },
            "group opacity updates only composition"
        );
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn group_zero_opacity_preserves_destination_and_can_reuse_its_capture() {
    for mut renderer in renderers() {
        let extent = size(DevicePixels(96), DevicePixels(64));
        let mut zero = Scene::default();
        rect(&mut zero, 0., 0., 96., 64., gpui::white());
        let c = capture(0.);
        zero.begin_layer_capture(c.clone(), viewport());
        let from = zero.len();
        rect(&mut zero, 16., 16., 64., 32., gpui::rgb(0x0000ff).into());
        let to = zero.len();
        zero.end_layer_capture(c, viewport());
        zero.finish();
        let image = renderer.render_scene_to_image(&zero, extent).unwrap();
        pixel(&image, 48, 24, [255; 4]);
        let mut shown = Scene::default();
        rect(&mut shown, 0., 0., 96., 64., gpui::white());
        let c = capture(0.5);
        shown.begin_layer_capture(c.clone(), viewport());
        shown.replay(from..to, &zero);
        shown.end_layer_capture(c, viewport());
        shown.finish();
        let image = renderer.render_scene_to_image(&shown, extent).unwrap();
        pixel(&image, 48, 24, [128, 128, 255, 255]);
        assert_eq!(
            renderer.stats(),
            LayerCacheStats {
                captures: 1,
                hits: 1
            }
        );
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn cached_child_view_reuses_capture_until_notified_and_handles_zero_opacity() {
    use gpui::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Recorder {
        inner: gpui_wgpu::WgpuHeadlessRenderer,
        output: Arc<Mutex<(LayerCacheStats, usize, usize)>>,
    }
    impl PlatformHeadlessRenderer for Recorder {
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.inner.sprite_atlas()
        }
        fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
            self.inner.render_scene(scene, size)?;
            *self.output.lock().unwrap() = (
                self.inner.layer_cache_stats(),
                scene.monochrome_sprites.len(),
                scene.subpixel_sprites.len(),
            );
            Ok(())
        }
        fn render_scene_to_image(
            &mut self,
            scene: &Scene,
            size: Size<DevicePixels>,
        ) -> Result<image::RgbaImage> {
            let image = self.inner.render_scene_to_image(scene, size)?;
            *self.output.lock().unwrap() = (
                self.inner.layer_cache_stats(),
                scene.monochrome_sprites.len(),
                scene.subpixel_sprites.len(),
            );
            Ok(image)
        }
    }
    struct Child {
        green: bool,
        renders: Arc<AtomicUsize>,
    }
    impl Render for Child {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders.fetch_add(1, Ordering::SeqCst);
            let green = self.green;
            div()
                .relative()
                .size_full()
                .font_family(".SystemUIFont")
                .text_size(px(12.))
                .text_color(black())
                .child(
                    canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| {
                            window.paint_quad(fill(bounds, rgb(0xff0000)));
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(40.), px(0.)),
                                    size(px(40.), bounds.size.height),
                                ),
                                rgb(if green { 0x00ff00 } else { 0x0000ff }),
                            ));
                        },
                    )
                    .absolute()
                    .size_full(),
                )
                .child("Captured glyphs")
        }
    }
    struct Parent {
        child: Entity<Child>,
        opacity: f32,
        dark: bool,
    }
    impl Render for Parent {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(if self.dark { black() } else { white() })
                .child(
                    crate::painted()
                        .w(px(80.))
                        .h(px(48.))
                        .layer_effect(LayerEffect::opacity(self.opacity).unwrap())
                        .child(
                            self.child
                                .clone()
                                .cached(StyleRefinement::default().w(px(80.)).h(px(48.))),
                        ),
                )
        }
    }
    let output = Arc::new(Mutex::new((LayerCacheStats::default(), 0, 0)));
    let saved = output.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let renders = count.clone();
    #[cfg(target_os = "macos")]
    let text = Arc::new(gpui_apple::AppleTextSystem::new());
    #[cfg(not(target_os = "macos"))]
    let text = Arc::new(NoopTextSystem::new());
    let mut cx = HeadlessAppContext::with_platform(text, Arc::new(()), move || {
        Ok(Some(Box::new(Recorder {
            inner: gpui_wgpu::WgpuHeadlessRenderer::new()?,
            output: saved.clone(),
        })))
    });
    let window = cx
        .open_window(size(px(96.), px(64.)), |_, cx| {
            cx.new(|cx| Parent {
                child: cx.new(|_| Child {
                    green: false,
                    renders,
                }),
                opacity: 0.5,
                dark: false,
            })
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let scale = cx
        .update_window(window.into(), |_, window, _| window.scale_factor())
        .unwrap();
    cx.capture_screenshot(window.into()).unwrap();
    let initial = *output.lock().unwrap();
    let initial_renders = count.load(Ordering::SeqCst);
    #[cfg(target_os = "macos")]
    {
        assert!(initial.1 > 0, "real captured text uses grayscale glyphs");
        assert_eq!(initial.2, 0);
    }
    for (opacity, dark, expected) in [(0., false, [255; 4]), (0.25, true, [0, 0, 64, 255])] {
        window
            .update(&mut cx, |view, _, cx| {
                view.opacity = opacity;
                view.dark = dark;
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let image = cx.capture_screenshot(window.into()).unwrap();
        pixel(&image, (48. * scale) as u32, (24. * scale) as u32, expected);
        let stats = output.lock().unwrap().0;
        assert_eq!(stats.captures, initial.0.captures);
        assert!(stats.hits > initial.0.hits);
        assert_eq!(count.load(Ordering::SeqCst), initial_renders);
    }
    window
        .update(&mut cx, |view, _, cx| {
            view.child.update(cx, |child, cx| {
                child.green = true;
                cx.notify();
            })
        })
        .unwrap();
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
        .unwrap();
    let image = cx.capture_screenshot(window.into()).unwrap();
    pixel(
        &image,
        (48. * scale) as u32,
        (24. * scale) as u32,
        [0, 64, 0, 255],
    );
    assert!(output.lock().unwrap().0.captures > initial.0.captures);
    assert!(count.load(Ordering::SeqCst) > initial_renders);
}

#[path = "layer_filter_tests.rs"]
mod filter_tests;

#[path = "layer_blend_tests.rs"]
mod blend_tests;

#[path = "layer_blur_tests.rs"]
mod blur_tests;

#[path = "effect_cleanup_tests.rs"]
mod effect_cleanup_tests;
