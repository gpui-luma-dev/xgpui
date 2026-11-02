//! Native pixel/lifetime validation; no visible window is opened.
#[cfg(target_os = "macos")]
mod native {
    include!("../../../../gpui/crates/gpui-apple/tests/support/external_import_pixels.rs");
}
#[cfg(target_os = "windows")]
mod native {
    include!("../../../../gpui/crates/gpui-windows/tests/support/external_import_pixels.rs");
}

#[cfg(target_os = "linux")]
mod native {
    #[test]
    #[ignore = "requires Vulkan DMA-BUF export/import and Khronos validation; run just media-dma-buf-test"]
    fn dma_buf_native_generated_snapshot() -> anyhow::Result<()> {
        gpui_wgpu::custom_draw::dma_buf::native_validation::validate_generated_snapshot()
    }
}
