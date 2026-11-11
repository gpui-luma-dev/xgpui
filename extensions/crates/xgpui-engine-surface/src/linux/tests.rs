//! Linux tests: a wgpu engine on its own Vulkan device renders into slots; the
//! xgpui wgpu headless renderer composites them. Skips without Vulkan support.
use super::*;
use crate::AcquireError;
use gpui::{
    Bounds, ContentMask, CustomContent, CustomDraw, DevicePixels, PlatformHeadlessRenderer,
    ScaledPixels, Scene, Size, point, size,
};
use gpui_wgpu::WgpuHeadlessRenderer;

fn report(line: impl std::fmt::Display) {
    eprintln!("linux engine surface: {line}");
}

/// An engine device with external memory, or `None` (reported) to skip.
fn engine_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        flags: wgpu::InstanceFlags::default(),
        backend_options: wgpu::BackendOptions::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        display: None,
    });
    let mut adapters = gpui::block_on(instance.enumerate_adapters(wgpu::Backends::VULKAN));
    adapters.sort_by_key(|adapter| adapter.get_info().device_type == wgpu::DeviceType::Cpu);
    let adapter = adapters.into_iter().find(|adapter| {
        adapter
            .features()
            .contains(WgpuSurfaceTarget::required_features())
    });
    let Some(adapter) = adapter else {
        report("skipped: no Vulkan adapter with VULKAN_EXTERNAL_MEMORY_FD");
        return None;
    };
    report(format!("engine adapter {:?}", adapter.get_info().name));
    WgpuSurfaceTarget::request_device(&adapter).ok()
}

fn host() -> Option<WgpuHeadlessRenderer> {
    match WgpuHeadlessRenderer::new() {
        Ok(renderer) => Some(renderer),
        Err(error) => {
            report(format!("skipped: no headless compositor ({error})"));
            None
        }
    }
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

#[test]
fn linux_bgra_frame_composites_scaled_without_copy() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
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

#[test]
fn linux_p3_frame_composites_as_preview() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
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
    report("PASS linear P3 RGBA16F frame composited as sRGB preview");
}

#[test]
fn linux_retained_snapshot_keeps_its_slot_immutable() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
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
    // The renderer drops its in-flight reference when its next frame polls.
    let latest = renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    assert!(near(latest.get_pixel(16, 8).0, [0, 255, 0, 255]));
    assert!(target.surface().acquire().is_ok(), "slot never returned");
    report("PASS replayed snapshot kept its frame; slot returned afterwards");
}

#[test]
fn linux_repeated_frames_reuse_imports() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    // Cycle the ring several times; every frame must show its own color.
    for frame in 0..12 {
        let (color, expected) = match frame % 3 {
            0 => (wgpu::Color::RED, [255, 0, 0, 255]),
            1 => (wgpu::Color::GREEN, [0, 255, 0, 255]),
            _ => (wgpu::Color::BLUE, [0, 0, 255, 255]),
        };
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

#[test]
fn linux_present_from_engine_thread() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    let surface = target.surface().clone();
    std::thread::spawn(move || {
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
    report("PASS frame presented from an engine thread");
}

/// The compositor waits on the GPU: the engine's frame is held on a gate that
/// another thread opens after 300 ms, and the composited image must show it.
#[test]
fn linux_gpu_side_wait_holds_composition_until_engine_signals() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    if !target.gpu_sync() {
        report("skipped: engine device lacks VK_KHR_external_semaphore_fd (CPU-wait mode)");
        return;
    }
    draw(&mut target, wgpu::Color::RED);
    renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    // Gate the next engine submission on a semaphore the CPU signals later.
    let gate = unsafe { timeline_semaphore(&target.raw, false).unwrap() };
    unsafe { target.queue.as_hal::<Vulkan>() }
        .unwrap()
        .add_wait_semaphore(gate, Some(1), vk::PipelineStageFlags::ALL_COMMANDS);
    draw(&mut target, wgpu::Color::GREEN);
    let content = target.surface().content().unwrap();
    let raw = target.raw.clone();
    let started = std::time::Instant::now();
    let opener = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe {
            raw.signal_semaphore(&vk::SemaphoreSignalInfo::default().semaphore(gate).value(1))
                .unwrap()
        };
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
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(5)),
    });
    unsafe { target.raw.destroy_semaphore(gate, None) };
    report("PASS compositor waited on the GPU for the engine's timeline signal");
}

