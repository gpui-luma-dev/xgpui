use super::*;
use crate::metal;
use crate::metal::*;
use gpui::{CustomContent, CustomDraw, Quad, rgb, rgba};
use objc2::{rc::Retained, runtime::ProtocolObject};
use std::{any::Any, sync::Arc};

#[derive(Debug)]
struct Patch {
    color: [f64; 4],
    format: MTLPixelFormat,
    pending: Arc<Mutex<Vec<metal::CommandBuffer>>>,
}
impl Patch {
    fn new(color: [f64; 4]) -> Self {
        Self {
            color,
            format: MTLPixelFormat::RGBA16Float,
            pending: Arc::new(Mutex::new(Vec::new())),
        }
    }
}
// SAFETY: the fixture serializes pending command access through its mutex.
unsafe impl Send for Patch {}
unsafe impl Sync for Patch {}
impl CustomContent for Patch {
    fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let c = backend
            .downcast_ref::<crate::custom_draw::MetalCustomContext>()
            .unwrap();
        let d = metal::TextureDescriptor::new();
        unsafe { d.setWidth(extent.width.0 as usize) };
        unsafe { d.setHeight(extent.height.0 as usize) };
        d.setPixelFormat(self.format);
        d.setStorageMode(metal::MTLStorageMode::Private);
        d.setUsage(metal::MTLTextureUsage::ShaderRead | metal::MTLTextureUsage::RenderTarget);
        let texture = c
            .device
            .newTextureWithDescriptor(&d)
            .expect("Metal texture allocation failed");
        let command = c
            .queue
            .commandBuffer()
            .expect("Metal command buffer allocation failed");
        let p = metal::RenderPassDescriptor::new();
        let a = unsafe { p.colorAttachments().objectAtIndexedSubscript(0) };
        a.setTexture(Some(&texture));
        a.setLoadAction(metal::MTLLoadAction::Clear);
        a.setStoreAction(metal::MTLStoreAction::Store);
        let [r, g, b, alpha] = self.color;
        a.setClearColor(MTLClearColor {
            red: r * alpha,
            green: g * alpha,
            blue: b * alpha,
            alpha: alpha,
        });
        command
            .renderCommandEncoderWithDescriptor(&p)
            .expect("Metal render encoder allocation failed")
            .endEncoding();
        self.pending.lock().push(command.to_owned());
        Ok(Box::new(
            crate::custom_draw::MetalWideSdrOutput::from_linear_p3_premultiplied(
                c,
                texture,
                vec![command.to_owned()],
            )?,
        ))
    }
}
fn renderer() -> MetalRenderer {
    let mut r = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
    r.set_color_mode(gpui::WindowColorMode::WideGamutSdr)
        .unwrap();
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
    let t = r.wide_sdr.as_ref().unwrap().target.as_ref().unwrap();
    let stride = (t.width() as usize * 8).div_ceil(256) * 256;
    let output = r
        .device
        .newBufferWithLength_options(
            (stride * t.height() as usize) as usize,
            metal::MTLResourceOptions::StorageModeShared,
        )
        .expect("Metal buffer allocation failed");
    let command = r
        .command_queue
        .commandBuffer()
        .expect("Metal command buffer allocation failed");
    let blit = command
        .blitCommandEncoder()
        .expect("Metal blit encoder allocation failed");
    unsafe {
        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage_options(t, 0, 0, metal::MTLOrigin { x: 0, y: 0, z: 0 }, metal::MTLSize {
            width: t.width(),
            height: t.height(),
            depth: 1,
        }, &output, 0, stride as usize, (stride * t.height() as usize) as usize, metal::MTLBlitOption::empty())
    };
    blit.endEncoding();
    command.commit();
    (metal::Buffer(output), stride, t.height() as usize)
}
fn sample(s: &(metal::Buffer, usize, usize), x: usize, y: usize) -> [f32; 4] {
    let words =
        unsafe { slice::from_raw_parts(s.0.contents().cast::<u16>().as_ptr(), s.1 * s.2 / 2) };
    std::array::from_fn(|i| half(words[y * s.1 / 2 + x * 4 + i]))
}
fn wait(r: &MetalRenderer) {
    let c = r
        .command_queue
        .commandBuffer()
        .expect("Metal command buffer allocation failed");
    c.commit();
    c.waitUntilCompleted();
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
fn wide_sdr_interleaved_ui_wide_order_and_queued_scratch_reuse() -> Result<()> {
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
fn wide_sdr_opaque_ui_matches_legacy_and_float_target_keeps_p3() -> Result<()> {
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
        r.set_color_mode(gpui::WindowColorMode::Default)?;
        let legacy = r.render_scene_to_image(&s, size(DevicePixels(64), DevicePixels(32)))?;
        for (a, b) in wide.pixels().zip(legacy.pixels()) {
            assert!(a.0.iter().zip(b.0).all(|(a, b)| a.abs_diff(b) <= 1));
        }
        r.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
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
fn wide_sdr_low_alpha_clip_rounded_edges_and_opacity() -> Result<()> {
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
fn wide_sdr_invalid_contract_discards_pending_commands_and_recovers() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let patch = Arc::new(Patch::new([1., 0., 0., 1.]));
        let mut bad = Patch::new([0., 1., 0., 1.]);
        bad.format = MTLPixelFormat::BGRA8Unorm;
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
fn wide_sdr_masks_paths_resize_and_target_budget() -> Result<()> {
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
        let mut lab = r.wide_sdr.take().unwrap();
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
fn wide_sdr_layer_is_explicit_linear_p3_float16_without_edr() {
    objc2::rc::autoreleasepool(|_| {
        let layer = metal::CAMetalLayer::new();
        configure_layer(&layer).unwrap();
        assert_eq!(layer.pixelFormat(), MTLPixelFormat::RGBA16Float);
        let native = &layer;
        assert!(!native.wantsExtendedDynamicRangeContent());
        let actual = native.colorspace().unwrap();
        let expected = objc2_core_graphics::CGColorSpace::with_name(Some(unsafe {
            objc2_core_graphics::kCGColorSpaceExtendedLinearDisplayP3
        }))
        .unwrap();
        assert_ne!(
            unsafe {
                core_foundation::base::CFEqual(
                    ptr::from_ref(&*actual).cast(),
                    ptr::from_ref(&*expected).cast(),
                )
            },
            0
        );
    });
}

#[test]
fn wide_sdr_typed_output_rejects_an_8bit_texture() -> Result<()> {
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

#[test]
fn wide_sdr_renderer_isolation_mode_switches_and_queued_resources() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let pool = Arc::new(Mutex::new(Default::default()));
        let mut a = MetalRenderer::new(pool.clone(), false);
        let mut b_renderer = MetalRenderer::new(pool, false);
        assert_eq!(a.color_status().requested(), gpui::WindowColorMode::Default);
        a.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
        let pipeline = a.wide_sdr.as_ref().unwrap().compose.as_ptr();
        a.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
        assert_eq!(pipeline, a.wide_sdr.as_ref().unwrap().compose.as_ptr());
        assert_eq!(
            a.layer().unwrap().pixelFormat(),
            MTLPixelFormat::RGBA16Float
        );
        assert_eq!(
            b_renderer.layer().unwrap().pixelFormat(),
            MTLPixelFormat::BGRA8Unorm
        );
        let mut wide = Scene::default();
        wide.insert_primitive(draw(
            b(0., 0., 8., 8.),
            Arc::new(Patch::new([1., 0., 0., 1.])),
        ));
        wide.finish();
        assert!(
            b_renderer
                .render_scene(&wide, size(DevicePixels(8), DevicePixels(8)))
                .is_err()
        );
        a.render_scene(&wide, size(DevicePixels(8), DevicePixels(8)))?;
        let saved = snapshot(&a); // GPU copy queued, not CPU-complete.
        a.set_color_mode(gpui::WindowColorMode::Default)?;
        assert_eq!(a.layer().unwrap().pixelFormat(), MTLPixelFormat::BGRA8Unorm);
        b_renderer.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
        assert_eq!(a.color_status().requested(), gpui::WindowColorMode::Default);
        b_renderer.render_scene(&wide, size(DevicePixels(8), DevicePixels(8)))?;
        wait(&a);
        close(sample(&saved, 4, 4), [1., 0., 0., 1.], 0.001);
        // Old producers still work in the opted-in renderer.
        #[derive(Debug)]
        struct Native;
        impl CustomContent for Native {
            fn prepare(&self, _: &dyn Any, _: Size<DevicePixels>) -> Result<Box<dyn Any>> {
                Ok(Box::new(gpui::Background::from(rgb(0x808080))))
            }
        }
        let mut legacy = Scene::default();
        let mut d = draw(b(0., 0., 8., 8.), Arc::new(Patch::new([1., 0., 0., 1.])));
        d.content = Arc::new(Native);
        legacy.insert_primitive(d);
        legacy.finish();
        let image =
            b_renderer.render_scene_to_image(&legacy, size(DevicePixels(8), DevicePixels(8)))?;
        assert!(
            image.get_pixel(4, 4).0[..3]
                .iter()
                .all(|c| c.abs_diff(128) <= 1)
        );
        Ok(())
    })
}

#[test]
fn wide_sdr_transparency_failure_preserves_configuration() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = MetalRenderer::new(Arc::new(Mutex::new(Default::default())), true);
        let status = r.color_status();
        assert!(
            r.set_color_mode(gpui::WindowColorMode::WideGamutSdr)
                .is_err()
        );
        assert_eq!(r.color_status(), status);
        assert_eq!(r.layer().unwrap().pixelFormat(), MTLPixelFormat::BGRA8Unorm);
        r.update_transparency(false);
        r.set_color_mode(gpui::WindowColorMode::WideGamutSdr)?;
        r.update_transparency(true); // Infallible old API must not invalidate the mode.
        assert!(r.opaque);
        assert_eq!(
            r.color_status().requested(),
            gpui::WindowColorMode::WideGamutSdr
        );
        Ok(())
    })
}

