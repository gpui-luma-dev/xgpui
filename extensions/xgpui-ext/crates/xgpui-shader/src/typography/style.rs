//! Opt-in main-pass fills and bounded, isolated glyph-only shadows.
use anyhow::{Result, ensure};
use gpui::*;
use std::sync::Arc;

/// One bounded shadow behind the emitted monochrome glyphs. Color emoji remain
/// sharp and are not tinted or included in the shadow silhouette.
#[derive(Clone)]
pub struct TextShadow {
    pub(super) offset: Point<Pixels>,
    radius: Pixels,
    brush: Arc<dyn GlyphBrush>,
    filter: crate::LayerEffect,
}
impl TextShadow {
    /// Gaussian support radius in [0,12] logical pixels, offset per axis in [-12,12].
    /// Rendering uses the existing SDR layer filter (at most 64 physical pixels).
    pub fn new(color: Hsla, offset: Point<Pixels>, radius: Pixels) -> Result<Self> {
        ensure!(
            [offset.x, offset.y]
                .into_iter()
                .all(|v| f32::from(v).is_finite() && f32::from(v).abs() <= 12.),
            "text shadow offsets must be finite and within 12 logical pixels"
        );
        ensure!(
            f32::from(radius).is_finite() && (0. ..=12.).contains(&f32::from(radius)),
            "text shadow radius must be in [0,12] logical pixels"
        );
        let rgba = Rgba::from(color);
        let brush = crate::Brush::solid([rgba.r, rgba.g, rgba.b, rgba.a])?.text_brush()?;
        Ok(Self {
            offset,
            radius,
            brush,
            filter: crate::LayerEffect::blur(radius)?
                .with_blur_quality(LayerBlurQuality::FullResolution),
        })
    }
    pub fn glow(color: Hsla, radius: Pixels) -> Result<Self> {
        Self::new(color, point(px(0.), px(0.)), radius)
    }
    pub(super) fn padding(&self) -> Pixels {
        self.radius + self.offset.x.abs().max(self.offset.y.abs()) + px(1.)
    }
    pub(super) fn paint(
        &self,
        window: &mut Window,
        viewport: Bounds<Pixels>,
        content: Bounds<Pixels>,
        commands: &[super::render::GlyphCommand],
    ) {
        // Respect the existing physical filter limit at high device scales.
        let filter = if f32::from(self.radius) * window.scale_factor() > 64. {
            crate::LayerEffect::blur(px(64. / window.scale_factor()))
                .unwrap()
                .with_blur_quality(LayerBlurQuality::FullResolution)
        } else {
            self.filter.clone()
        };
        let draw = |window: &mut Window| {
            let content = Bounds::new(content.origin + self.offset, content.size);
            window.with_content_mask(Some(ContentMask { bounds: content }), |window| {
                window.with_glyph_brush(self.brush.clone(), viewport, |window| {
                    super::render::paint(commands, window, white(), self.offset, true).unwrap();
                });
            });
        };
        let visible = viewport
            .intersect(&window.content_mask().bounds)
            .intersect(&Bounds::new(Point::default(), window.viewport_size()));
        if visible.is_empty() {
            return;
        }
        if self.radius > px(0.)
            && let Some(tiles) = shadow_tiles(visible, window.scale_factor())
        {
            for tile in tiles {
                filter.paint(window, tile, draw);
            }
            return;
        }
        // An oversized footprint must not invalidate the scene. Preserve the
        // sharp offset shadow when it cannot fit the existing capture budget.
        window.with_content_mask(Some(ContentMask { bounds: visible }), draw);
    }
}

/// Mirror Window::layer_capture_regions' worst-case sampling reservation. Tile
/// output edges are aligned to device pixels; full-resolution filtering gives
/// neighbouring captures the same sampling grid and avoids downsample seams.
fn shadow_tiles(bounds: Bounds<Pixels>, scale: f32) -> Option<Vec<Bounds<Pixels>>> {
    let halo = (16. + 8. / scale) * MAX_LAYER_GROUPS as f32;
    let side = (MAX_LAYER_SIDE as f32 - 4.) / scale - 2. * halo;
    if side < 1. {
        return None;
    }
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let step = (side * scale).floor() / scale;
    let nx = (width / step).ceil() as usize;
    let ny = (height / step).ceil() as usize;
    if nx.checked_mul(ny)? > MAX_LAYER_GROUPS {
        return None;
    }
    let mut tiles = Vec::new();
    for y in 0..ny {
        for x in 0..nx {
            let left = if x == 0 {
                f32::from(bounds.left())
            } else {
                ((f32::from(bounds.left()) + x as f32 * step) * scale).round() / scale
            };
            let top = if y == 0 {
                f32::from(bounds.top())
            } else {
                ((f32::from(bounds.top()) + y as f32 * step) * scale).round() / scale
            };
            let right = if x + 1 == nx {
                f32::from(bounds.right())
            } else {
                ((f32::from(bounds.left()) + (x + 1) as f32 * step) * scale).round() / scale
            };
            let bottom = if y + 1 == ny {
                f32::from(bounds.bottom())
            } else {
                ((f32::from(bounds.top()) + (y + 1) as f32 * step) * scale).round() / scale
            };
            tiles.push(Bounds::new(
                point(px(left), px(top)),
                size(px(right - left), px(bottom - top)),
            ));
        }
    }
    Some(tiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn retina_tiles_fit_the_capture_limit_and_oversized_rows_fall_back() {
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(980.), px(90.)));
        for scale in [1., 2., 3.] {
            if let Some(tiles) = shadow_tiles(bounds, scale) {
                assert!(tiles.len() <= MAX_LAYER_GROUPS);
                let halo = (16. + 8. / scale) * MAX_LAYER_GROUPS as f32;
                for tile in &tiles {
                    assert!(
                        (f32::from(tile.size.width) + halo * 2.) * scale + 2.
                            <= MAX_LAYER_SIDE as f32
                    );
                    assert!(
                        (f32::from(tile.size.height) + halo * 2.) * scale + 2.
                            <= MAX_LAYER_SIDE as f32
                    );
                }
                assert_eq!(tiles.last().unwrap().right(), bounds.right());
            }
        }
        assert!(shadow_tiles(bounds, 2.).unwrap().len() > 1);
        assert!(shadow_tiles(bounds, 8.).is_none());
    }
    #[::core::prelude::v1::test]
    fn shadow_rejects_invalid_inputs_and_reserves_the_halo() {
        for radius in [-1., 12.1, f32::NAN, f32::INFINITY] {
            assert!(TextShadow::glow(white(), px(radius)).is_err());
        }
        for offset in [-13., 13., f32::NAN] {
            assert!(TextShadow::new(black(), point(px(offset), px(0.)), px(4.)).is_err());
        }
        let shadow = TextShadow::new(black(), point(px(-4.), px(6.)), px(8.)).unwrap();
        assert_eq!(shadow.padding(), px(15.));
    }
}

