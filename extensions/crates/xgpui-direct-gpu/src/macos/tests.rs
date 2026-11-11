//! Headless Metal tests: a raw-Metal "engine" on its own command queue draws
//! into slots; xgpui's real Metal renderer composites them.
use super::*;
use crate::{AcquireError, Transport};
use gpui::{Bounds, CustomContent, DevicePixels, Size};
use gpui::{
    ContentMask, CustomDraw, PlatformHeadlessRenderer, ScaledPixels, Scene, WindowColorMode, point,
    size,
};
use gpui_apple::metal_renderer::MetalHeadlessRenderer;
use objc2_metal::{MTLClearColor, MTLCommandQueue, MTLCreateSystemDefaultDevice};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// An independent engine: its own command queue and shared event.
struct Engine {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn objc2_metal::MTLCommandQueue>>,
    event: Retained<ProtocolObject<dyn MTLSharedEvent>>,
    next: u64,
}
impl Engine {
    fn new() -> Self {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let queue = device.newCommandQueue().expect("engine queue");
        let event = device.newSharedEvent().expect("engine event");
        Self {
            device,
            queue,
            event,
            next: 0,
        }
    }

    /// Clear `slot` to `color` (in the slot's own encoding), signal, present.
    fn draw_and_present(&mut self, surface: &EngineSurface, slot: SurfaceSlot, color: [f64; 4]) {
        let texture = surface_texture(
            &self.device,
            slot.iosurface(),
            &slot.desc(),
            MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead,
        )
        .unwrap();
        let command = self.queue.commandBuffer().expect("engine command buffer");
        let pass = MTLRenderPassDescriptor::new();
        let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        attachment.setTexture(Some(&texture));
        attachment.setLoadAction(MTLLoadAction::Clear);
        attachment.setStoreAction(MTLStoreAction::Store);
        let [red, green, blue, alpha] = color;
        attachment.setClearColor(MTLClearColor {
            red,
            green,
            blue,
            alpha,
        });
        command
            .renderCommandEncoderWithDescriptor(&pass)
            .expect("engine encoder")
            .endEncoding();
        self.next += 1;
        command.encodeSignalEvent_value(ProtocolObject::from_ref(&*self.event), self.next);
        command.commit();
        surface
            // SAFETY: the signal was committed above.
            .present(slot, unsafe {
                SyncPoint::submitted(self.event.clone(), self.next)
            })
            .unwrap();
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

/// A scene placing `content` at twice the slot size.
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
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 2)
}

#[test]
fn bgra_frame_composites_scaled_without_copy() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 3)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        assert!(surface.content().is_none());
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [0.2, 0.6, 0.9, 1.0]);
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        for (x, y) in [(1, 1), (16, 8), (30, 14)] {
            let pixel = image.get_pixel(x, y).0;
            assert!(near(pixel, [51, 153, 230, 255]), "({x},{y}) = {pixel:?}");
        }
        assert_eq!(surface.stats().presents, 1);
        assert_eq!(surface.transport(), Transport::ZeroCopy);
    });
}

#[test]
fn wide_frame_composites_in_wide_window() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        renderer
            .set_color_mode(WindowColorMode::WideGamutSdr)
            .unwrap();
        let slot = surface.acquire().unwrap();
        // Linear neutral 0.5 is identical in P3 and sRGB: encoded ≈ 188.
        engine.draw_and_present(&surface, slot, [0.5, 0.5, 0.5, 1.0]);
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        let pixel = image.get_pixel(16, 8).0;
        assert!(near(pixel, [188, 188, 188, 255]), "{pixel:?}");
    });
}

#[test]
fn retained_snapshot_keeps_its_slot_immutable() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let first = surface.acquire().unwrap();
        let first_index = first.index();
        engine.draw_and_present(&surface, first, [1.0, 0.0, 0.0, 1.0]);
        let old_scene = scene(surface.content().unwrap());
        let second = surface.acquire().unwrap();
        engine.draw_and_present(&surface, second, [0.0, 1.0, 0.0, 1.0]);

        // Slot one is retained by the old scene, slot two is the latest frame.
        assert_eq!(surface.acquire().err(), Some(AcquireError::WouldBlock));
        assert_eq!(surface.stats().would_block, 1);
        // Replaying the old scene still shows the first frame.
        let replay = renderer
            .render_scene_to_image(&old_scene, viewport())
            .unwrap();
        assert!(near(replay.get_pixel(16, 8).0, [255, 0, 0, 255]));
        let latest = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        assert!(near(latest.get_pixel(16, 8).0, [0, 255, 0, 255]));

        drop(old_scene);
        // The renderer also released its in-flight reference after completion.
        let deadline = Instant::now() + Duration::from_secs(2);
        let reacquired = loop {
            if let Ok(slot) = surface.acquire() {
                break slot;
            }
            assert!(Instant::now() < deadline, "slot never returned");
            std::thread::sleep(Duration::from_millis(2));
        };
        assert_eq!(reacquired.index(), first_index);
    });
}