#[test]
fn wide_sdr_legacy_gpu_output_remains_srgb_and_requires_no_new_producer_method() -> Result<()> {
    #[derive(Debug)]
    struct Legacy;
    impl CustomContent for Legacy {
        fn prepare(&self, backend: &dyn Any, extent: Size<DevicePixels>) -> Result<Box<dyn Any>> {
            let c = backend
                .downcast_ref::<crate::custom_draw::MetalCustomContext>()
                .unwrap();
            let t = crate::custom_draw::texture(&c.device, extent)?;
            let command = c
                .queue
                .commandBuffer()
                .expect("Metal command buffer allocation failed");
            let p = metal::RenderPassDescriptor::new();
            let attachment = unsafe { p.colorAttachments().objectAtIndexedSubscript(0) };
            attachment.setTexture(Some(&t));
            attachment.setLoadAction(metal::MTLLoadAction::Clear);
            attachment.setStoreAction(metal::MTLStoreAction::Store);
            attachment.setClearColor(MTLClearColor {
                red: 0.5,
                green: 0.5,
                blue: 0.5,
                alpha: 1.,
            });
            command
                .renderCommandEncoderWithDescriptor(&p)
                .expect("Metal render encoder allocation failed")
                .endEncoding();
            Ok(Box::new(crate::custom_draw::MetalCustomOutput {
                texture: t,
                commands: vec![command.to_owned()],
            }))
        }
    }
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let mut s = Scene::default();
        let mut old = draw(b(0., 0., 8., 8.), Arc::new(Patch::new([1., 0., 0., 1.])));
        old.content = Arc::new(Legacy);
        s.insert_primitive(old);
        s.insert_primitive(draw(
            b(8., 0., 8., 8.),
            Arc::new(Patch::new([1., 0., 0., 1.])),
        ));
        s.finish();
        let image = r.render_scene_to_image(&s, size(DevicePixels(16), DevicePixels(8)))?;
        assert!(
            image.get_pixel(4, 4).0[..3]
                .iter()
                .all(|c| c.abs_diff(128) <= 1)
        );
        let raw = snapshot(&r);
        wait(&r);
        close(sample(&raw, 12, 4), [1., 0., 0., 1.], 0.001);
        Ok(())
    })
}