#[cfg(all(test, target_os = "macos", feature = "wgpu"))]
mod render_tests {
    use super::*;
    use crate::typography::{KineticEffect, SelectableText};
    use crate::{Brush, BrushStop};
    struct View {
        text: Entity<SelectableText>,
    }
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(white())
                .text_color(black())
                .text_size(px(28.))
                .line_height(px(44.))
                .child(self.text.clone())
        }
    }
    #[::core::prelude::v1::test]
    fn styled_motion_matches_static_and_shadow_never_captures_hidden_selection() {
        for native in [false, true] {
            let mut app = HeadlessAppContext::with_platform(
                Arc::new(gpui_apple::AppleTextSystem::new()),
                Arc::new(()),
                move || {
                    let renderer: Box<dyn PlatformHeadlessRenderer> = if native {
                        Box::new(gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
                    } else {
                        Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)
                    };
                    Ok(Some(renderer))
                },
            );
            let window = app
                .open_window(size(px(1200.), px(180.)), |_, cx| {
                    cx.new(|cx| View {
                        text: cx.new(|cx| SelectableText::new("office café 👩🏽‍💻", cx).unwrap()),
                    })
                })
                .unwrap();
            let text = window
                .update(&mut app, |view, _, _| view.text.clone())
                .unwrap();
            let fill = Brush::linear(
                0.,
                vec![
                    BrushStop::new(0., [0.1, 0.2, 0.9, 1.]),
                    BrushStop::new(1., [0.8, 0.1, 0.5, 1.]),
                ],
            )
            .unwrap();
            let mut capture = |effect, shadow: TextShadow, selected, reduced| {
                app.update(|cx| {
                    cx.set_reduce_motion(reduced);
                    text.update(cx, |text, cx| {
                        text.set_fill(Some(fill.clone()), cx).unwrap();
                        text.set_shadow(Some(shadow), cx);
                        text.set_effect(effect, cx).unwrap();
                        if selected {
                            text.select_all(cx);
                        } else {
                            text.selection.move_to(0, false);
                        }
                        cx.notify();
                    });
                });
                app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                    .unwrap();
                app.capture_screenshot(window.into()).unwrap()
            };
            let shadow =
                TextShadow::new(rgba(0x2040ffb0).into(), point(px(2.), px(4.)), px(8.)).unwrap();
            let reference = capture(KineticEffect::Static, shadow.clone(), false, false);
            for effect in [
                KineticEffect::Typewriter { progress: 1. },
                KineticEffect::Scramble {
                    progress: 1.,
                    seed: 1,
                    tick: 2,
                },
            ] {
                let actual = capture(effect, shadow.clone(), false, false);
                assert!(
                    reference.as_raw() == actual.as_raw(),
                    "styled full resolution must match static pixels"
                );
            }
            let reduced = capture(
                KineticEffect::Typewriter { progress: 0. },
                shadow.clone(),
                false,
                true,
            );
            assert!(
                reference.as_raw() == reduced.as_raw(),
                "reduced motion retains fill and shadow on the full source"
            );
            let blank = capture(
                KineticEffect::Typewriter { progress: 0. },
                shadow.clone(),
                false,
                false,
            );
            let hidden_selected = capture(
                KineticEffect::Typewriter { progress: 0. },
                shadow,
                true,
                false,
            );
            assert!(
                blank.as_raw() == hidden_selected.as_raw(),
                "neither highlight nor shadow may reveal hidden selection"
            );
            let transparent_shadow =
                TextShadow::new(rgba(0x2040ff00).into(), point(px(2.), px(4.)), px(8.)).unwrap();
            let no_halo = capture(KineticEffect::Static, transparent_shadow, false, false);
            assert!(
                reference.as_raw() != no_halo.as_raw(),
                "the shadow must produce real halo pixels"
            );
            app.update(|cx| {
                text.read_with(cx, |text, _| {
                    assert_eq!(text.text(), "office café 👩🏽‍💻");
                    assert_eq!(text.accessible_run.borrow().value(), Some("office café 👩🏽‍💻"));
                })
            });
        }
    }
}
