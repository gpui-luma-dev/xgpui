use super::*;

fn verify(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let mut scene = Scene::default();
    let full = rect(0., 0., 96., 64.);
    scene.insert_primitive(Quad {
        bounds: full,
        content_mask: ContentMask { bounds: full },
        background: black().into(),
        ..Default::default()
    });
    let shadow = rect(12., 12., 28., 28.);
    scene.insert_primitive(Shadow {
        order: 0,
        blur_radius: ScaledPixels(4.),
        bounds: shadow,
        corner_radii: Corners::all(ScaledPixels(4.)),
        content_mask: ContentMask {
            bounds: rect(0., 0., 36., 64.),
        },
        color: white(),
        element_bounds: shadow,
        element_corner_radii: Corners::all(ScaledPixels(4.)),
        inset: 0,
        pad: 0,
    });
    scene.insert_primitive(Quad {
        bounds: rect(48., 8., 36., 36.),
        content_mask: ContentMask {
            bounds: rect(0., 0., 80., 64.),
        },
        border_widths: Edges::all(ScaledPixels(3.25)),
        border_color: rgb(0xff0000).into(),
        background: rgb(0x0000ff).into(),
        ..Default::default()
    });
    scene.finish();
    let image = renderer.render_scene_to_image(&scene, size(DevicePixels(96), DevicePixels(64)))?;
    let pixel = |x, y| image.get_pixel(x, y).0;
    ensure!(pixel(24, 24)[0] > 180, "shadow center should remain bright");
    ensure!(
        (1..pixel(24, 24)[0]).contains(&pixel(9, 24)[0]),
        "shadow tail should fade outside its shape"
    );
    ensure!(
        pixel(37, 24) == [0, 0, 0, 255],
        "shadow leaked through clip"
    );
    let inner_edge = pixel(51, 20);
    ensure!(
        inner_edge[0] > 0 && inner_edge[2] > 0 && inner_edge[3] == 255,
        "fractional outline edge should blend with the fill: {inner_edge:?}"
    );
    ensure!(
        pixel(81, 20) == [0, 0, 0, 255],
        "outline fill leaked through clip"
    );
    Ok(())
}

#[::core::prelude::v1::test]
fn native_primitive_clipping_outline_and_shadow_readback() -> Result<()> {
    verify(
        gpui_platform::current_headless_renderer()?
            .expect("native renderer")
            .as_mut(),
    )
}

#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_primitive_clipping_outline_and_shadow_readback() -> Result<()> {
    verify(&mut gpui_wgpu::WgpuHeadlessRenderer::new()?)
}