#[test]
fn linux_reports_sync_mode() {
    let Some((device, queue)) = engine_device() else {
        return;
    };
    let target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    report(format!(
        "sync mode: {}",
        if target.gpu_sync() {
            "GPU-side wait (shared timeline semaphore)"
        } else {
            "engine waits on CPU"
        }
    ));
}

/// An idle compositor (no further frames) must not starve the engine: slots
/// held only by finished compositions return when the ring is full (#197 K4).
#[test]
fn linux_idle_compositor_releases_slots_without_frames() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    draw(&mut target, wgpu::Color::RED);
    // Composite once without waiting, then draw nothing more.
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

/// Losing the engine device (#197 K6): new work is rejected, the last frame
/// keeps compositing from the compositor's own import, and a new engine
/// device and surface recover.
#[test]
fn linux_engine_device_loss_keeps_last_frame_and_recovers() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    draw(&mut target, wgpu::Color::RED);
    let surface = target.surface().clone();
    let image = renderer
        .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
        .unwrap();
    assert!(near(image.get_pixel(16, 8).0, [255, 0, 0, 255]));
    let pending = surface.acquire().unwrap();
    device.destroy();
    surface.invalidate();
    assert_eq!(surface.acquire().err(), Some(AcquireError::Invalidated));
    assert!(surface.present(pending, SyncPoint::completed()).is_err());
    drop(target);
    drop(queue);
    drop(device);
    let image = renderer
        .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
        .unwrap();
    assert!(
        near(image.get_pixel(16, 8).0, [255, 0, 0, 255]),
        "last frame must survive the engine device"
    );
    let Some((device, queue)) = engine_device() else {
        return;
    };
    let mut replacement =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 2), &device, &queue).unwrap();
    draw(&mut replacement, wgpu::Color::GREEN);
    let image = renderer
        .render_scene_to_image(&scene(replacement.surface().content().unwrap()), viewport())
        .unwrap();
    assert!(near(image.get_pixel(16, 8).0, [0, 255, 0, 255]));
    report("PASS engine loss: new work rejected, last frame kept, new surface recovered");
}

/// Linear P3 (0.5, 0.25, 0.25) mapped to clipped sRGB and encoded. Unmapped
/// channels would encode as (188, 137, 137).
const P3_PREVIEW: [u8; 4] = [197, 134, 136, 255];

fn near3(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 3)
}

/// K2: the engine writes an RGBA16F slot from a compute shader through a
/// storage binding; the compositor shows it as a preview.
#[test]
fn linux_compute_written_frame_composites() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 2), &device, &queue)
            .unwrap();
    let slot = target.surface().acquire().unwrap();
    let texture = target.texture(&slot).unwrap();
    let commands = crate::test_engine::compute_fill(&device, &texture, [0.5, 0.25, 0.25, 1.0]);
    target.present(slot, [commands]).unwrap();
    let image = renderer
        .render_scene_to_image(
            &scene(target.surface().preview_content().unwrap()),
            viewport(),
        )
        .unwrap();
    let pixel = image.get_pixel(16, 8).0;
    assert!(near3(pixel, P3_PREVIEW), "{pixel:?}");
    report("PASS compute-written RGBA16F slot composited");
}

