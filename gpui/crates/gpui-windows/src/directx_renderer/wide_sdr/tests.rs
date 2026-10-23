use super::*;
use std::borrow::Cow;

fn extent() -> Size<DevicePixels> {
    size(DevicePixels(8), DevicePixels(8))
}
fn setup() -> Result<(Dx11HeadlessRenderer, Compositor)> {
    let renderer = Dx11HeadlessRenderer::new()?;
    let compositor = Compositor::new(&renderer.0.devices.as_ref().unwrap().device, extent())?;
    Ok((renderer, compositor))
}
fn patch(renderer: &Dx11HeadlessRenderer, color: [f32; 4]) -> Result<Target> {
    ensure!(
        color
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v)),
        "invalid test P3 color"
    );
    let d = renderer.0.devices.as_ref().unwrap();
    let target = Target::new(&d.device, extent())?;
    let [r, g, b, a] = color;
    unsafe {
        d.device_context
            .ClearRenderTargetView(&target.rtv, &[r * a, g * a, b * a, a]);
    }
    Ok(target)
}
fn layer(source: &Target, opacity: f32) -> Layer<'_> {
    Layer::P3 {
        source,
        bounds: [0., 0., 8., 8.],
        opacity,
    }
}
fn scene(color: Rgba, x: f32, width: f32) -> Scene {
    let mut scene = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(x), ScaledPixels(0.)),
        size(ScaledPixels(width), ScaledPixels(8.)),
    );
    scene.insert_primitive(Quad {
        bounds,
        content_mask: ContentMask { bounds },
        background: color.into(),
        ..Default::default()
    });
    scene.finish();
    scene
}
fn read(renderer: &Dx11HeadlessRenderer, target: &Target) -> Result<Vec<[f32; 4]>> {
    let devices = renderer.0.devices.as_ref().unwrap();
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        target.texture.GetDesc(&mut desc);
    }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe {
        devices
            .device
            .CreateTexture2D(&desc, None, Some(&mut staging))?;
    }
    let staging = staging.unwrap();
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        devices.device_context.OMSetRenderTargets(None, None);
        devices
            .device_context
            .CopyResource(&staging, &target.texture);
        devices
            .device_context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let mut pixels = Vec::new();
    for y in 0..desc.Height as usize {
        // SAFETY: Map succeeded; copy each Width*8 row before Unmap, respecting RowPitch.
        let row = unsafe {
            std::slice::from_raw_parts(
                (mapped.pData as *const u8).add(y * mapped.RowPitch as usize),
                desc.Width as usize * 8,
            )
        };
        pixels.extend(row.as_chunks::<8>().0.iter().map(|p| {
            std::array::from_fn(|i| reference::half(u16::from_le_bytes([p[2 * i], p[2 * i + 1]])))
        }));
    }
    unsafe {
        devices.device_context.Unmap(&staging, 0);
    }
    Ok(pixels)
}
fn close(actual: [f32; 4], expected: [f32; 4], tolerance: f32) {
    for i in 0..4 {
        assert!(
            (actual[i] - expected[i]).abs() <= tolerance,
            "channel {i}: {actual:?} != {expected:?}"
        );
    }
}
#[::core::prelude::v1::test]
fn p3_conversion_preserves_saturation_and_neutral() -> Result<()> {
    let (mut r, c) = setup()?;
    for color in [
        [1., 0., 0., 1.],
        [0., 1., 0., 1.],
        [0., 0., 1., 1.],
        [1.; 4],
        [0.25, 0.25, 0.25, 1.],
    ] {
        let p = patch(&r, color)?;
        c.render(&mut r.0, &[layer(&p, 1.)])?;
        for pixel in read(&r, &c.target)? {
            close(pixel, reference::p3(color), 0.002);
        }
    }
    Ok(())
}
#[::core::prelude::v1::test]
fn associated_alpha_opacity_and_zero_alpha() -> Result<()> {
    let (mut r, c) = setup()?;
    for alpha in [0., 0.00001, 0.25, 1.] {
        let p = patch(&r, [1., 0., 0., alpha])?;
        c.render(&mut r.0, &[layer(&p, 0.5)])?;
        close(
            read(&r, &c.target)?[0],
            reference::p3([alpha * 0.5, 0., 0., alpha * 0.5]),
            if alpha < 0.001 { 0.0000001 } else { 0.0005 },
        );
    }
    Ok(())
}
#[::core::prelude::v1::test]
fn overlays_and_draw_order_preserve_intermediate_ui_and_clear_scratch() -> Result<()> {
    let (mut r, c) = setup()?;
    let red = patch(&r, [1., 0., 0., 1.])?;
    let blue = patch(&r, [0., 0., 1., 0.5])?;
    let overlay = scene(rgba(0x00ff0080), 0., 4.);
    let tooltip = scene(rgb(0xffffff), 6., 2.);
    for _ in 0..3 {
        c.render(
            &mut r.0,
            &[
                layer(&red, 1.),
                Layer::Ui(&overlay),
                layer(&blue, 1.),
                Layer::Ui(&tooltip),
            ],
        )?;
        let pixels = read(&r, &c.target)?;
        let base = reference::p3([1., 0., 0., 1.]);
        let a = 128. / 255.;
        let ui = [0., a, 0., a];
        close(
            pixels[1],
            reference::over(reference::p3([0., 0., 0.5, 0.5]), reference::over(ui, base)),
            0.005,
        );
        close(
            pixels[5],
            reference::over(reference::p3([0., 0., 0.5, 0.5]), base),
            0.003,
        );
        close(pixels[7], [1.; 4], 0.002);
    }
    c.render(
        &mut r.0,
        &[layer(&blue, 1.), Layer::Ui(&overlay), layer(&red, 1.)],
    )?;
    close(
        read(&r, &c.target)?[1],
        reference::p3([1., 0., 0., 1.]),
        0.002,
    );
    Ok(())
}
#[::core::prelude::v1::test]
fn native_srgb_decode_and_low_alpha_over_light_and_dark() -> Result<()> {
    let (mut r, c) = setup()?;
    let native = scene(rgb(0x808080), 0., 8.);
    c.render(&mut r.0, &[Layer::Ui(&native)])?;
    close(
        read(&r, &c.target)?[0],
        [
            reference::decode(128. / 255.),
            reference::decode(128. / 255.),
            reference::decode(128. / 255.),
            1.,
        ],
        0.002,
    );
    let p = patch(&r, [1., 0., 0., 0.00001])?;
    for gray in [0., 1.] {
        let bg = patch(&r, [gray, gray, gray, 1.])?;
        c.render(&mut r.0, &[layer(&bg, 1.), layer(&p, 1.)])?;
        close(
            read(&r, &c.target)?[0],
            reference::over(
                reference::p3([0.00001, 0., 0., 0.00001]),
                [gray, gray, gray, 1.],
            ),
            0.002,
        );
    }
    Ok(())
}
#[::core::prelude::v1::test]
fn invalid_frame_does_not_clear_previous_output() -> Result<()> {
    let (mut r, c) = setup()?;
    let p = patch(&r, [1.; 4])?;
    c.render(&mut r.0, &[layer(&p, 1.)])?;
    assert!(c.render(&mut r.0, &[layer(&p, f32::NAN)]).is_err());
    close(read(&r, &c.target)?[0], [1.; 4], 0.002);
    assert!(validate_extent(size(DevicePixels(0), DevicePixels(8))).is_err());
    assert!(validate_extent(size(DevicePixels(16384), DevicePixels(16384))).is_err());
    assert!(validate_extent(size(DevicePixels(-1), DevicePixels(8))).is_err());
    let (foreign, _) = setup()?;
    let foreign_patch = patch(&foreign, [1.; 4])?;
    assert!(c.render(&mut r.0, &[layer(&foreign_patch, 1.)]).is_err());
    close(read(&r, &c.target)?[0], [1.; 4], 0.002);
    Ok(())
}
#[::core::prelude::v1::test]
fn cached_subpixel_tiles_use_scalar_alpha_and_restore_legacy_pipeline() -> Result<()> {
    let (mut r, c) = setup()?;
    let extent = size(DevicePixels(4), DevicePixels(4));
    // An asymmetric LCD tile would fringe under dual-source rendering.
    let key = AtlasKey::Glyph(RenderGlyphParams {
        font_id: FontId(0),
        glyph_id: GlyphId(1),
        font_size: px(12.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: true,
        dilation: 0,
    });
    let tile = r
        .sprite_atlas()
        .get_or_insert_with(key, &mut || {
            Ok(Some((extent, Cow::Owned([255u8, 64, 0, 255].repeat(16)))))
        })?
        .unwrap();
    let mut text = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    text.insert_primitive(SubpixelSprite {
        order: 0,
        pad: 0,
        bounds,
        content_mask: ContentMask { bounds },
        color: white(),
        tile,
        transformation: TransformationMatrix::default(),
    });
    text.finish();
    let old_fragment = r.0.pipelines.subpixel_sprites.fragment.as_raw();
    let old_blend = r.0.pipelines.subpixel_sprites.blend_state.as_raw();
    c.render(&mut r.0, &[Layer::Ui(&text)])?;
    let pixels = read(&r, &c.target)?;
    let coverage = pixels[3 * 8 + 3][3];
    assert!(
        coverage > 0. && coverage < 1.,
        "scalar text needs associated alpha: {coverage}"
    );
    for p in pixels {
        close(p, [p[3]; 4], 0.004);
    }
    assert_eq!(
        r.0.pipelines.subpixel_sprites.fragment.as_raw(),
        old_fragment
    );
    assert_eq!(
        r.0.pipelines.subpixel_sprites.blend_state.as_raw(),
        old_blend
    );
    for gray in [0., 1.] {
        let bg = patch(&r, [gray, gray, gray, 1.])?;
        c.render(&mut r.0, &[layer(&bg, 1.), Layer::Ui(&text)])?;
        let p = read(&r, &c.target)?[3 * 8 + 3];
        close(
            p,
            [
                coverage + gray * (1. - coverage),
                coverage + gray * (1. - coverage),
                coverage + gray * (1. - coverage),
                1.,
            ],
            0.004,
        );
    }
    // Legacy rendering still uses its original pipeline after the scoped override.
    r.0.render(&text, WindowBackgroundAppearance::Opaque)?;
    Ok(())
}

#[::core::prelude::v1::test]
fn native_grayscale_tiles_keep_coverage_and_scene_mask() -> Result<()> {
    let (mut r, c) = setup()?;
    let tile_size = size(DevicePixels(4), DevicePixels(4));
    let key = AtlasKey::Glyph(RenderGlyphParams {
        font_id: FontId(0),
        glyph_id: GlyphId(2),
        font_size: px(12.),
        subpixel_variant: point(0, 0),
        scale_factor: 1.,
        is_emoji: false,
        subpixel_rendering: false,
        dilation: 0,
    });
    let tile = r
        .sprite_atlas()
        .get_or_insert_with(key, &mut || {
            Ok(Some((tile_size, Cow::Owned(vec![128; 16]))))
        })?
        .unwrap();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    let mask = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(4.), ScaledPixels(8.)),
    );
    let mut text = Scene::default();
    text.insert_primitive(MonochromeSprite {
        order: 0,
        pad: 0,
        bounds,
        content_mask: ContentMask { bounds: mask },
        color: white(),
        tile,
        transformation: TransformationMatrix::unit(),
    });
    text.finish();
    c.render(&mut r.0, &[Layer::Ui(&text)])?;
    let first = read(&r, &c.target)?;
    let coverage = first[3 * 8 + 2][3];
    assert!(coverage > 0. && coverage < 1.);
    close(first[3 * 8 + 2], [coverage; 4], 0.004);
    close(first[3 * 8 + 6], [0.; 4], 0.00001);
    let bg = patch(&r, [1., 0., 0., 1.])?;
    c.render(&mut r.0, &[layer(&bg, 1.), Layer::Ui(&text)])?;
    let pixels = read(&r, &c.target)?;
    close(
        pixels[3 * 8 + 2],
        reference::over([coverage; 4], reference::p3([1., 0., 0., 1.])),
        0.004,
    );
    close(pixels[3 * 8 + 6], reference::p3([1., 0., 0., 1.]), 0.002);
    Ok(())
}

