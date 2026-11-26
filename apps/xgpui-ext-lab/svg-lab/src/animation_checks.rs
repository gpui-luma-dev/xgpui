use anyhow::{Result, ensure};
use gpui::{prelude::*, *};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use xgpui_shader::BackdropBlur;

struct AnimatedSvg {
    blur: BackdropBlur,
    renders: Rc<RefCell<usize>>,
}

impl Render for AnimatedSvg {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        *self.renders.borrow_mut() += 1;
        let blur = self.blur.clone();
        div()
            .size_full()
            .bg(black())
            .child(
                svg()
                    .absolute()
                    .left(px(8.))
                    .top(px(16.))
                    .size(px(16.))
                    .text_color(white())
                    .data(br#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><rect width="16" height="16"/></svg>"#)
                    .with_timed_transformation(
                        TimedTransformation::new(Duration::from_secs(1))
                            .to(Transformation::translate(point(px(80.), px(0.)))),
                    ),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        blur.paint(window, bounds, Corners::all(px(0.)), 1.);
                    },
                )
                .size_full(),
            )
    }
}

struct Readback {
    renderer: Box<dyn PlatformHeadlessRenderer>,
    key: Rc<RefCell<Option<BackdropCacheKey>>>,
}

impl PlatformHeadlessRenderer for Readback {
    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.renderer.sprite_atlas()
    }

    fn render_scene(&mut self, scene: &Scene, extent: Size<DevicePixels>) -> Result<()> {
        self.renderer.render_scene(scene, extent)
    }

    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        extent: Size<DevicePixels>,
    ) -> Result<image::RgbaImage> {
        let keys = scene.backdrop_cache_keys(extent)?;
        ensure!(
            keys.len() == 1 && keys[0].reusable,
            "expected one cacheable backdrop"
        );
        let image = self.renderer.render_scene_to_image(scene, extent)?;
        let mut replay = Scene::default();
        replay.replay(0..scene.len(), scene);
        replay.finish();
        ensure!(
            keys == replay.backdrop_cache_keys(extent)?,
            "replay changed the sampled revision"
        );
        ensure!(
            image == self.renderer.render_scene_to_image(&replay, extent)?,
            "replay changed the animated image"
        );
        *self.key.borrow_mut() = keys.into_iter().next();
        Ok(image)
    }
}

fn verify(
    create_renderer: impl Fn() -> Result<Option<Box<dyn PlatformHeadlessRenderer>>> + 'static,
) -> Result<()> {
    let key = Rc::new(RefCell::new(None));
    let mut cx =
        HeadlessAppContext::with_platform(Arc::new(NoopTextSystem::new()), Arc::new(()), {
            let key = key.clone();
            move || {
                Ok(create_renderer()?.map(|renderer| {
                    Box::new(Readback {
                        renderer,
                        key: key.clone(),
                    }) as _
                }))
            }
        });
    let renders = Rc::new(RefCell::new(0));
    let blur = BackdropBlur::new(px(2.))?;
    let window = cx.open_window(size(px(96.), px(48.)), |_, cx| {
        cx.new(|_| AnimatedSvg {
            blur,
            renders: renders.clone(),
        })
    })?;
    cx.simulate_window_scale_factor_change(window.into(), 1.);
    cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
    let original_draw_count = *renders.borrow();
    let first = cx.capture_screenshot(window.into())?;
    let first_key = key.borrow().clone().unwrap();
    assert!(first.get_pixel(16, 24)[0] > 180 && first.get_pixel(56, 24)[0] < 30);

    cx.advance_clock(Duration::from_millis(500));
    let second = cx.capture_screenshot(window.into())?;
    let second_key = key.borrow().clone().unwrap();
    assert_ne!(
        second_key, first_key,
        "changed SVG pose must invalidate the cache"
    );
    assert!(second.get_pixel(16, 24)[0] < 30 && second.get_pixel(56, 24)[0] > 180);
    assert_eq!(cx.capture_screenshot(window.into())?, second);
    assert_eq!(
        key.borrow().as_ref(),
        Some(&second_key),
        "same pose should reuse the capture"
    );
    assert_eq!(
        *renders.borrow(),
        original_draw_count,
        "animation rebuilt the view"
    );
    Ok(())
}

#[::core::prelude::v1::test]
fn native_svg_animation_invalidates_backdrop() -> Result<()> {
    verify(gpui_platform::current_headless_renderer)
}

#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_svg_animation_invalidates_backdrop() -> Result<()> {
    verify(|| Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))))
}
