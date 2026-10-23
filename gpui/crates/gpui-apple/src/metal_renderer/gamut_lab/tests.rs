use super::*;
use gpui::{CustomContent, CustomDraw, Quad, rgb, rgba};
use std::{any::Any, sync::Arc};

#[derive(Debug)]
struct Patch {
    color: [f64; 4],
    tag: [u32; 4],
    format: MTLPixelFormat,
    pending: Arc<Mutex<Vec<metal::CommandBuffer>>>,
}
impl Patch {
    fn new(color: [f64; 4]) -> Self {
        Self {
            color,
            tag: WIDE_TAG,
            format: MTLPixelFormat::RGBA16Float,
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }
}
impl CustomContent for Patch {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let c = backend
            .downcast_ref::<crate::custom_draw::MetalCustomContext>()
            .unwrap();
        let d = metal::TextureDescriptor::new();
        d.set_width(extent.width.0 as u64);
        d.set_height(extent.height.0 as u64);
        d.set_pixel_format(self.format);
        d.set_storage_mode(metal::MTLStorageMode::Private);
        d.set_usage(metal::MTLTextureUsage::ShaderRead | metal::MTLTextureUsage::RenderTarget);
        let texture = c.device.new_texture(&d);
        let command = c.queue.new_command_buffer();
        let p = metal::RenderPassDescriptor::new();
        let a = p.color_attachments().object_at(0).unwrap();
        a.set_texture(Some(&texture));
        a.set_load_action(metal::MTLLoadAction::Clear);
        a.set_store_action(metal::MTLStoreAction::Store);
        let [r, g, b, alpha] = self.color;
        a.set_clear_color(metal::MTLClearColor::new(
            r * alpha,
            g * alpha,
            b * alpha,
            alpha,
        ));
        command.new_render_command_encoder(p).end_encoding();
        self.pending.lock().push(command.to_owned());
        Ok(Box::new((
            crate::custom_draw::MetalCustomOutput {
                texture,
                commands: vec![command.to_owned()],
            },
            self.tag,
        )))
    }
}
fn renderer() -> MetalRenderer {
    let mut r = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
    r.gamut_lab = Some(Prototype::new(&r.device).unwrap());
    r
}
fn b(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
    Bounds::new(
        point(ScaledPixels(x), ScaledPixels(y)),
        size(ScaledPixels(w), ScaledPixels(h)),
    )
}
fn quad(scene: &mut Scene, bounds: Bounds<ScaledPixels>, color: gpui::Rgba) {
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: color.into(),
        ..Default::default()
    });
}
fn draw(bounds: Bounds<ScaledPixels>, patch: Arc<Patch>) -> CustomDraw {
    CustomDraw {
        order: 0,
        bounds,
        content_mask: ContentMask { bounds },
        corner_radii: gpui::Corners::all(ScaledPixels(0.)),
        opacity: 1.,
        content: patch,
        coverage: None,
    }
}
fn half(v: u16) -> f32 {
    let sign = if v & 0x8000 == 0 { 1. } else { -1. };
    let e = (v >> 10) & 31;
    let m = (v & 1023) as f32;
    if e == 0 {
        sign * m * 2f32.powi(-24)
    } else {
        sign * (1. + m / 1024.) * 2f32.powi(e as i32 - 15)
    }
}
fn snapshot(r: &MetalRenderer) -> (metal::Buffer, usize, usize) {
    let t = r.gamut_lab.as_ref().unwrap().target.as_ref().unwrap();
    let stride = (t.width() as usize * 8).div_ceil(256) * 256;
    let output = r.device.new_buffer(
        (stride * t.height() as usize) as u64,
        metal::MTLResourceOptions::StorageModeShared,
    );
    let command = r.command_queue.new_command_buffer();
    let blit = command.new_blit_command_encoder();
    blit.copy_from_texture_to_buffer(
        t,
        0,
        0,
        metal::MTLOrigin { x: 0, y: 0, z: 0 },
        metal::MTLSize {
            width: t.width(),
            height: t.height(),
            depth: 1,
        },
        &output,
        0,
        stride as u64,
        (stride * t.height() as usize) as u64,
        metal::MTLBlitOption::empty(),
    );
    blit.end_encoding();
    command.commit();
    (output, stride, t.height() as usize)
}
fn sample(s: &(metal::Buffer, usize, usize), x: usize, y: usize) -> [f32; 4] {
    let words = unsafe { slice::from_raw_parts(s.0.contents().cast::<u16>(), s.1 * s.2 / 2) };
    std::array::from_fn(|i| half(words[y * s.1 / 2 + x * 4 + i]))
}
fn wait(r: &MetalRenderer) {
    let c = r.command_queue.new_command_buffer();
    c.commit();
    c.wait_until_completed();
    assert_ne!(c.status(), metal::MTLCommandBufferStatus::Error);
}
fn close(actual: [f32; 4], expected: [f32; 4], epsilon: f32) {
    assert!(
        actual
            .into_iter()
            .zip(expected)
            .all(|(a, e)| a.is_finite() && (a - e).abs() <= epsilon),
        "{actual:?} != {expected:?}"
    );
}
fn decode(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[test]
fn gamut_lab_interleaved_ui_wide_order_and_queued_scratch_reuse() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let mut snapshots = Vec::new();
        for frame in 0..12 {
            let mut s = Scene::default();
            quad(&mut s, b(0., 0., 32., 24.), rgb(0x202020));
            let first = if frame % 2 == 0 {
                [1., 0., 0., 1.]
            } else {
                [0., 0., 1., 1.]
            };
            s.insert_primitive(draw(b(2., 2., 20., 16.), Arc::new(Patch::new(first))));
            quad(&mut s, b(6., 6., 20., 6.), rgba(0xffffff80));
            s.insert_primitive(draw(
                b(12., 9., 16., 12.),
                Arc::new(Patch::new([0., 1., 0., 0.5])),
            ));
            quad(&mut s, b(18., 13., 8., 3.), rgb(0x808080));
            s.finish();
            r.render_scene(&s, size(DevicePixels(32), DevicePixels(24)))?;
            snapshots.push((snapshot(&r), first));
        }
        wait(&r);
        for (shot, first) in snapshots {
            close(sample(&shot, 3, 3), first.map(|v| v as f32), 0.001);
            let a = 128. / 255.;
            let under = first.map(|v| v as f32 * (1. - a) + a);
            close(
                sample(&shot, 7, 7),
                [under[0], under[1], under[2], 1.],
                0.002,
            );
            close(
                sample(&shot, 13, 10),
                [under[0] * 0.5, under[1] * 0.5 + 0.5, under[2] * 0.5, 1.],
                0.002,
            );
            let gray = decode(128. / 255.);
            close(sample(&shot, 20, 14), [gray, gray, gray, 1.], 0.001);
            let gray = decode(32. / 255.);
            close(sample(&shot, 0, 0), [gray, gray, gray, 1.], 0.001);
        }
        Ok(())
    })
}