#[test]
fn slot_returns_only_after_gpu_composition_completes() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let first = surface.acquire().unwrap();
        engine.draw_and_present(&surface, first, [1.0, 0.0, 0.0, 1.0]);
        // Submit a composition without waiting, then drop every CPU reference.
        let frame = scene(surface.content().unwrap());
        renderer.render_scene(&frame, viewport()).unwrap();
        drop(frame);
        let second = surface.acquire().unwrap();
        engine.draw_and_present(&surface, second, [0.0, 1.0, 0.0, 1.0]);
        // The first slot comes back once the renderer's completion handler
        // releases its snapshot, without any further frames.
        let deadline = Instant::now() + Duration::from_secs(2);
        while surface.acquire().is_err() {
            assert!(
                Instant::now() < deadline,
                "slot never returned after completion"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    });
}

#[test]
fn present_wakes_and_rejects_foreign_slots() {
    let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
    let other = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
    let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = wakes.clone();
    surface.on_present(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    let engine = Engine::new();
    let foreign = other.acquire().unwrap();
    assert!(
        surface
            .present(foreign, SyncPoint::new(engine.event.clone(), 0))
            .is_err()
    );
    let slot = surface.acquire().unwrap();
    surface
        .present(slot, SyncPoint::new(engine.event.clone(), 0))
        .unwrap();
    assert_eq!(wakes.load(std::sync::atomic::Ordering::Relaxed), 1);
}

/// Fill a BGRA slot with four quadrant colors from the CPU (test only).
fn fill_quadrants(engine: &mut Engine, surface: &EngineSurface, slot: SurfaceSlot) {
    let desc = slot.desc();
    let texture = surface_texture(
        &engine.device,
        slot.iosurface(),
        &desc,
        MTLTextureUsage::ShaderRead,
    )
    .unwrap();
    let (w, h) = (desc.width as usize, desc.height as usize);
    let mut bytes = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            // BGRA: TL red, TR green, BL blue, BR white.
            let pixel = match (x < w / 2, y < h / 2) {
                (true, true) => [0, 0, 255, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [255, 0, 0, 255],
                (false, false) => [255, 255, 255, 255],
            };
            bytes[(y * w + x) * 4..][..4].copy_from_slice(&pixel);
        }
    }
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            objc2_metal::MTLRegion {
                origin: objc2_metal::MTLOrigin { x: 0, y: 0, z: 0 },
                size: objc2_metal::MTLSize {
                    width: w,
                    height: h,
                    depth: 1,
                },
            },
            0,
            std::ptr::NonNull::new(bytes.as_mut_ptr().cast()).unwrap(),
            w * 4,
        );
    }
    engine.next += 1;
    engine.event.setSignaledValue(engine.next);
    surface
        .present(slot, SyncPoint::new(engine.event.clone(), engine.next))
        .unwrap();
}

#[test]
fn placements_of_different_sizes_show_the_whole_slot() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let slot = surface.acquire().unwrap();
        fill_quadrants(&mut engine, &surface, slot);
        let content = surface.content().unwrap();
        let mut scene = Scene::default();
        // Large placement 64x32 at (0,0); small placement 16x8 at (64,0).
        for (x, w, h) in [(0., 64., 32.), (64., 16., 8.)] {
            let bounds = Bounds::new(
                point(ScaledPixels(x), ScaledPixels(0.)),
                size(ScaledPixels(w), ScaledPixels(h)),
            );
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Default::default(),
                opacity: 1.,
                content: content.clone(),
                coverage: None,
            });
        }
        scene.finish();
        let image = renderer
            .render_scene_to_image(&scene, size(DevicePixels(80), DevicePixels(32)))
            .unwrap();
        let red = [255, 0, 0, 255];
        let green = [0, 255, 0, 255];
        let blue = [0, 0, 255, 255];
        let white = [255; 4];
        let mut report = Vec::new();
        for (name, (x0, w, h)) in [("large", (0u32, 64u32, 32u32)), ("small", (64, 16, 8))] {
            for (label, fx, fy, want) in [
                ("TL", 0.25, 0.25, red),
                ("TR", 0.75, 0.25, green),
                ("BL", 0.25, 0.75, blue),
                ("BR", 0.75, 0.75, white),
            ] {
                let x = x0 + (w as f32 * fx) as u32;
                let y = (h as f32 * fy) as u32;
                let got = image.get_pixel(x, y).0;
                if !near(got, want) {
                    report.push(format!(
                        "{name} {label} at ({x},{y}) = {got:?}, want {want:?}"
                    ));
                }
            }
        }
        assert!(report.is_empty(), "{}", report.join("\n"));
    });
}

