use super::*;
#[::core::prelude::v1::test]
fn format_policy_and_sdr_white_units() -> Result<()> {
    assert_eq!(format(WindowColorMode::Default), DXGI_FORMAT_B8G8R8A8_UNORM);
    assert_eq!(
        format(WindowColorMode::WideGamutSdr),
        DXGI_FORMAT_R16G16B16A16_FLOAT
    );
    assert_eq!(
        color_space(WindowColorMode::WideGamutSdr),
        DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709
    );
    assert_eq!(white_scale(1000)?, 1.);
    assert_eq!(white_scale(2500)?, 2.5);
    assert!(white_scale(0).is_err());
    Ok(())
}
#[::core::prelude::v1::test]
fn monitor_overlap_uses_largest_area_without_integer_overflow() {
    let window = RECT {
        left: -100,
        top: 0,
        right: 300,
        bottom: 200,
    };
    assert_eq!(
        overlap(
            window,
            RECT {
                left: -500,
                top: 0,
                right: 0,
                bottom: 500
            }
        ),
        20000
    );
    assert_eq!(
        overlap(
            window,
            RECT {
                left: 0,
                top: 0,
                right: 500,
                bottom: 500
            }
        ),
        60000
    );
    assert_eq!(
        overlap(
            window,
            RECT {
                left: 400,
                top: 0,
                right: 500,
                bottom: 500
            }
        ),
        0
    );
}

#[::core::prelude::v1::test]
fn uncertain_display_white_defers_wide_presentation_without_changing_intent() -> Result<()> {
    let mut renderer = Dx11HeadlessRenderer::new()?;
    renderer.0.set_color_mode(
        WindowColorMode::WideGamutSdr,
        WindowBackgroundAppearance::Opaque,
    )?;
    renderer.0.color_display.valid = false;
    let error = renderer
        .0
        .draw(&Scene::default(), WindowBackgroundAppearance::Opaque)
        .unwrap_err();
    assert!(error.to_string().contains("display color configuration"));
    assert_eq!(
        renderer.0.color_status().requested(),
        WindowColorMode::WideGamutSdr
    );
    renderer
        .0
        .set_color_mode(WindowColorMode::Default, WindowBackgroundAppearance::Opaque)?;
    assert!(renderer.0.ensure_color_ready().is_ok());
    Ok(())
}