#[test]
fn gamut_lab_opaque_ui_matches_legacy_and_float_target_keeps_p3() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let mut s = Scene::default();
        quad(&mut s, b(0., 0., 64., 32.), rgb(0x172554));
        for i in 0..8 {
            quad(
                &mut s,
                b(i as f32 * 8., 0., 8., 16.),
                rgba(0xffffff00 | ((i * 31) as u32)),
            );
        }
        s.finish();
        let wide = r.render_scene_to_image(&s, size(DevicePixels(64), DevicePixels(32)))?;
        r.gamut_lab = None;
        let legacy = r.render_scene_to_image(&s, size(DevicePixels(64), DevicePixels(32)))?;
        for (a, b) in wide.pixels().zip(legacy.pixels()) {
            assert!(a.0.iter().zip(b.0).all(|(a, b)| a.abs_diff(b) <= 1));
        }
        r.gamut_lab = Some(Prototype::new(&r.device)?);
        let mut s = Scene::default();
        s.insert_primitive(draw(
            b(0., 0., 8., 8.),
            Arc::new(Patch::new([1., 0., 0., 1.])),
        ));
        s.finish();
        let preview = r.render_scene_to_image(&s, size(DevicePixels(8), DevicePixels(8)))?;
        let raw = snapshot(&r);
        wait(&r);
        close(sample(&raw, 4, 4), [1., 0., 0., 1.], 0.0001);
        // P3 red has a negative linear sRGB green component: it is outside sRGB.
        assert!(-0.042056955 * sample(&raw, 4, 4)[0] < 0.);
        assert_eq!(preview.get_pixel(4, 4).0, [255, 0, 0, 255]);
        Ok(())
    })
}