#[test]
fn wide_sdr_invalid_viewport_is_rejected_before_gpu_allocation() -> Result<()> {
    let mut renderer = renderer();
    let scene = Scene::default();
    let invalid = size(DevicePixels(i32::MAX), DevicePixels(i32::MAX));
    assert!(renderer.render_scene(&scene, invalid).is_err());
    assert!(renderer.render_scene_to_image(&scene, invalid).is_err());
    assert!(renderer.headless_render_target.is_none());
    assert!(renderer.path_intermediate_texture.is_none());
    assert!(renderer.wide_sdr.as_ref().unwrap().scratch.is_none());
    Ok(())
}

#[derive(Debug)]
struct Rec2020Patch(gpui::ImageTextureData);
impl CustomContent for Rec2020Patch {
    fn image_color_mode(&self) -> gpui::ImageColorMode {
        gpui::ImageColorMode::LinearRec2020Wide
    }
    fn prepare(&self, backend: &dyn Any, _: Size<DevicePixels>) -> Result<Box<dyn Any>> {
        let context = backend
            .downcast_ref::<crate::custom_draw::MetalCustomContext>()
            .unwrap();
        Ok(Box::new(context.create_image_output(&self.0)?))
    }
}

#[test]
fn rec2020_native_conversion_retains_extended_p3_channels() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let mut bytes = Vec::new();
        for &(input, _) in REC2020_P3_REFERENCES {
            for v in input {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
        }
        let count = REC2020_P3_REFERENCES.len();
        let data = gpui::ImageTextureData::page(
            count as u32,
            1,
            bytes,
            gpui::ImageTextureFormat::LinearRec2020Rgba16Float,
        )?;
        let bounds = b(0., 0., count as f32, 1.);
        let mut s = Scene::default();
        s.insert_primitive(CustomDraw {
            order: 0,
            bounds,
            content_mask: ContentMask { bounds },
            corner_radii: gpui::Corners::all(ScaledPixels(0.)),
            opacity: 1.,
            content: Arc::new(Rec2020Patch(data)),
            coverage: None,
        });
        s.finish();
        r.render_scene(&s, size(DevicePixels(count as i32), DevicePixels(1)))?;
        let shot = snapshot(&r);
        wait(&r);
        for (x, &(_, expected)) in REC2020_P3_REFERENCES.iter().enumerate() {
            let actual = sample(&shot, x, 0);
            for i in 0..3 {
                let e = half(expected[i]);
                // One binary16 step covers shader matrix rounding before storage.
                let next = half(expected[i].saturating_add(1));
                let previous = half(expected[i].saturating_sub(1));
                let budget = (next - e)
                    .abs()
                    .max((previous - e).abs())
                    .min(0.001)
                    .max(2f32.powi(-24));
                assert!(
                    (actual[i] - e).abs() <= budget,
                    "pixel {x} channel {i}: {} != {e} (budget {budget})",
                    actual[i]
                );
                if e == 0. {
                    assert_eq!(actual[i], 0.);
                }
            }
        }
        // Rec.2020 green explicitly has negative red and blue in linear P3.
        assert!(REC2020_P3_REFERENCES.iter().enumerate().any(|(x, _)| {
            let p = sample(&shot, x, 0);
            p[0] < -0.1 && p[2] < -0.01
        }));
        Ok(())
    })
}

