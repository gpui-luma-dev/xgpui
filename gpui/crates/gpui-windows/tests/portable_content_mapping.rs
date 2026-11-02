#![cfg(all(target_os = "windows", feature = "test-support"))]
use gpui_windows::Dx11HeadlessRenderer as Renderer;
fn new_renderer() -> Renderer {
    Renderer::new().unwrap()
}
use gpui_windows::content_executor::{
    Dx11ContentLimits as ContentLimits, Dx11ContentTestGate as ContentGate,
};
include!("../../gpui/tests/support/content_mapping_pixels.rs");