// Real flip-chain and DirectComposition validation needs an interactive Windows
// desktop. WARP pixel tests above/beside this module do not require one.
struct NativeWindow(HWND);
impl NativeWindow {
    fn new() -> Result<Self> {
        use windows::{Win32::UI::WindowsAndMessaging::*, core::w};
        Ok(Self(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("GPUI wide SDR test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                320,
                240,
                None,
                None,
                None,
                None,
            )?
        }))
    }
}
impl Drop for NativeWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(self.0);
        }
    }
}
struct ComApartment;
impl ComApartment {
    fn new() -> Result<Self> {
        unsafe {
            windows::Win32::System::Com::CoInitializeEx(
                None,
                windows::Win32::System::Com::COINIT_MULTITHREADED,
            )
            .ok()?;
        }
        Ok(Self)
    }
}
impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            windows::Win32::System::Com::CoUninitialize();
        }
    }
}
fn warp_devices() -> Result<DirectXDevices> {
    let renderer = Dx11HeadlessRenderer::new()?;
    let devices = renderer.0.devices.as_ref().unwrap();
    Ok(DirectXDevices {
        adapter: devices.adapter.clone(),
        dxgi_factory: devices.dxgi_factory.clone(),
        device: devices.device.clone(),
        device_context: devices.device_context.clone(),
    })
}
fn chain_format(renderer: &DirectXRenderer) -> Result<DXGI_FORMAT> {
    Ok(unsafe {
        renderer
            .resources
            .as_ref()
            .unwrap()
            .swap_chain
            .as_ref()
            .unwrap()
            .GetDesc1()?
    }
    .Format)
}
#[::core::prelude::v1::test]
#[ignore = "requires an interactive Windows desktop; run with --ignored --test-threads=1"]
fn native_swap_chains_toggle_resize_isolate_and_restore_after_device_replacement() -> Result<()> {
    let _com = ComApartment::new()?;
    for disable_composition in [false, true] {
        let a = NativeWindow::new()?;
        let b = NativeWindow::new()?;
        let devices = warp_devices()?;
        let mut first = DirectXRenderer::new(a.0, &devices, disable_composition)?;
        let second = DirectXRenderer::new(b.0, &devices, disable_composition)?;
        first.resize(size(DevicePixels(32), DevicePixels(24)))?;
        first.set_color_mode(
            WindowColorMode::WideGamutSdr,
            WindowBackgroundAppearance::Opaque,
        )?;
        assert_eq!(chain_format(&first)?, DXGI_FORMAT_R16G16B16A16_FLOAT);
        assert_eq!(chain_format(&second)?, DXGI_FORMAT_B8G8R8A8_UNORM);
        assert_eq!(second.color_status().effective(), WindowColorMode::Default);
        let pipeline = first.wide_sdr.as_ref().unwrap().fragment_identity();
        first.set_color_mode(
            WindowColorMode::WideGamutSdr,
            WindowBackgroundAppearance::Opaque,
        )?;
        assert_eq!(
            first.wide_sdr.as_ref().unwrap().fragment_identity(),
            pipeline
        );
        assert!(
            first
                .set_color_mode(
                    WindowColorMode::WideGamutSdr,
                    WindowBackgroundAppearance::Transparent
                )
                .is_err()
        );
        first.draw(&Scene::default(), WindowBackgroundAppearance::Opaque)?;
        first.resize(size(DevicePixels(64), DevicePixels(48)))?;
        assert_eq!(chain_format(&first)?, DXGI_FORMAT_R16G16B16A16_FLOAT);
        first.draw(&Scene::default(), WindowBackgroundAppearance::Opaque)?;
        let replacement = warp_devices()?;
        first.handle_device_lost(&replacement)?;
        first.mark_drawable();
        assert_eq!(
            first.color_status().requested(),
            WindowColorMode::WideGamutSdr
        );
        assert_eq!(chain_format(&first)?, DXGI_FORMAT_R16G16B16A16_FLOAT);
        first.draw(&Scene::default(), WindowBackgroundAppearance::Opaque)?;
        first.set_color_mode(WindowColorMode::Default, WindowBackgroundAppearance::Opaque)?;
        assert_eq!(chain_format(&first)?, DXGI_FORMAT_B8G8R8A8_UNORM);
        first.draw(&Scene::default(), WindowBackgroundAppearance::Opaque)?;
    }
    Ok(())
}
#[::core::prelude::v1::test]
#[ignore = "requires an interactive Windows desktop; run with --ignored --test-threads=1"]
fn native_retained_back_buffer_rejects_transition_then_recovery_restores_rendering() -> Result<()> {
    let _com = ComApartment::new()?;
    for disable_composition in [false, true] {
        let window = NativeWindow::new()?;
        let devices = warp_devices()?;
        let mut renderer = DirectXRenderer::new(window.0, &devices, disable_composition)?;
        let retained = renderer.resources.as_ref().unwrap().render_target.clone();
        assert!(
            renderer
                .set_color_mode(
                    WindowColorMode::WideGamutSdr,
                    WindowBackgroundAppearance::Opaque
                )
                .is_err()
        );
        assert_eq!(
            renderer.color_status().requested(),
            WindowColorMode::Default
        );
        assert!(renderer.needs_color_recovery());
        drop(retained);
        renderer.handle_device_lost(&warp_devices()?)?;
        renderer.mark_drawable();
        renderer.set_color_mode(
            WindowColorMode::WideGamutSdr,
            WindowBackgroundAppearance::Opaque,
        )?;
        renderer.draw(&Scene::default(), WindowBackgroundAppearance::Opaque)?;
    }
    Ok(())
}
#[::core::prelude::v1::test]
fn display_name_matching_ignores_case_and_trailing_storage() {
    let mut a = [123u16; 32];
    let mut b = [456u16; 32];
    for (i, c) in "\\\\.\\DISPLAY1\0".encode_utf16().enumerate() {
        a[i] = c;
    }
    for (i, c) in "\\\\.\\display1\0".encode_utf16().enumerate() {
        b[i] = c;
    }
    assert!(same_device_name(&a, &b));
    b[0] = 0;
    assert!(!same_device_name(&a, &b));
}
#[::core::prelude::v1::test]
fn color_transitions_share_the_cross_window_draw_guard() {
    let coordinator = crate::events::DrawCoordinator::new();
    let first = coordinator.try_begin_renderer_update().unwrap();
    assert!(coordinator.try_begin_renderer_update().is_none());
    drop(first);
    assert!(coordinator.try_begin_renderer_update().is_some());
}