include!("rec2020_reference.rs");

#[test]
fn public_fp16_readback_preserves_extended_channels_and_unpads_rows() -> Result<()> {
    objc2::rc::autoreleasepool(|_| {
        let mut r = renderer();
        let mut scene = Scene::default();
        scene.insert_primitive(draw(
            b(0., 0., 13., 7.),
            Arc::new(Patch::new([-0.125, 1.25, 0.00006103515625, 1.])),
        ));
        scene.finish();
        let image = r.render_scene_to_linear_p3(&scene, size(DevicePixels(13), DevicePixels(7)))?;
        assert_eq!(image.size, size(DevicePixels(13), DevicePixels(7)));
        assert_eq!(image.pixels.len(), 91);
        for pixel in image.pixels {
            assert_eq!(pixel, [0xb000, 0x3d00, 0x0400, 0x3c00]);
        }
        // Resize must replace the source target and return the new dimensions.
        let image = r.render_scene_to_linear_p3(&scene, size(DevicePixels(5), DevicePixels(3)))?;
        assert_eq!(image.pixels.len(), 15);
        r.set_color_mode(gpui::WindowColorMode::Default)?;
        assert!(
            r.render_scene_to_linear_p3(&scene, size(DevicePixels(5), DevicePixels(3)))
                .is_err()
        );
        Ok(())
    })
}
