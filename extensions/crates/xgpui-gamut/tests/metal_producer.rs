#![cfg(target_os = "macos")]
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
        for (alpha, opacity, radius) in [
            (0., 1., 0.),
            (0.5, 1., 0.),
            (1., 1., 0.),
            (0.5, 0.35, 3.),
            (1., 1., 3.),
        ] {
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
                corner_radii: Corners::all(ScaledPixels(radius)),
                opacity,
                content: Arc::new(WideColor::linear_p3([1., 0., 0., alpha])?),
                coverage: None,
            });
            scene.finish();
            let image =
                renderer.render_scene_to_image(&scene, size(DevicePixels(8), DevicePixels(8)))?;
            let mut edge_samples = 0;
            for (x, y, pixel) in image.enumerate_pixels() {
                let qx = (x as f32 + 0.5 - 4.).abs() - 4. + radius;
                let qy = (y as f32 + 0.5 - 4.).abs() - 4. + radius;
                let distance = qx.max(0.).hypot(qy.max(0.)) + qx.max(qy).min(0.) - radius;
                let coverage = (0.5 - distance).clamp(0., 1.);
                edge_samples += usize::from(coverage > 0. && coverage < 1.);
                // Straight-alpha source-over reference: coverage affects alpha only.
                let above_alpha = alpha * coverage * opacity;
                let output_alpha = above_alpha + (1. - above_alpha);
                let expected: Vec<_> = [1.2249402, -0.04205695, -0.01963755]
                    .into_iter()
                    .map(|red| {
                        let linear = ((red * above_alpha + gray * (1. - above_alpha))
                            / output_alpha)
                            .clamp(0., 1.);
                        let encoded = if linear <= 0.0031308 {
                            12.92 * linear
                        } else {
                            1.055 * linear.powf(1. / 2.4) - 0.055
                        };
                        (encoded * 255.).round() as u8
                    })
                    .collect();
                for i in 0..3 {
                    assert!(
                        pixel[i].abs_diff(expected[i]) <= 2,
                        "({x},{y}), gray={gray}, alpha={alpha}, opacity={opacity}, coverage={coverage}: {pixel:?}, expected {expected:?}"
                    );
                }
                assert_eq!(pixel[3], 255);
            }
            if radius > 0. {
                assert!(edge_samples > 0, "must exercise fractional coverage");
            }
        }
    }
    Ok(())
}
