#![cfg(all(feature = "portable-shaders", target_os = "macos", feature = "wgpu"))]
use super::*;
use crate::instanced::{ShadowPainter, ShadowStop, ShadowStyle};
use std::sync::Mutex;

#[derive(Default)]
struct Recorded {
    shadows: Vec<ShaderDraw>,
    quads: Vec<Quad>,
}
struct Recorder {
    inner: Box<dyn PlatformHeadlessRenderer>,
    recorded: Arc<Mutex<Recorded>>,
}
impl PlatformHeadlessRenderer for Recorder {
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.inner.sprite_atlas()
    }
    fn render_scene(&mut self, scene: &Scene, extent: Size<DevicePixels>) -> anyhow::Result<()> {
        *self.recorded.lock().unwrap() = Recorded {
            shadows: scene.shader_draws.clone(),
            quads: scene.quads.clone(),
        };
        self.inner.render_scene(scene, extent)
    }
    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        extent: Size<DevicePixels>,
    ) -> anyhow::Result<image::RgbaImage> {
        *self.recorded.lock().unwrap() = Recorded {
            shadows: scene.shader_draws.clone(),
            quads: scene.quads.clone(),
        };
        self.inner.render_scene_to_image(scene, extent)
    }
}
struct View {
    outer: crate::instanced::ShadowLayer,
    inner: crate::instanced::ShadowLayer,
}
impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let card = painted()
            .w(px(64.))
            .h(px(48.))
            .ml(px(24.))
            .mt(px(24.))
            .rounded(px(4.))
            .opacity(0.5)
            .overflow_hidden()
            .bg(white())
            .p_2()
            .shadow_paint(self.outer.clone())
            .shadow_paint(self.inner.clone())
            .child(
                canvas(
                    |_, _, _| (),
                    |_, (), window, _| {
                        // The child deliberately overpaints. Only its host mask may contain it.
                        window.paint_quad(fill(
                            Bounds::new(point(px(0.), px(0.)), size(px(128.), px(100.))),
                            rgb(0x00ff00),
                        ));
                    },
                )
                .size_full(),
            );
        div().size_full().bg(black()).child(
            div()
                .w(px(80.))
                .h(px(100.))
                .opacity(0.5)
                .overflow_hidden()
                .child(card),
        )
    }
}

#[::core::prelude::v1::test]
fn helper_preserves_layout_masks_opacity_and_background_child_order() {
    for metal in [true, false] {
        let recorded = Arc::new(Mutex::new(Recorded::default()));
        let output = recorded.clone();
        let mut cx = HeadlessAppContext::with_platform(
            Arc::new(NoopTextSystem::new()),
            Arc::new(()),
            move || {
                let inner: Box<dyn PlatformHeadlessRenderer> = if metal {
                    Box::new(gpui_apple::metal_renderer::MetalHeadlessRenderer::new())
                } else {
                    Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?)
                };
                Ok(Some(Box::new(Recorder {
                    inner,
                    recorded: output.clone(),
                })))
            },
        );
        let painter = ShadowPainter::new().unwrap();
        let style = ShadowStyle {
            stops: vec![ShadowStop::new(0., [1., 0., 0., 1.])],
            offset: point(px(0.), px(0.)),
            blur_radius: px(8.),
            ..Default::default()
        };
        let outer = painter.layer(style.clone()).unwrap();
        let inner = painter
            .layer(ShadowStyle {
                inset: true,
                ..style
            })
            .unwrap();
        let window = cx
            .open_window(size(px(128.), px(100.)), move |_, cx| {
                cx.new(|_| View { outer, inner })
            })
            .unwrap();
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let scale = cx
            .update_window(window.into(), |_, window, _| window.scale_factor())
            .unwrap();
        let image = cx.capture_screenshot(window.into()).unwrap();
        assert!(
            image
                .get_pixel((22. * scale) as u32, (48. * scale) as u32)
                .0[0]
                > 5,
            "outset must escape the card's own overflow mask"
        );
        assert_eq!(
            &image
                .get_pixel((90. * scale) as u32, (48. * scale) as u32)
                .0[..3],
            &[0, 0, 0],
            "ancestor clip must contain all card painting"
        );
        let result = recorded.lock().unwrap();
        assert_eq!(result.shadows.len(), 2);
        let outer = &result.shadows[0];
        let inner = &result.shadows[1];
        assert_eq!(outer.opacity, 0.5, "outset inherits parent opacity");
        assert_eq!(
            outer.data[4][3], 0.5,
            "outset folds card opacity exactly once into premultiplied color"
        );
        assert_eq!(
            inner.opacity, 0.25,
            "inset inherits both parent and card opacity"
        );
        assert_eq!(
            inner.data[4][3], 1.,
            "inset must not multiply card opacity twice"
        );
        assert_eq!(
            outer.content_mask.bounds.origin,
            point(ScaledPixels(0.), ScaledPixels(0.))
        );
        assert_eq!(
            outer.content_mask.bounds.size.width,
            ScaledPixels(80. * scale)
        );
        let background = result
            .quads
            .iter()
            .find(|q| q.background.as_solid().is_some_and(|c| c.l > 0.99))
            .unwrap();
        let child = result
            .quads
            .iter()
            .find(|q| {
                q.background
                    .as_solid()
                    .is_some_and(|c| c.s > 0.99 && (c.h - 1. / 3.).abs() < 0.01)
            })
            .unwrap();
        assert_eq!(
            background.bounds.origin,
            point(ScaledPixels(24. * scale), ScaledPixels(24. * scale))
        );
        assert_eq!(
            background.bounds.size,
            size(ScaledPixels(64. * scale), ScaledPixels(48. * scale))
        );
        assert_eq!(background.background.as_solid().unwrap().a, 0.25);
        assert!(
            outer.order < background.order
                && background.order < inner.order
                && inner.order < child.order,
            "outset → background → inset → children"
        );
        assert_eq!(
            child.content_mask.bounds.origin,
            point(ScaledPixels(24. * scale), ScaledPixels(24. * scale))
        );
        assert_eq!(
            child.content_mask.bounds.size.width,
            ScaledPixels(56. * scale)
        );
    }
}