// Engines present from their own threads; the UI composites on its own.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<EngineSurface>();
    send_sync::<SyncPoint>();
};
#[cfg(feature = "wgpu")]
const _: () = {
    const fn send<T: Send>() {}
    send::<WgpuSurfaceTarget>();
};

#[test]
fn present_from_engine_thread_composites_on_ui_thread() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 3)).unwrap();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let engine_surface = surface.clone();
        std::thread::spawn(move || {
            let mut engine = Engine::new();
            let slot = engine_surface.acquire().unwrap();
            engine.draw_and_present(&engine_surface, slot, [0.0, 0.0, 1.0, 1.0]);
        })
        .join()
        .unwrap();
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        assert!(near(image.get_pixel(16, 8).0, [0, 0, 255, 255]));
    });
}

#[test]
fn rounded_placement_clips_corners() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [1.0, 1.0, 1.0, 1.0]);
        let bounds = Bounds::new(
            point(ScaledPixels(0.), ScaledPixels(0.)),
            size(ScaledPixels(32.), ScaledPixels(16.)),
        );
        let mut rounded = Scene::default();
        rounded.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: gpui::Corners::all(ScaledPixels(6.)),
            opacity: 1.,
            content: surface.content().unwrap(),
            coverage: None,
        });
        rounded.finish();
        let image = renderer
            .render_scene_to_image(&rounded, viewport())
            .unwrap();
        let corner = image.get_pixel(0, 0).0;
        assert!(!near(corner, [255; 4]), "corner not clipped: {corner:?}");
        assert!(near(image.get_pixel(16, 8).0, [255; 4]));
    });
}

/// Losing the engine (#197 K6): new work is rejected, the last frame keeps
/// compositing from the surface's own memory, and a new surface recovers.
#[test]
fn invalidated_surface_keeps_last_frame_and_recovers() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let pending = {
            let mut engine = Engine::new();
            let slot = surface.acquire().unwrap();
            engine.draw_and_present(&surface, slot, [1.0, 0.0, 0.0, 1.0]);
            let image = renderer
                .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
                .unwrap();
            assert!(near(image.get_pixel(16, 8).0, [255, 0, 0, 255]));
            let pending = surface.acquire().unwrap();
            (pending, engine.event.clone())
            // The engine's queue and resources are dropped here.
        };
        surface.invalidate();
        assert!(!surface.is_valid());
        assert_eq!(surface.acquire().err(), Some(AcquireError::Invalidated));
        let (slot, event) = pending;
        assert!(surface.present(slot, SyncPoint::new(event, 0)).is_err());
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        assert!(
            near(image.get_pixel(16, 8).0, [255, 0, 0, 255]),
            "last frame must survive the engine"
        );
        let replacement = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut engine = Engine::new();
        let slot = replacement.acquire().unwrap();
        engine.draw_and_present(&replacement, slot, [0.0, 1.0, 0.0, 1.0]);
        let image = renderer
            .render_scene_to_image(&scene(replacement.content().unwrap()), viewport())
            .unwrap();
        assert!(near(image.get_pixel(16, 8).0, [0, 255, 0, 255]));
    });
}

/// Linear P3 (0.5, 0.25, 0.25) mapped to clipped sRGB and encoded. Unmapped
/// channels would encode as (188, 137, 137).
const P3_PREVIEW: [u8; 4] = [197, 134, 136, 255];

fn near3(actual: [u8; 4], expected: [u8; 4]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 3)
}

