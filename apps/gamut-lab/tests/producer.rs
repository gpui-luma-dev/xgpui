//! Exercise the extension producer through the real Metal host, not a copy of it.
use gpui::{
    Bounds, ContentMask, Corners, CustomDraw, DevicePixels, Quad, ScaledPixels, Scene,
    WindowColorMode, point, rgb, size,
};
use gpui_apple::metal_renderer::MetalRenderer;
use parking_lot::Mutex;
use std::sync::Arc;
use xgpui_gamut::WideColor;

#[test]
fn extension_producer_preserves_p3_association_over_native_ui() -> anyhow::Result<()> {
    let mut renderer = MetalRenderer::new_headless(Arc::new(Mutex::new(Default::default())));
    renderer.set_color_mode(WindowColorMode::WideGamutSdr)?;
    let bounds = Bounds::new(
        point(ScaledPixels(0.), ScaledPixels(0.)),
        size(ScaledPixels(8.), ScaledPixels(8.)),
    );
    for gray in [0., 1.] {
        for alpha in [0., 0.5, 1.] {
            let mut scene = Scene::default();
            scene.insert_primitive(Quad {
                bounds,
                content_mask: ContentMask { bounds },
                background: rgb(if gray == 0. { 0 } else { 0xffffff }).into(),
                ..Default::default()
            });
            scene.insert_primitive(CustomDraw {
                order: 0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Corners::default(),
                opacity: 1.,
                content: Arc::new(WideColor::linear_p3([1., 0., 0., alpha])?),
                coverage: None,
            });
            scene.finish();
            let image =
                renderer.render_scene_to_image(&scene, size(DevicePixels(8), DevicePixels(8)))?;
            let expected: Vec<_> = [1.2249402, -0.04205695, -0.01963755]
                .into_iter()
                .map(|red| {
                    let linear = (red * alpha + gray * (1. - alpha)).clamp(0., 1.);
                    let encoded = if linear <= 0.0031308 {
                        12.92 * linear
                    } else {
                        1.055 * linear.powf(1. / 2.4) - 0.055
                    };
                    (encoded * 255.).round() as u8
                })
                .collect();
            let pixel = image.get_pixel(4, 4).0;
            for i in 0..3 {
                assert!(
                    pixel[i].abs_diff(expected[i]) <= 2,
                    "gray={gray}, alpha={alpha}: {pixel:?}, expected {expected:?}"
                );
            }
            assert_eq!(pixel[3], 255);
        }
    }
    Ok(())
}