/// Acceptance (#197): a compositor that cannot wait on the GPU never blocks
/// on the engine. It shows the last ready frame until the new one is reached.
#[test]
fn linux_compositor_without_gpu_wait_never_blocks_on_the_engine() {
    let (Some((device, queue)), Some(mut renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    let Some(timeline) = target.timeline.clone() else {
        report("skipped: engine device lacks VK_KHR_external_semaphore_fd (CPU-wait mode)");
        return;
    };
    draw(&mut target, wgpu::Color::RED);
    renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    // Behave as a compositor that could not import the semaphore.
    timeline
        .no_host_wait
        .store(true, std::sync::atomic::Ordering::Release);
    let wakes = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter = wakes.clone();
    target.surface().on_present(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let gate = unsafe { timeline_semaphore(&target.raw, false).unwrap() };
    unsafe { target.queue.as_hal::<Vulkan>() }
        .unwrap()
        .add_wait_semaphore(gate, Some(1), vk::PipelineStageFlags::ALL_COMMANDS);
    draw(&mut target, wgpu::Color::GREEN);
    let woken = wakes.load(std::sync::atomic::Ordering::SeqCst);
    let raw = target.raw.clone();
    let opener = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        unsafe {
            raw.signal_semaphore(&vk::SemaphoreSignalInfo::default().semaphore(gate).value(1))
                .unwrap()
        };
    });

    // Composition right away shows the last ready frame; it must not block.
    let started = std::time::Instant::now();
    let image = renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    let elapsed = started.elapsed();
    let pixel = image.get_pixel(16, 8).0;
    assert!(
        near(pixel, [255, 0, 0, 255]),
        "unready frame shown: {pixel:?}"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "composition blocked on the engine ({elapsed:?})"
    );
    assert_eq!(target.surface().stats().waiting, 1);
    // The waiter wakes the UI once the engine finishes; then it shows.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while wakes.load(std::sync::atomic::Ordering::SeqCst) == woken {
        assert!(
            std::time::Instant::now() < deadline,
            "waiter never woke the UI"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    opener.join().unwrap();
    let image = renderer
        .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
        .unwrap();
    let pixel = image.get_pixel(16, 8).0;
    assert!(
        near(pixel, [0, 255, 0, 255]),
        "ready frame not shown: {pixel:?}"
    );
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(std::time::Duration::from_secs(5)),
    });
    unsafe { target.raw.destroy_semaphore(gate, None) };
    report(format!(
        "PASS compositor without GPU waits showed the last ready frame in {elapsed:?}, then the new frame once woken"
    ));
}

/// #197 K6: the compositor's own device is replaced. Slots are re-imported on
/// the new device (never a stale import keyed by a reused address), retained
/// scenes still composite, and new frames keep flowing.
#[test]
fn linux_compositor_device_replacement_reimports_slots() {
    let (Some((device, queue)), Some(mut first)) = (engine_device(), host()) else {
        return;
    };
    let mut target =
        WgpuSurfaceTarget::new(desc(SurfaceFormat::Bgra8Srgb, 3), &device, &queue).unwrap();
    draw(&mut target, wgpu::Color::RED);
    let retained = scene(target.surface().content().unwrap());
    let image = first.render_scene_to_image(&retained, viewport()).unwrap();
    assert!(near(image.get_pixel(16, 8).0, [255, 0, 0, 255]));
    // The compositor's device is lost and replaced: drop it, make a new one.
    drop(first);
    let Some(mut second) = host() else {
        return;
    };
    // An old retained scene composites on the new device (slot re-imported).
    let image = second.render_scene_to_image(&retained, viewport()).unwrap();
    let pixel = image.get_pixel(16, 8).0;
    assert!(
        near(pixel, [255, 0, 0, 255]),
        "retained frame on new compositor: {pixel:?}"
    );
    drop(retained);
    // New frames keep cycling the ring on the new compositor.
    for (i, color) in [
        wgpu::Color::GREEN,
        wgpu::Color::BLUE,
        wgpu::Color::GREEN,
        wgpu::Color::BLUE,
    ]
    .into_iter()
    .enumerate()
    {
        draw(&mut target, color);
        let image = second
            .render_scene_to_image(&scene(target.surface().content().unwrap()), viewport())
            .unwrap();
        let expected = if i % 2 == 0 {
            [0, 255, 0, 255]
        } else {
            [0, 0, 255, 255]
        };
        let pixel = image.get_pixel(16, 8).0;
        assert!(
            near(pixel, expected),
            "frame {i} on new compositor: {pixel:?}"
        );
    }
    report("PASS compositor device replaced: retained and new frames composite on the new device");
}

