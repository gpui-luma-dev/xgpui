//! Windows tests: a wgpu engine on its own D3D12 device renders into shared
//! slots; xgpui's D3D11 renderer on the same hardware adapter composites them.
use super::*;
use crate::AcquireError;
use gpui::{
    Bounds, ContentMask, CustomContent, CustomDraw, DevicePixels, PlatformHeadlessRenderer,
    ScaledPixels, Scene, Size, point, size,
};
use gpui_windows::Dx11HeadlessRenderer;
use windows::Win32::Graphics::Dxgi::IDXGIAdapter1;

fn report(line: impl std::fmt::Display) {
    eprintln!("windows engine surface: {line}");
}

/// Engine device and a host renderer on the same hardware adapter, or `None`.
fn setup() -> Option<(wgpu::Device, wgpu::Queue, Dx11HeadlessRenderer)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        flags: wgpu::InstanceFlags::default(),
        backend_options: wgpu::BackendOptions::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    });
    let adapters = gpui::block_on(instance.enumerate_adapters(wgpu::Backends::DX12));
    let Some(adapter) = adapters
        .into_iter()
        .find(|adapter| adapter.get_info().device_type != wgpu::DeviceType::Cpu)
    else {
        report("skipped: no hardware D3D12 adapter");
        return None;
    };
    report(format!("adapter {:?}", adapter.get_info().name));
    let dxgi: IDXGIAdapter1 = unsafe { adapter.as_hal::<Dx12>() }?.as_raw().cast().ok()?;
    let host = match Dx11HeadlessRenderer::with_adapter(&dxgi) {
        Ok(host) => host,
        Err(error) => {
            report(format!(
                "skipped: hardware D3D11 renderer unavailable ({error})"
            ));
            return None;
        }
    };
    let (device, queue) =
        gpui::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).ok()?;
    Some((device, queue, host))
}

fn desc(format: SurfaceFormat, slots: usize) -> SurfaceDesc {
    SurfaceDesc {
        width: 16,
        height: 8,
        format,
        slots,
    }
}

/// Clear the next slot to `color` on the engine and present it.
fn draw(target: &mut WgpuSurfaceTarget, color: wgpu::Color) {
    let slot = target.surface().acquire().expect("free slot");
    let texture = target.texture(&slot).unwrap();
    let view = texture.create_view(&Default::default());
    let mut encoder = target.device.create_command_encoder(&Default::default());
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("engine_clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(color),
                store: wgpu::StoreOp::Store,
            },
        })],
        ..Default::default()
    });
    target.present(slot, [encoder.finish()]).unwrap();
}

fn scene(content: Arc<dyn CustomContent>) -> Scene {
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(32.), ScaledPixels(16.)),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity: 1.,
        content,
        coverage: None,
    });
    scene.finish();
    scene
}

fn viewport() -> Size<DevicePixels> {
    size(DevicePixels(32), DevicePixels(16))
}

fn near(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 3)
}

#[::core::prelude::v1::test]
fn windows_bgra_frame_composites_scaled_without_copy() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    draw(
        &mut target,
        wgpu::Color {
            r: 0.2,
            g: 0.6,
            b: 0.9,
            a: 1.,
        },
    );
    let image = renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    for (x, y) in [(1, 1), (16, 8), (30, 14)] {
        let pixel = image.get_pixel(x, y).0;
        assert!(near(pixel, [51, 153, 230, 255]), "({x},{y}) = {pixel:?}");
    }
    report("PASS bgra frame composited with exact pixels");
}

#[::core::prelude::v1::test]
fn windows_wide_frame_composites_in_wide_mode() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    if let Err(error) = renderer.set_color_mode(gpui::WindowColorMode::WideGamutSdr) {
        report(format!("skipped: wide mode unavailable ({error})"));
        return;
    }
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 2), &device, &queue)
            .unwrap();
    // Linear neutral 0.5 is identical in P3 and sRGB: encoded ≈ 188.
    draw(
        &mut target,
        wgpu::Color {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.,
        },
    );
    let image = renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    let pixel = image.get_pixel(16, 8).0;
    assert!(near(pixel, [188, 188, 188, 255]), "{pixel:?}");
    report("PASS linear P3 RGBA16F frame composited in wide mode");
}