#[::core::prelude::v1::test]
fn native_run_retains_legacy_blending_and_wide_bounds_clip() -> Result<()> {
    let (mut r, c) = setup()?;
    let mut native = Scene::default();
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    for color in [rgb(0xff0000), rgba(0x00ff0080)] {
        native.insert_primitive(Quad {
            bounds,
            content_mask: ContentMask { bounds },
            background: color.into(),
            ..Default::default()
        });
    }
    native.finish();
    let blue = patch(&r, [0., 0., 1., 1.])?;
    c.render(
        &mut r.0,
        &[
            Layer::Ui(&native),
            Layer::P3 {
                source: &blue,
                bounds: [4., 2., 8., 4.],
                opacity: 0.5,
            },
        ],
    )?;
    let pixels = read(&r, &c.target)?;
    // The UI run blends encoded sRGB before conversion, as in the legacy renderer.
    let expected = [
        reference::decode(127. / 255.),
        reference::decode(128. / 255.),
        0.,
        1.,
    ];
    close(pixels[1], expected, 0.005);
    close(pixels[3 * 8 + 3], expected, 0.005);
    close(
        pixels[3 * 8 + 5],
        reference::over(reference::p3([0., 0., 0.5, 0.5]), expected),
        0.005,
    );
    close(pixels[7 * 8 + 5], expected, 0.005);
    Ok(())
}
