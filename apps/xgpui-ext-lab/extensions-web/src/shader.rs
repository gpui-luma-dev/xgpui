//! Portable producer status; no browser-only compatibility shader implementation.
use gpui::{prelude::*, *};
pub struct ShaderDemo;
impl ShaderDemo {
    pub fn new() -> Self {
        Self
    }
}
impl Render for ShaderDemo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().flex().flex_col().gap_4().bg(rgb(0x111827)).text_color(rgb(0xe2e8f0))
   .child(div().text_xl().child("Portable shader content"))
   .child("Gradient, MeshGradient and ShaderPaint now share the portable content API. Browser execution awaits the portable wgpu executor.")
   .child("Use the native paint lab for producer previews. The browser's direct brush and color sample editors remain available.")
    }
}
