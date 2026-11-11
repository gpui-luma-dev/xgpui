//! Headless Metal tests: a raw-Metal "engine" on its own command queue draws
//! into slots; xgpui's real Metal renderer composites them.
use super::*;
use gpui::{
    ContentMask, CustomDraw, PlatformHeadlessRenderer, ScaledPixels, Scene, WindowColorMode, point,
    size,
};
use gpui_apple::metal_renderer::MetalHeadlessRenderer;
use objc2_metal::{MTLClearColor, MTLCommandQueue, MTLCreateSystemDefaultDevice};
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
            .present(slot, SyncPoint::new(self.event.clone(), self.next))
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
