//! Experimental bounded rounded subtree composition for issue #37.
//!
//! This is intentionally separate from ordinary `overflow_hidden`: it uses two
//! capture textures and the existing layer compositor's limits. Nested captures,
//! backdrop effects and escaped deferred overlays are outside this initial slice.
use anyhow::{Result, ensure};
use gpui::*;

/// Experimental rounded overflow backed by bounded subtree isolation.
///
/// The caller supplies the inner border bounds and corresponding inner radii in
/// both phases. This preserves legacy layout/overflow APIs. Uniform border insets
/// can be subtracted from outer radii; elliptical inner corners are not supported.
/// Not a replacement for the eventual direct/cached-coverage clipping path.
pub struct RoundedOverflow {
    radii: Corners<Pixels>,
}
impl RoundedOverflow {
    /// Use circular per-corner radii, clamped exactly like GPUI painted corners.
    pub fn new(radii: Corners<Pixels>) -> Result<Self> {
        let mut result = Self {
            radii: Corners::default(),
        };
        result.set_radii(radii)?;
        Ok(result)
    }
    /// Update radii without changing the shared composition program.
    pub fn set_radii(&mut self, radii: Corners<Pixels>) -> Result<()> {
        ensure!(
            [
                radii.top_left,
                radii.top_right,
                radii.bottom_right,
                radii.bottom_left
            ]
            .iter()
            .all(|r| f32::from(*r).is_finite() && *r >= px(0.)),
            "invalid rounded overflow radii"
        );
        self.radii = radii;
        Ok(())
    }
    /// Prepaint with matching input geometry and grayscale capture-compatible text.
    /// Nested captured groups are explicitly rejected rather than darkening shared edges.
    pub fn prepaint<R>(
        &self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        f: impl FnOnce(&mut Window) -> R,
    ) -> Result<R> {
        ensure!(
            !window.is_layer_captured(),
            "experimental rounded overflow cannot nest inside a captured group"
        );
        window.with_rounded_hit_test_mask(bounds, self.radii, |window| {
            window.with_layer_capture_scope(bounds, f)
        })
    }
    /// Clip the completed ordinary GPUI subtree, including its child backgrounds.
    /// This uses the existing four-groups/frame, 1024-device-pixel capture limit.
    pub fn paint<R>(
        &mut self,
        window: &mut Window,
        bounds: Bounds<Pixels>,
        f: impl FnOnce(&mut Window) -> R,
    ) -> Result<R> {
        ensure!(
            !window.is_layer_captured(),
            "experimental rounded overflow cannot nest inside a captured group"
        );
        window.with_rounded_layer_clip(bounds, self.radii, crate::backdrop::shared_program()?, f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn rounded_overflow_rejects_bad_radii_and_translates_all_shader_targets() {
        for r in [f32::NAN, f32::INFINITY, -1.] {
            assert!(RoundedOverflow::new(Corners::all(px(r))).is_err());
        }
        crate::backdrop::shared_program().unwrap();
    }
}

#[cfg(all(test, feature = "wgpu", not(target_family = "wasm")))]
mod render_tests {
    use super::*;
    use std::{cell::Cell, rc::Rc, sync::Arc};
    struct ReplayRenderer {
        inner: Box<dyn PlatformHeadlessRenderer>,
    }
    impl PlatformHeadlessRenderer for ReplayRenderer {
        fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
            self.inner.sprite_atlas()
        }
        fn render_scene(&mut self, scene: &Scene, size: Size<DevicePixels>) -> anyhow::Result<()> {
            self.inner.render_scene(scene, size)
        }
        fn render_scene_to_image(
            &mut self,
            scene: &Scene,
            size: Size<DevicePixels>,
        ) -> anyhow::Result<image::RgbaImage> {
            let image = self.inner.render_scene_to_image(scene, size)?;
            let mut replay = Scene::default();
            replay.replay(0..scene.len(), scene);
            replay.finish();
            let replayed = self.inner.render_scene_to_image(&replay, size)?;
            assert_eq!(
                image.as_raw(),
                replayed.as_raw(),
                "retained replay changed rounded output"
            );
            Ok(image)
        }
    }
    struct View {
        radius: Rc<Cell<f32>>,
    }
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let radius = self.radius.get();
            div().size_full().bg(white()).child(
                canvas(
                    move |b, window, _| {
                        RoundedOverflow::new(Corners::all(px(radius)))
                            .unwrap()
                            .prepaint(window, b, |_| ())
                            .unwrap();
                    },
                    move |b, (), window, _| {
                        RoundedOverflow::new(Corners::all(px(radius)))
                            .unwrap()
                            .paint(window, b, |window| {
                                window.paint_quad(fill(b, rgb(0xff0000)))
                            })
                            .unwrap();
                    },
                )
                .w(px(80.))
                .h(px(64.)),
            )
        }
    }
    fn exercise(
        factory: impl Fn() -> anyhow::Result<Option<Box<dyn PlatformHeadlessRenderer>>> + 'static,
    ) {
        let factory = Arc::new(factory);
        let make = factory.clone();
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(NoopTextSystem::new()),
            Arc::new(()),
            move || {
                Ok(make()?.map(|inner| {
                    Box::new(ReplayRenderer { inner }) as Box<dyn PlatformHeadlessRenderer>
                }))
            },
        );
        let radius = Rc::new(Cell::new(20.));
        let r = radius.clone();
        let window = cx
            .open_window(size(px(96.), px(80.)), move |_, cx| {
                cx.new(|_| View { radius: r })
            })
            .unwrap();
        for value in [20., 0., 32., 20.] {
            radius.set(value);
            cx.update_window(window.into(), |_, window, cx| {
                window.refresh();
                window.draw(cx).clear(cx)
            })
            .unwrap();
            let image = cx.capture_screenshot(window.into()).unwrap();
            let scale = cx
                .update_window(window.into(), |_, window, _| window.scale_factor())
                .unwrap();
            let corner = image.get_pixel(scale as u32, scale as u32).0;
            let middle = image
                .get_pixel((40. * scale) as u32, (20. * scale) as u32)
                .0;
            assert!(
                middle[0] > 245 && middle[1] < 10,
                "missing captured interior: {middle:?}"
            );
            if value > 0. {
                assert!(
                    corner[0] > 245 && corner[1] > 245,
                    "rounded corner leaked: {corner:?}"
                );
            } else {
                assert!(
                    corner[0] > 245 && corner[1] < 10,
                    "zero radius clipped: {corner:?}"
                );
            }
        }
    }
    #[::core::prelude::v1::test]
    fn rounded_overflow_wgpu_pixels_radius_changes_and_replay() {
        exercise(|| Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))));
    }
    #[cfg(target_os = "macos")]
    #[::core::prelude::v1::test]
    fn rounded_overflow_metal_pixels_radius_changes_and_replay() {
        exercise(|| {
            Ok(Some(Box::new(
                gpui_apple::metal_renderer::MetalHeadlessRenderer::new()?,
            )))
        });
    }
}
