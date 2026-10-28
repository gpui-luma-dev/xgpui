use super::*;
use crate::brush::text::TextBrush;
use gpui::{
    AtlasKey, DevicePixels, FontId, GlyphId, MonochromeSprite, RenderGlyphParams,
    TransformationMatrix,
};
use std::borrow::Cow;

pub(super) fn glyph(tile: gpui::AtlasTile, x: f32, scale: f32) -> MonochromeSprite {
    MonochromeSprite {
        order: 0,
        pad: 0,
        bounds: Bounds::new(
            point(
                gpui::ScaledPixels(x * scale),
                gpui::ScaledPixels(8. * scale),
            ),
            size(
                gpui::ScaledPixels(tile.bounds.size.width.0 as f32),
                gpui::ScaledPixels(tile.bounds.size.height.0 as f32),
            ),
        ),
        content_mask: ContentMask {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(56.), px(32.))).scale(scale),
        },
        color: gpui::white(),
        tile,
        transformation: TransformationMatrix::unit(),
    }
}
pub(super) fn masked_draw(brush: &TextBrush, glyph: MonochromeSprite, scale: f32) -> ShaderDraw {
    let parameters = brush
        .parameters(
            &glyph,
            Bounds::new(point(px(8.), px(8.)), size(px(64.), px(16.))),
            scale,
        )
        .unwrap();
    ShaderDraw {
        order: 0,
        bounds: glyph.bounds,
        content_mask: glyph.content_mask,
        opacity: 0.5,
        scale_factor: scale,
        program: brush.program.scene_program(),
        path_mask: None,
        glyph_mask: Some(glyph.tile),
        data: brush.program.pack(&parameters).unwrap(),
    }
}

#[test]
fn text_programs_translate_share_identity_and_keep_instance_size() {
    let base = Brush::linear(90., stops()).unwrap();
    let a = TextBrush::new(base.clone()).unwrap();
    let b = TextBrush::new(base).unwrap();
    assert_eq!(a.program.id(), b.program.id());
    assert!(a.program.scene_program().uses_glyph_mask());
    assert_eq!(std::mem::size_of::<BrushInstance>(), 128);
    for kind in [
        crate::NoiseKind::Simplex,
        crate::NoiseKind::Worley,
        crate::NoiseKind::WorleyF2,
        crate::NoiseKind::Dither,
    ] {
        for mode in [
            crate::BlendMode::Normal,
            crate::BlendMode::Multiply,
            crate::BlendMode::Screen,
            crate::BlendMode::Overlay,
            crate::BlendMode::ColorDodge,
            crate::BlendMode::SoftLight,
            crate::BlendMode::LinearBurn,
        ] {
            let noise = crate::Noise {
                kind,
                frequency: if kind == crate::NoiseKind::Dither {
                    1.
                } else {
                    0.04
                },
                octaves: 1,
                strength: 0.5,
            };
            TextBrush::new(
                Brush::solid([1.; 4])
                    .unwrap()
                    .with_noise(noise, mode)
                    .unwrap(),
            )
            .unwrap();
        }
    }
}