/// K2: an RGBA16F surface in a standard (non-wide) window composites as an
/// sRGB preview from the same slot, without a copy; the wide placement is
/// still rejected there.
#[test]
fn wide_frame_previews_in_standard_window() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [0.5, 0.25, 0.25, 1.0]);
        assert!(
            renderer
                .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
                .is_err(),
            "wide placement needs a wide window"
        );
        let image = renderer
            .render_scene_to_image(&scene(surface.preview_content().unwrap()), viewport())
            .unwrap();
        let pixel = image.get_pixel(16, 8).0;
        assert!(near3(pixel, P3_PREVIEW), "{pixel:?}");
    });
}

/// K2: the same frame placed in a wide and a standard window at once.
#[test]
fn one_frame_composites_wide_and_preview() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 2)).unwrap();
        let mut engine = Engine::new();
        let mut standard = MetalHeadlessRenderer::new().unwrap();
        let mut wide = MetalHeadlessRenderer::new().unwrap();
        wide.set_color_mode(WindowColorMode::WideGamutSdr).unwrap();
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [0.5, 0.5, 0.5, 1.0]);
        let wide_scene = scene(surface.content().unwrap());
        let preview_scene = scene(surface.preview_content().unwrap());
        for image in [
            wide.render_scene_to_image(&wide_scene, viewport()).unwrap(),
            standard
                .render_scene_to_image(&preview_scene, viewport())
                .unwrap(),
        ] {
            let pixel = image.get_pixel(16, 8).0;
            assert!(near(pixel, [188, 188, 188, 255]), "{pixel:?}");
        }
        drop((wide_scene, preview_scene));
        let deadline = Instant::now() + Duration::from_secs(2);
        while surface.stats().free < 1 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(surface.stats().free, 1, "both placements released the slot");
    });
}

/// K2: a wgpu engine writes an RGBA16F slot from a compute shader.
#[cfg(feature = "wgpu")]
#[test]
fn compute_written_frame_composites() {
    use crate::{test_engine::compute_fill, wgpu};
    objc2::rc::autoreleasepool(|_| {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter =
            gpui::block_on(instance.request_adapter(&Default::default())).expect("Metal adapter");
        let (device, queue) = gpui::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: WgpuSurfaceTarget::required_features(),
            ..Default::default()
        }))
        .unwrap();
        let mut target =
            WgpuSurfaceTarget::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 2), &device, &queue)
                .unwrap();
        let slot = target.surface().acquire().unwrap();
        let texture = target.texture(&slot).unwrap();
        let commands = compute_fill(&device, &texture, [0.5, 0.25, 0.25, 1.0]);
        target.present(slot, [commands]).unwrap();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let image = renderer
            .render_scene_to_image(
                &scene(target.surface().preview_content().unwrap()),
                viewport(),
            )
            .unwrap();
        let pixel = image.get_pixel(16, 8).0;
        assert!(near3(pixel, P3_PREVIEW), "{pixel:?}");
    });
}

/// Clear `slot` to `color` and present it with an observed point on a
/// separate event that the test signals from the CPU (or never).
fn present_observed(
    engine: &mut Engine,
    surface: &EngineSurface,
    slot: SurfaceSlot,
    color: [f64; 4],
    gate: &Retained<ProtocolObject<dyn MTLSharedEvent>>,
    value: u64,
) {
    let texture = surface_texture(
        &engine.device,
        slot.iosurface(),
        &slot.desc(),
        MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead,
    )
    .unwrap();
    let command = engine.queue.commandBuffer().expect("engine command buffer");
    let pass = MTLRenderPassDescriptor::new();
    let attachment = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
    attachment.setTexture(Some(&texture));
    attachment.setLoadAction(MTLLoadAction::Clear);
    attachment.setStoreAction(MTLStoreAction::Store);
    let [red, green, blue, alpha] = color;
    attachment.setClearColor(MTLClearColor {
        red,
        green,
        blue,
        alpha,
    });
    command
        .renderCommandEncoderWithDescriptor(&pass)
        .expect("engine encoder")
        .endEncoding();
    command.commit();
    command.waitUntilCompleted();
    surface
        .present(slot, SyncPoint::new(gate.clone(), value))
        .unwrap();
}

