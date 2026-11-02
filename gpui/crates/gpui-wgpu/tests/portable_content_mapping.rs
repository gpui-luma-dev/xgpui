#![cfg(all(not(target_family = "wasm"), feature = "test-support"))]
use gpui_wgpu::WgpuHeadlessRenderer as Renderer;
fn new_renderer() -> Renderer {
    Renderer::new().unwrap()
}
use gpui_wgpu::content_executor::{
    WgpuContentLimits as ContentLimits, WgpuContentTestGate as ContentGate,
};
include!("../../gpui/tests/support/content_mapping_pixels.rs");