#[cfg(feature = "wgpu")]
#[test]
fn glyph_atlas_mask_multiplies_alpha_and_uses_full_text_bounds() {
    use gpui::{PlatformHeadlessRenderer, Quad};
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
    let stops = stops();
    let brush = TextBrush::new(Brush::linear(90., stops.clone()).unwrap()).unwrap();
    for renderer in &mut renderers {
        for scale in [1., 1.5, 2.] {
            let n = (16. * scale) as i32;
            let bytes: Vec<u8> = (0..n * n)
                .map(|i| [0, 64, 128, 255][((i % n) % 4) as usize])
                .collect();
            let key = AtlasKey::Glyph(RenderGlyphParams {
                font_id: FontId(0),
                glyph_id: GlyphId(n as u32),
                font_size: px(16.),
                subpixel_variant: point(0, 0),
                scale_factor: scale,
                is_emoji: false,
                subpixel_rendering: false,
                dilation: 0,
            });
            let tile = renderer
                .sprite_atlas()
                .get_or_insert_with(key, &mut || {
                    Ok(Some((
                        size(DevicePixels(n), DevicePixels(n)),
                        Cow::Owned(bytes.clone()),
                    )))
                })
                .unwrap()
                .unwrap();
            let mut scene = Scene::default();
            let full = Bounds::new(point(px(0.), px(0.)), size(px(96.), px(32.))).scale(scale);
            scene.insert_primitive(Quad {
                bounds: full,
                content_mask: ContentMask { bounds: full },
                background: gpui::black().into(),
                ..Default::default()
            });
            scene.insert_primitive(Primitive::Shader(masked_draw(
                &brush,
                glyph(tile, 8., scale),
                scale,
            )));
            scene.insert_primitive(Primitive::Shader(masked_draw(
                &brush,
                glyph(tile, 48., scale),
                scale,
            )));
            scene.finish();
            let image = renderer
                .render_scene_to_image(
                    &scene,
                    size(
                        DevicePixels((96. * scale) as i32),
                        DevicePixels((32. * scale) as i32),
                    ),
                )
                .unwrap();
            for origin in [8., 48.] {
                for local_x in 0..8 {
                    let x = (origin * scale) as u32 + local_x;
                    let y = (12. * scale) as u32;
                    let coverage = [0, 64, 128, 255][local_x as usize % 4] as f32 / 255.;
                    let t = ((x as f32 + 0.5) / scale - 8.) / 64.;
                    let expected = reference(&stops, t, 0.5 * coverage);
                    let pixel = image.get_pixel(x, y).0;
                    for i in 0..3 {
                        assert!(
                            pixel[i].abs_diff(expected[i]) <= 2,
                            "glyph scale={scale} x={x}: {pixel:?}, expected={expected:?}"
                        );
                    }
                }
            }
            assert_eq!(
                &image
                    .get_pixel((60. * scale) as u32, (12. * scale) as u32)
                    .0[..3],
                &[0, 0, 0],
                "clip must not remap glyph brush coordinates"
            );
            assert!(scene.custom_draws.is_empty());
            assert!(scene.monochrome_sprites.is_empty());
            assert_eq!(
                scene
                    .batches()
                    .filter(|b| matches!(b, PrimitiveBatch::Shaders { .. }))
                    .count(),
                1
            );
            let mut replay = Scene::default();
            replay.replay(0..scene.len(), &scene);
            replay.finish();
            let repeated = renderer
                .render_scene_to_image(
                    &replay,
                    size(
                        DevicePixels((96. * scale) as i32),
                        DevicePixels((32. * scale) as i32),
                    ),
                )
                .unwrap();
            assert_eq!(
                image, repeated,
                "replay must retain glyph atlas coverage and paint snapshot"
            );
            // Same program, different atlas textures cannot form one batch.
            let mut second = masked_draw(&brush, glyph(tile, 48., scale), scale);
            second.glyph_mask.as_mut().unwrap().texture_id.index += 1;
            let mut split = Scene::default();
            split.insert_primitive(Primitive::Shader(masked_draw(
                &brush,
                glyph(tile, 8., scale),
                scale,
            )));
            split.insert_primitive(Primitive::Shader(second));
            split.finish();
            assert_eq!(split.batches().count(), 2);
        }
    }
}