/// Acceptance (#197): an engine that never signals cannot stall the
/// compositor. Its frames wait off-screen; the last ready frame keeps
/// compositing; the ring fills and backpressures the engine.
#[test]
fn never_signaled_frames_cannot_stall_composition() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 3)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [1.0, 0.0, 0.0, 1.0]);
        let never = engine.device.newSharedEvent().expect("event");
        let mut presented = 0;
        while let Ok(slot) = surface.acquire() {
            present_observed(&mut engine, &surface, slot, [0.0, 0.0, 1.0, 1.0], &never, 1);
            presented += 1;
        }
        assert!(presented >= 1);
        assert_eq!(surface.stats().waiting, presented);
        assert_eq!(surface.acquire().err(), Some(AcquireError::WouldBlock));
        let started = Instant::now();
        for _ in 0..3 {
            let image = renderer
                .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
                .unwrap();
            assert!(near(image.get_pixel(16, 8).0, [255, 0, 0, 255]));
        }
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "composition stalled: {:?}",
            started.elapsed()
        );
    });
}

/// An observed frame appears once its point is reached, the waiter wakes the
/// UI without a new present, and older frames are then released.
#[test]
fn observed_frame_appears_when_reached_and_wakes_the_ui() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 3)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let wakes = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let counter = wakes.clone();
        surface.on_present(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [1.0, 0.0, 0.0, 1.0]);
        let gate = engine.device.newSharedEvent().expect("event");
        let slot = surface.acquire().unwrap();
        present_observed(&mut engine, &surface, slot, [0.0, 1.0, 0.0, 1.0], &gate, 5);
        let presents_woken = wakes.load(std::sync::atomic::Ordering::SeqCst);
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        assert!(
            near(image.get_pixel(16, 8).0, [255, 0, 0, 255]),
            "unreached frame must not show"
        );
        assert_eq!(surface.stats().waiting, 1);
        gate.setSignaledValue(5);
        let deadline = Instant::now() + Duration::from_secs(2);
        while wakes.load(std::sync::atomic::Ordering::SeqCst) == presents_woken {
            assert!(Instant::now() < deadline, "waiter never woke the UI");
            std::thread::sleep(Duration::from_millis(2));
        }
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        assert!(near(image.get_pixel(16, 8).0, [0, 255, 0, 255]));
        let deadline = Instant::now() + Duration::from_secs(2);
        while surface.stats().free < 2 {
            assert!(Instant::now() < deadline, "superseded frame kept its slot");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(surface.stats().waiting, 0);
    });
}

/// A submitted point whose engine work is still running is waited for on
/// the GPU: the frame shows at once, with no CPU wait.
#[test]
fn submitted_frame_is_waited_for_on_the_gpu() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Bgra8Srgb, 2)).unwrap();
        let mut engine = Engine::new();
        let mut renderer = MetalHeadlessRenderer::new().unwrap();
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(
            surface.stats().waiting,
            0,
            "submitted frames never wait off-screen"
        );
        let image = renderer
            .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
            .unwrap();
        assert!(near(image.get_pixel(16, 8).0, [0, 0, 255, 255]));
    });
}

/// #197 K6: the compositor is replaced (new renderer and queue). Retained
/// scenes and new frames composite on the replacement.
#[test]
fn compositor_replacement_keeps_compositing() {
    objc2::rc::autoreleasepool(|_| {
        let surface = EngineSurface::new(desc(SurfaceFormat::Rgba16FloatLinearP3, 3)).unwrap();
        let mut engine = Engine::new();
        let mut first = MetalHeadlessRenderer::new().unwrap();
        first.set_color_mode(WindowColorMode::WideGamutSdr).unwrap();
        let slot = surface.acquire().unwrap();
        engine.draw_and_present(&surface, slot, [0.5, 0.5, 0.5, 1.0]);
        let retained = scene(surface.content().unwrap());
        let image = first.render_scene_to_image(&retained, viewport()).unwrap();
        assert!(near(image.get_pixel(16, 8).0, [188, 188, 188, 255]));
        drop(first);
        let mut second = MetalHeadlessRenderer::new().unwrap();
        second
            .set_color_mode(WindowColorMode::WideGamutSdr)
            .unwrap();
        let image = second.render_scene_to_image(&retained, viewport()).unwrap();
        assert!(near(image.get_pixel(16, 8).0, [188, 188, 188, 255]));
        drop(retained);
        for i in 0..4 {
            let slot = surface.acquire().unwrap();
            let v = if i % 2 == 0 { 1.0 } else { 0.0 };
            engine.draw_and_present(&surface, slot, [v, v, v, 1.0]);
            let image = second
                .render_scene_to_image(&scene(surface.content().unwrap()), viewport())
                .unwrap();
            let e = if i % 2 == 0 { 255 } else { 0 };
            assert!(near(image.get_pixel(16, 8).0, [e, e, e, 255]), "frame {i}");
        }
    });
}