#[::core::prelude::v1::test]
fn windows_retained_snapshot_keeps_its_slot_immutable() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    draw(&mut target, wgpu::Color::RED);
    let old_scene = scene(target.surface().content().unwrap());
    draw(&mut target, wgpu::Color::GREEN);
    assert_eq!(
        target.surface().acquire().err(),
        Some(AcquireError::WouldBlock)
    );
    let replay = renderer
        .render_scene_to_image(&old_scene, viewport())
        .unwrap();
    assert!(near(replay.get_pixel(16, 8).0, [255, 0, 0, 255]));
    drop(old_scene);
    // D3D11 retires snapshots on a later flushed frame.
    let mut released = false;
    for _ in 0..50 {
        let latest = renderer
            .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
            .unwrap();
        assert!(near(latest.get_pixel(16, 8).0, [0, 255, 0, 255]));
        if target.surface().acquire().is_ok() {
            released = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(released, "slot never returned");
    report("PASS replayed snapshot kept its frame; slot returned afterwards");
}

#[::core::prelude::v1::test]
fn windows_repeated_frames_cycle_the_ring() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    for frame in 0..12 {
        let (color, expected) = match frame % 3 {
            0 => (wgpu::Color::RED, [255, 0, 0, 255]),
            1 => (wgpu::Color::GREEN, [0, 255, 0, 255]),
            _ => (wgpu::Color::BLUE, [0, 0, 255, 255]),
        };
        // Wait for a slot: D3D11 releases snapshots on later flushed frames.
        let mut tries = 0;
        while target.surface().stats().free == 0 && tries < 50 {
            renderer
                .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
                .unwrap();
            tries += 1;
        }
        draw(&mut target, color);
        let image = renderer
            .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
            .unwrap();
        let pixel = image.get_pixel(16, 8).0;
        assert!(near(pixel, expected), "frame {frame}: {pixel:?}");
    }
    assert_eq!(target.surface().stats().presents, 12);
    report("PASS 12 frames through a 3-slot ring, each composited correctly");
}

#[::core::prelude::v1::test]
fn windows_present_from_engine_thread() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    let target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    let surface = target.surface().clone();
    let target = std::thread::spawn(move || {
        let mut target = target;
        draw(&mut target, wgpu::Color::BLUE);
        target
    })
    .join()
    .unwrap();
    let image = renderer
        .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
        .unwrap();
    assert!(near(image.get_pixel(16, 8).0, [0, 0, 255, 255]));
    drop(target);
    report("PASS frame presented from an engine thread");
}

/// The compositor waits on the GPU: the engine's frame is held on a fence that
/// another thread signals after 300 ms, and the composited image must show it.
#[::core::prelude::v1::test]
fn windows_gpu_side_wait_holds_composition_until_engine_signals() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    if !target.gpu_sync() {
        report("skipped: no shared engine fence (CPU-wait mode)");
        return;
    }
    draw(&mut target, wgpu::Color::RED);
    renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    let raw: windows::Win32::Graphics::Direct3D12::ID3D12Device =
        unsafe { device.as_hal::<Dx12>() }
            .unwrap()
            .raw_device()
            .clone();
    let gate: ID3D12Fence = unsafe {
        raw.CreateFence(
            0,
            windows::Win32::Graphics::Direct3D12::D3D12_FENCE_FLAG_NONE,
        )
    }
    .unwrap();
    unsafe { queue.as_hal::<Dx12>() }
        .unwrap()
        .add_wait_fence(gate.clone(), 1);
    draw(&mut target, wgpu::Color::GREEN);
    let content = target.surface().content().unwrap();
    struct Gate(ID3D12Fence);
    // SAFETY: ID3D12Fence is free-threaded; Signal may be called on any thread.
    unsafe impl Send for Gate {}
    let opener_gate = Gate(gate.clone());
    let started = std::time::Instant::now();
    let opener = std::thread::spawn(move || {
        let gate = opener_gate;
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe { gate.0.Signal(1) }.unwrap();
    });
    let image = renderer
        .render_scene_to_image(&scene(content), viewport())
        .unwrap();
    let elapsed = started.elapsed();
    opener.join().unwrap();
    let pixel = image.get_pixel(16, 8).0;
    report(format!(
        "gated frame composited after {elapsed:?}: {pixel:?} (expect green, >= 300 ms)"
    ));
    assert!(
        near(pixel, [0, 255, 0, 255]),
        "composited stale content: {pixel:?}"
    );
    assert!(
        elapsed >= std::time::Duration::from_millis(250),
        "composition did not wait for the engine ({elapsed:?})"
    );
    report("PASS compositor waited on the GPU for the engine's fence");
}

#[::core::prelude::v1::test]
fn windows_reports_sync_mode() {
    let Some((device, queue, _renderer)) = setup() else {
        return;
    };
    let target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    report(format!(
        "sync mode: {}",
        if target.gpu_sync() {
            "GPU-side wait (shared fence)"
        } else {
            "engine waits on CPU"
        }
    ));
}

/// An idle compositor (no further frames) must not starve the engine: the
/// compositor's fence worker releases finished frames (#197 K4).
#[::core::prelude::v1::test]
fn windows_idle_compositor_releases_slots_without_frames() {
    let Some((device, queue, mut renderer)) = setup() else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    draw(&mut target, wgpu::Color::RED);
    // Composite once without reading back, then draw nothing more.
    let frame = scene(target.surface().content().unwrap());
    renderer.render_scene(&frame, viewport()).unwrap();
    drop(frame);
    draw(&mut target, wgpu::Color::GREEN);
    let started = std::time::Instant::now();
    let slot = loop {
        if let Ok(slot) = target.surface().acquire() {
            break slot;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "slot never returned while the compositor was idle"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    drop(slot);
    report(format!(
        "PASS slot returned after {:?} with no further compositor frames",
        started.elapsed()
    ));
}
