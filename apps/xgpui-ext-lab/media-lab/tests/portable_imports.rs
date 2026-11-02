//! Native pixel/lifetime validation; no visible window is opened.
#[cfg(target_os = "macos")]
mod native {
    include!("../../../../gpui/crates/gpui-apple/tests/support/external_import_pixels.rs");
}
#[cfg(target_os = "windows")]
mod native {
    include!("../../../../gpui/crates/gpui-windows/tests/support/external_import_pixels.rs");
}
