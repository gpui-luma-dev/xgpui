use super::*;
use std::sync::Arc;

struct View {
    text: Entity<SelectableText>,
    dark: bool,
    font_size: f32,
}
impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(if self.dark { black() } else { white() })
            .text_color(if self.dark { white() } else { black() })
            .text_size(px(self.font_size))
            .line_height(px(48.))
            .child(self.text.clone())
    }
}

#[::core::prelude::v1::test]
fn metal_wide_typography_glow_and_shadow_keep_canonical_selection() {
    let mut app = HeadlessAppContext::with_platform(
        Arc::new(gpui_apple::AppleTextSystem::new()),
        Arc::new(()),
        || {
            let mut r = gpui_apple::metal_renderer::MetalHeadlessRenderer::new();
            assert_eq!(
                r.set_color_mode(WindowColorMode::WideGamutSdr)?.effective(),
                WindowColorMode::WideGamutSdr
            );
            Ok(Some(Box::new(r)))
        },
    );
    let source = "office cafe\u{301}";
    let window = app
        .open_window(size(px(400.), px(120.)), |_, cx| {
            cx.new(|cx| View {
                text: cx.new(|cx| SelectableText::new(source, cx).unwrap()),
                dark: true,
                font_size: 24.,
            })
        })
        .unwrap();
    let text = window
        .update(&mut app, |view, _, _| view.text.clone())
        .unwrap();
    for (effect, dark, font_size) in [
        (KineticEffect::Static, true, 24.),
        (KineticEffect::Typewriter { progress: 0.6 }, false, 30.),
        (
            KineticEffect::Scramble {
                progress: 0.4,
                seed: 42,
                tick: 9,
            },
            true,
            24.,
        ),
        (
            KineticEffect::Wave {
                amplitude: px(3.),
                phase: 0.7,
                cluster_phase: 0.4,
            },
            false,
            30.,
        ),
    ] {
        window
            .update(&mut app, |view, _, cx| {
                view.dark = dark;
                view.font_size = font_size;
                cx.notify();
            })
            .unwrap();
        for radius in [4., 8., 12.] {
            app.update(|cx| {
                text.update(cx, |text, cx| {
                    text.set_effect(effect, cx).unwrap();
                    text.set_fill(
                        Some(
                            crate::Brush::linear(
                                90.,
                                vec![
                                    crate::BrushStop::new(0., [1., 0.1, 0.3, 1.]),
                                    crate::BrushStop::new(1., [0.1, 0.3, 1., 1.]),
                                ],
                            )
                            .unwrap(),
                        ),
                        cx,
                    )
                    .unwrap();
                    text.set_shadow(
                        Some(TextShadow::glow(rgb(0x20ff80).into(), px(radius)).unwrap()),
                        cx,
                    );
                    text.selection.move_to(0, false);
                    text.selection.move_to(source.len(), true);
                })
            });
            app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let glow = app.capture_screenshot(window.into()).unwrap();
            app.update(|cx| text.update(cx, |text, cx| text.set_shadow(None, cx)));
            app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let plain = app.capture_screenshot(window.into()).unwrap();
            assert!(
                glow.pixels().zip(plain.pixels()).any(|(a, b)| a != b),
                "glow must render for {effect:?}, radius={radius}"
            );
            app.update(|cx| {
                text.update(cx, |text, cx| {
                    assert_eq!(text.text(), source);
                    assert_eq!(text.selected_text(), source);
                    text.set_shadow(
                        Some(
                            TextShadow::new(
                                rgb(0x20ff80).into(),
                                point(px(3.), px(3.)),
                                px(radius),
                            )
                            .unwrap(),
                        ),
                        cx,
                    );
                })
            });
            app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let shadow = app.capture_screenshot(window.into()).unwrap();
            assert!(shadow.pixels().zip(plain.pixels()).any(|(a, b)| a != b));
        }
    }
}