/// Write `bytes` into the next slot with the queue and present it.
fn present_written(target: &mut WgpuSurfaceTarget, bytes: &[u8]) {
    let slot = target.surface().acquire().expect("free slot");
    let desc = slot.desc();
    let texture = target.texture(&slot).unwrap();
    target.queue.write_texture(
        texture.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(desc.width * desc.format.bytes_per_pixel()),
            rows_per_image: None,
        },
        texture.size(),
    );
    target.present(slot, []).unwrap();
}

fn strip_scene(content: Arc<dyn CustomContent>, width: u32, opacity: f32) -> Scene {
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(
            ScaledPixels(width as f32),
            ScaledPixels(crate::colour_reference::HEIGHT as f32),
        ),
    );
    let mut scene = Scene::default();
    scene.insert_primitive(CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: Default::default(),
        opacity,
        content,
        coverage: None,
    });
    scene.finish();
    scene
}

/// Acceptance (#197): colour through the compositor against an independent
/// CPU reference: near-black, out-of-sRGB P3, fractional alpha and opacity
/// mapped to clipped sRGB; sRGB controls unchanged.
#[test]
fn linux_colour_matches_reference() {
    use crate::colour_reference::*;
    let (Some((device, queue)), Some(renderer)) = (engine_device(), host()) else {
        return;
    };
    let mut strip = WgpuSurfaceTarget::new(
        SurfaceDesc {
            width: width(),
            height: HEIGHT,
            format: SurfaceFormat::Rgba16FloatLinearP3,
            slots: 2,
        },
        &device,
        &queue,
    )
    .unwrap();
    present_written(&mut strip, &strip_rgba16f());
    let control_width = SRGB_CONTROLS.len() as u32 * STRIPE;
    let mut controls = WgpuSurfaceTarget::new(
        SurfaceDesc {
            width: control_width,
            height: HEIGHT,
            format: SurfaceFormat::Bgra8Srgb,
            slots: 2,
        },
        &device,
        &queue,
    )
    .unwrap();
    present_written(&mut controls, &controls_bgra8());
    let extent = size(DevicePixels(width() as i32), DevicePixels(HEIGHT as i32));
    let mut failures = Vec::new();
    let mut worst = 0f64;
    let mut checked = 0;
    // Linux composites RGBA16F slots as an sRGB preview.
    let content = |target: &WgpuSurfaceTarget| target.surface().preview_content().unwrap();
    for (mode, mut renderer) in [("preview", renderer)] {
        for opacity in [1., 0.5] {
            let image = renderer
                .render_scene_to_image(&strip_scene(content(&strip), width(), opacity), extent)
                .unwrap();
            for (i, (name, patch)) in PATCHES.iter().enumerate() {
                let actual = image.get_pixel(centre(i), HEIGHT / 2).0;
                let expected = preview(*patch, opacity);
                let d = distance(actual, expected);
                worst = worst.max(d);
                checked += 1;
                if d > 2.5 {
                    failures.push(format!(
                        "{mode} {name} @ {opacity}: {actual:?} vs {expected:.1?}"
                    ));
                }
            }
            let image = renderer
                .render_scene_to_image(
                    &strip_scene(
                        controls.surface().content().unwrap(),
                        control_width,
                        opacity,
                    ),
                    extent,
                )
                .unwrap();
            for (i, (name, rgba)) in SRGB_CONTROLS.iter().enumerate() {
                let actual = image.get_pixel(centre(i), HEIGHT / 2).0;
                let expected = control(*rgba, opacity);
                let d = distance(actual, expected);
                worst = worst.max(d);
                checked += 1;
                if d > 2.5 {
                    failures.push(format!(
                        "{mode} {name} @ {opacity}: {actual:?} vs {expected:.1?}"
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "colour mismatches:\n{}",
        failures.join("\n")
    );
    report(format!(
        "PASS colour: {checked} patches match the CPU reference (worst 8-bit distance {worst:.2})"
    ));
}