#[test]
fn gamut_lab_low_alpha_clip_rounded_edges_and_opacity() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        for bg in [0x101010, 0xf0f0f0] {
            let neutral = decode((bg & 255) as f32 / 255.);
            for alpha in [0., 1e-5, 0.0005, 0.02, 0.5, 1.] {
                for color in [
                    [1., 0., 0., alpha],
                    [0., 1., 0., alpha],
                    [0., 0., 1., alpha],
                ] {
                    let mut s = Scene::default();
                    quad(&mut s, b(0., 0., 16., 16.), rgb(bg));
                    let mut d = draw(b(2., 2., 12., 12.), Arc::new(Patch::new(color)));
                    d.opacity = 0.35;
                    d.corner_radii = gpui::Corners::all(ScaledPixels(4.));
                    d.content_mask.bounds = b(4., 0., 12., 16.);
                    s.insert_primitive(d);
                    s.finish();
                    r.render_scene(&s, size(DevicePixels(16), DevicePixels(16)))?;
                    let shot = snapshot(&r);
                    wait(&r);
                    for y in 0..16 {
                        for x in 0..16 {
                            let lx = x as f32 + 0.5 - 2.;
                            let ly = y as f32 + 0.5 - 2.;
                            let qx = (lx - 6.).abs() - 2.;
                            let qy = (ly - 6.).abs() - 2.;
                            let dist = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - 4.;
                            let inside = (4..14).contains(&x) && (2..14).contains(&y);
                            let coverage = if inside {
                                (0.5 - dist).clamp(0., 1.)
                            } else {
                                0.
                            };
                            let a = alpha as f32 * 0.35 * coverage;
                            let expected = std::array::from_fn(|i| {
                                if i == 3 {
                                    1.
                                } else {
                                    color[i] as f32 * a + neutral * (1. - a)
                                }
                            });
                            close(sample(&shot, x, y), expected, 0.002);
                        }
                    }
                }
            }
        }
        Ok(())
    })
}

#[test]
fn gamut_lab_invalid_contract_discards_pending_commands_and_recovers() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let patch = Arc::new(Patch::new([1., 0., 0., 1.]));
        let mut bad = Patch::new([0., 1., 0., 1.]);
        bad.tag[1] = 99;
        let mut s = Scene::default();
        s.insert_primitive(draw(b(0., 0., 8., 8.), patch.clone()));
        s.insert_primitive(draw(b(8., 0., 8., 8.), Arc::new(bad)));
        s.finish();
        assert!(
            r.render_scene(&s, size(DevicePixels(16), DevicePixels(8)))
                .is_err()
        );
        assert_eq!(
            patch.pending.lock()[0].status(),
            metal::MTLCommandBufferStatus::NotEnqueued
        );
        let mut healthy = Scene::default();
        healthy.insert_primitive(draw(b(0., 0., 8., 8.), patch));
        healthy.finish();
        r.render_scene(&healthy, size(DevicePixels(8), DevicePixels(8)))?;
        let shot = snapshot(&r);
        wait(&r);
        close(sample(&shot, 4, 4), [1., 0., 0., 1.], 0.001);
        Ok(())
    })
}

