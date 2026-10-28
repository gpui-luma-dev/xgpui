use crate::examples::Examples;
use gpui::{prelude::*, *};
use xgpui_shader::{
    instanced::{AnimatedBorder, AnimatedBorderStyle, ShadowPainter, ShadowStop, ShadowStyle},
    painted,
};

#[derive(Clone)]
pub struct ShadowDemo {
    painter: ShadowPainter,
    border: AnimatedBorder,
    pub blur: f32,
    pub spread: f32,
    pub falloff: f32,
    pub offset: f32,
    pub gradient: bool,
    pub inset: bool,
    pub card_dim: bool,
    pub clicks: usize,
}
impl ShadowDemo {
    pub fn new() -> Self {
        Self {
            border: AnimatedBorder::new().expect("border shader"),
            painter: ShadowPainter::new().expect("shadow shader"),
            blur: 24.,
            spread: 0.,
            falloff: 1.,
            offset: 12.,
            gradient: true,
            inset: false,
            card_dim: false,
            clicks: 0,
        }
    }
    pub fn border_button(&self, phase: f32, cx: &mut Context<Examples>) -> impl IntoElement {
        div()
            .id("animated-generate")
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _, cx| {
                this.shadow.clicks += 1;
                cx.notify();
            }))
            .child(
                painted()
                    .rounded(px(999.))
                    .bg(rgb(0xffffff))
                    .animated_border(
                        self.border
                            .layer(AnimatedBorderStyle::default(), phase)
                            .expect("border frame"),
                    )
                    .px_6()
                    .py_3()
                    .text_color(rgb(0x10151f))
                    .child(format!("Generate · {}", self.clicks)),
            )
    }
    fn style(&self) -> ShadowStyle {
        ShadowStyle {
            stops: if self.gradient {
                vec![
                    ShadowStop::new(0., [0.98, 0.16, 0.45, 0.85]),
                    ShadowStop::new(0.35, [0.55, 0.25, 1., 0.8]),
                    ShadowStop::new(0.7, [0.05, 0.6, 1., 0.8]),
                    ShadowStop::new(1., [0.1, 0.85, 0.65, 0.75]),
                ]
            } else {
                vec![ShadowStop::new(0., [0., 0., 0., 0.6])]
            },
            angle: 90.,
            offset: point(px(self.offset * 0.35), px(self.offset)),
            blur_radius: px(self.blur),
            spread_radius: px(self.spread),
            falloff: self.falloff,
            inset: self.inset,
        }
    }
    /// Both cards are real styled elements; no canvas or manual shadow paint calls.
    pub fn render(&self, opacity: f32, cx: &mut Context<Examples>) -> Stateful<Div> {
        let native_color = if self.gradient {
            rgba(0x7160d9b3)
        } else {
            rgba(0x00000099)
        };
        let native = BoxShadow {
            color: native_color.into(),
            offset: point(px(self.offset * 0.35), px(self.offset)),
            blur_radius: px(self.blur),
            spread_radius: px(self.spread),
            inset: self.inset,
        };
        let layer = self
            .painter
            .layer(self.style())
            .expect("validated demo shadow style");
        let card_opacity = if self.card_dim { 0.5 } else { 1. };
        let label = format!("Child clicks: {}", self.clicks);
        let native_button = div()
            .id("native-shadow-child")
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0xdce5f5))
            .cursor_pointer()
            .child(label.clone())
            .on_click(cx.listener(|this, _, _, cx| {
                this.shadow.clicks += 1;
                cx.notify();
            }));
        let helper_button = div()
            .id("helper-shadow-child")
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(0xdce5f5))
            .cursor_pointer()
            .child(label)
            .on_click(cx.listener(|this, _, _, cx| {
                this.shadow.clicks += 1;
                cx.notify();
            }));
        let native_card = div()
            .w_full()
            .max_w(px(240.))
            .h(px(160.))
            .rounded(px(16.))
            .bg(rgb(0xf8fafd))
            .shadow(vec![native])
            .opacity(card_opacity)
            .overflow_hidden()
            .p_4()
            .flex()
            .flex_col()
            .justify_between()
            .text_color(rgb(0x22304a))
            .child(div().child("Standard GPUI"))
            .child(div().text_sm().child("A normal styled div"))
            .child(native_button);
        let helper_card = painted()
            .w_full()
            .max_w(px(240.))
            .h(px(160.))
            .rounded(px(16.))
            .bg(rgb(0xf8fafd))
            .shadow_paint(layer)
            .opacity(card_opacity)
            .overflow_hidden()
            .p_4()
            .flex()
            .flex_col()
            .justify_between()
            .text_color(rgb(0x22304a))
            .child(div().child("Extension helper"))
            .child(div().text_sm().child("painted().shadow_paint(layer)"))
            .child(helper_button);
        div()
            .id("shadow-helper-demo")
            .w_full()
            .flex_1()
            .min_h(px(180.))
            .overflow_hidden()
            .bg(rgb(0xe6ebf3))
            .opacity(opacity)
            .flex()
            .items_center()
            .child(
                div()
                    .w_1_2()
                    .px_8()
                    .flex()
                    .justify_center()
                    .child(native_card),
            )
            .child(
                div()
                    .w_1_2()
                    .px_8()
                    .flex()
                    .justify_center()
                    .child(helper_card),
            )
    }
}