#[cfg(all(feature = "wgpu", target_os = "macos"))]
#[test]
fn real_text_wrapping_nested_and_deferred_scopes_leave_emoji_and_plain_text_native() {
    use gpui::*;
    use std::sync::{Arc, Mutex};
    #[derive(Default)]
    struct Captured {
        shaders: Vec<ShaderDraw>,
        mono: usize,
        emoji: Vec<gpui::PolychromeSprite>,
        custom: usize,
    }
    struct Recorder {
        inner: Box<dyn PlatformHeadlessRenderer>,
        captured: Arc<Mutex<Captured>>,
    }
    impl PlatformHeadlessRenderer for Recorder {
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.inner.sprite_atlas()
        }
        fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> Result<()> {
            self.inner.render_scene(scene, size)
        }
        fn render_scene_to_image(
            &mut self,
            scene: &Scene,
            size: Size<DevicePixels>,
        ) -> Result<image::RgbaImage> {
            *self.captured.lock().unwrap() = Captured {
                shaders: scene.shader_draws.clone(),
                mono: scene.monochrome_sprites.len(),
                emoji: scene.polychrome_sprites.clone(),
                custom: scene.custom_draws.len(),
            };
            self.inner.render_scene_to_image(scene, size)
        }
    }
    struct CachedText {
        renders: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl Render for CachedText {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            self.renders
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            div().child("Cached text")
        }
    }
    struct View {
        cached: Entity<CachedText>,
        brush: Brush,
    }
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(black())
                .text_color(white())
                .font_family(".SystemUIFont")
                .text_size(px(20.))
                .flex()
                .flex_col()
                .child(
                    crate::painted()
                        .w(px(128.))
                        .h(px(100.))
                        .text_brush(Brush::linear(90., stops()).unwrap())
                        .child("Wrapped brushed text 🙂")
                        .child(
                            crate::painted()
                                .w(px(100.))
                                .h(px(24.))
                                .text_brush(Brush::solid([0., 1., 0., 1.]).unwrap())
                                .child("Nested"),
                        ),
                )
                .child(
                    crate::painted()
                        .w(px(128.))
                        .h(px(32.))
                        .text_brush(Brush::solid([0.125, 0.75, 0.25, 1.]).unwrap())
                        .child(deferred("Deferred")),
                )
                .child("Ordinary 🙂")
                .child(
                    crate::painted()
                        .w(px(128.))
                        .h(px(24.))
                        .text_brush(self.brush.clone())
                        .child(
                            self.cached
                                .clone()
                                .cached(StyleRefinement::default().w(px(128.)).h(px(24.))),
                        ),
                )
        }
    }
    for native in [false, true] {
        let captured = Arc::new(Mutex::new(Captured::default()));
        let output = captured.clone();
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(()),
            move || {
                let inner: Box<dyn PlatformHeadlessRenderer> = if native {
                    Box::new(gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
                } else {
                    Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)
                };
                Ok(Some(Box::new(Recorder {
                    inner,
                    captured: output.clone(),
                })))
            },
        );
        let renders = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = renders.clone();
        let window = cx
            .open_window(size(px(256.), px(240.)), |_, cx| {
                cx.new(|cx| View {
                    cached: cx.new(|_| CachedText { renders: count }),
                    brush: Brush::solid([0.6, 0.1, 0.3, 1.]).unwrap(),
                })
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let scale = cx
            .update_window(window.into(), |_, window, _| window.scale_factor())
            .unwrap();
        let image = cx.capture_screenshot(window.into()).unwrap();
        assert!(
            image
                .enumerate_pixels()
                .any(|(x, y, p)| x < (128. * scale) as u32
                    && y < (90. * scale) as u32
                    && (p[0].abs_diff(p[1]) > 8 || p[2].abs_diff(p[1]) > 8)),
            "brushed glyphs must render visible color"
        );
        let result = captured.lock().unwrap();
        assert!(
            result.shaders.len() > 10,
            "real glyphs must use shader draws"
        );
        assert!(result.shaders.iter().all(|s| s.glyph_mask.is_some()));
        assert!(
            result.shaders.iter().any(|s| s.data[0] == [0., 1., 0., 1.]),
            "nested override"
        );
        assert!(
            result
                .shaders
                .iter()
                .any(|s| s.data[0] == [0.125, 0.75, 0.25, 1.]),
            "deferred scope must retain its brush"
        );
        assert!(result.mono > 0, "ordinary sibling text must remain native");
        assert!(
            result
                .emoji
                .iter()
                .any(|s| s.bounds.origin.y.0 < 100. * scale),
            "emoji inside the brushed scope must remain native color sprites"
        );
        assert_eq!(result.custom, 0);
        drop(result);
        let first = renders.load(std::sync::atomic::Ordering::Relaxed);
        window.update(&mut cx, |_, _, cx| cx.notify()).unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        assert_eq!(
            renders.load(std::sync::atomic::Ordering::Relaxed),
            first,
            "unchanged brush snapshots should preserve cached child paint"
        );
        window
            .update(&mut cx, |view, _, cx| {
                view.brush = Brush::solid([1., 0.5, 0., 1.]).unwrap();
                cx.notify();
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let _ = cx.capture_screenshot(window.into()).unwrap();
        assert!(
            renders.load(std::sync::atomic::Ordering::Relaxed) > first,
            "changing inherited brush must invalidate cached children"
        );
        let result = captured.lock().unwrap();
        assert!(
            result
                .shaders
                .iter()
                .any(|s| s.data[0] == [1., 0.5, 0., 1.])
        );
        assert!(
            !result
                .shaders
                .iter()
                .any(|s| s.data[0] == [0.6, 0.1, 0.3, 1.]),
            "cached glyphs must not keep the old material"
        );
    }
}