#[test]
fn gamut_lab_masks_paths_resize_and_target_budget() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        for extent in [(24, 16), (40, 24), (24, 16)] {
            let mut s = Scene::default();
            quad(
                &mut s,
                b(0., 0., extent.0 as f32, extent.1 as f32),
                rgb(0x101010),
            );
            let mut d = draw(b(2., 2., 16., 12.), Arc::new(Patch::new([1., 0., 0., 1.])));
            d.coverage = Some(gpui::CustomCoverage::Border {
                widths: gpui::Edges::all(ScaledPixels(2.)),
                style: gpui::BorderStyle::Solid,
            });
            s.insert_primitive(d);
            let mut p = gpui::PathBuilder::fill();
            p.move_to(point(gpui::px(4.), gpui::px(4.)));
            p.line_to(point(gpui::px(8.), gpui::px(4.)));
            p.line_to(point(gpui::px(4.), gpui::px(8.)));
            p.close();
            let mut path = p.build()?.scale(1.);
            path.color = rgb(0x00ff00).into();
            path.content_mask = ContentMask {
                bounds: b(0., 0., 24., 16.),
            };
            s.insert_primitive(path);
            s.finish();
            r.render_scene(&s, size(DevicePixels(extent.0), DevicePixels(extent.1)))?;
            let shot = snapshot(&r);
            wait(&r);
            close(sample(&shot, 2, 6), [1., 0., 0., 1.], 0.002);
            let n = decode(16. / 255.);
            close(sample(&shot, 12, 8), [n, n, n, 1.], 0.002);
            // A native path follows the wide border in a fresh legacy run.
            let pixel = sample(&shot, 5, 5);
            assert!(pixel[1] > 0.9 && pixel[0] < 0.2);
        }
        let mut lab = r.gamut_lab.take().unwrap();
        assert!(
            lab.ensure_targets(&r, size(DevicePixels(16384), DevicePixels(16384)))
                .is_err()
        );
        assert!(
            lab.ensure_targets(&r, size(DevicePixels(0), DevicePixels(1)))
                .is_err()
        );
        Ok(())
    })
}

#[test]
fn gamut_lab_layer_is_explicit_linear_p3_float16_without_edr() {
    objc2::rc::autoreleasepool(|_| {
        let layer = metal::MetalLayer::new();
        configure_layer(&layer);
        assert_eq!(layer.pixel_format(), MTLPixelFormat::RGBA16Float);
        let native: &objc2_quartz_core::CAMetalLayer = unsafe { &*layer.as_ptr().cast() };
        assert!(!native.wantsExtendedDynamicRangeContent());
        let actual = native.colorspace().unwrap();
        let expected = core_graphics::color_space::CGColorSpace::create_with_name(unsafe {
            core_graphics::color_space::kCGColorSpaceExtendedLinearDisplayP3
        })
        .unwrap();
        assert_ne!(
            unsafe {
                core_foundation::base::CFEqual(
                    ptr::from_ref(&*actual).cast(),
                    expected.as_ptr().cast(),
                )
            },
            0
        );
    });
}

#[test]
fn gamut_lab_explicit_tag_does_not_accept_an_8bit_texture() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let mut patch = Patch::new([1., 0., 0., 1.]);
        patch.format = MTLPixelFormat::BGRA8Unorm;
        let pending = patch.pending.clone();
        let mut scene = Scene::default();
        scene.insert_primitive(draw(b(0., 0., 8., 8.), Arc::new(patch)));
        scene.finish();
        assert!(
            r.render_scene(&scene, size(DevicePixels(8), DevicePixels(8)))
                .is_err()
        );
        assert_eq!(
            pending.lock()[0].status(),
            metal::MTLCommandBufferStatus::NotEnqueued
        );
        Ok(())
    })
}
