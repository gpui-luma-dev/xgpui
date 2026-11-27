use super::*;
use std::{cell::RefCell, rc::Rc};

struct BackdropScene {
    blur: xgpui_shader::BackdropBlur,
    foreground: bool,
}

impl Render for BackdropScene {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let blur = self.blur.clone();
        let foreground = self.foreground;
        canvas(
            |_, _, _| (),
            move |_, (), window, _| {
                for stripe in 0..16 {
                    window.paint_quad(fill(
                        Bounds::new(point(px(0.), px(stripe as f32 * 4.)), size(px(96.), px(4.))),
                        if stripe % 2 == 0 { white() } else { black() },
                    ));
                }
                blur.paint(
                    window,
                    Bounds::new(point(px(12.), px(8.)), size(px(36.), px(48.))),
                    Corners::all(px(0.)),
                    1.,
                );
                if foreground {
                    // This opaque run can use the depth pass. It misses the output
                    // rectangle but overlaps the earlier backdrop's sampling halo.
                    let bounds = Bounds::new(point(px(48.), px(0.)), size(px(48.), px(64.)));
                    for color in [0x00ff00, 0x0000ff].into_iter().cycle().take(7) {
                        window.paint_quad(fill(bounds, rgb(color)));
                    }
                    window.paint_quad(fill(bounds, rgb(0xff0000)));
                }
            },
        )
        .size_full()
    }
}

struct Readback {
    renderer: Rc<RefCell<Box<dyn PlatformHeadlessRenderer>>>,
    replay: Rc<RefCell<Scene>>,
}

impl PlatformHeadlessRenderer for Readback {
    fn sprite_atlas(&self) -> Arc<dyn gpui::PlatformAtlas> {
        self.renderer.borrow().sprite_atlas()
    }

    fn render_scene(&mut self, scene: &Scene, extent: Size<DevicePixels>) -> Result<()> {
        self.renderer.borrow_mut().render_scene(scene, extent)
    }

    fn render_scene_to_image(
        &mut self,
        scene: &Scene,
        extent: Size<DevicePixels>,
    ) -> Result<image::RgbaImage> {
        let mut replay = self.replay.borrow_mut();
        replay.clear();
        replay.replay(0..scene.len(), scene);
        replay.finish();
        ensure!(
            scene.backdrop_cache_keys(extent)? == replay.backdrop_cache_keys(extent)?,
            "replay changed the backdrop prefix identity"
        );
        self.renderer
            .borrow_mut()
            .render_scene_to_image(scene, extent)
    }
}

fn verify(renderer: Box<dyn PlatformHeadlessRenderer>) -> Result<()> {
    let renderer = Rc::new(RefCell::new(renderer));
    let replay = Rc::new(RefCell::new(Scene::default()));
    let mut cx = gpui::HeadlessAppContext::with_platform(
        Arc::new(gpui::NoopTextSystem::new()),
        Arc::new(()),
        {
            let renderer = renderer.clone();
            let replay = replay.clone();
            move || {
                Ok(Some(Box::new(Readback {
                    renderer: renderer.clone(),
                    replay: replay.clone(),
                })))
            }
        },
    );
    let blur = xgpui_shader::BackdropBlur::new(px(16.))?;
    let mut capture = |foreground| {
        let window = cx.open_window(size(px(96.), px(64.)), |_, cx| {
            cx.new(|_| BackdropScene {
                blur: blur.clone(),
                foreground,
            })
        })?;
        cx.simulate_window_scale_factor_change(window.into(), 1.);
        cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
        cx.capture_screenshot(window.into())
    };
    let reference = capture(false)?;
    let actual = capture(true)?;
    let extent = size(DevicePixels(96), DevicePixels(64));
    let replay = replay.borrow();
    ensure!(replay.backdrops.len() == 1, "expected one backdrop pass");
    let replayed = renderer
        .borrow_mut()
        .render_scene_to_image(&replay, extent)?;
    ensure!(
        actual == replayed,
        "retained replay changed the final image"
    );

    let sample = reference.get_pixel(44, 24).0;
    ensure!(
        (70..=185).contains(&sample[0]) && sample[0].abs_diff(sample[1]) <= 2,
        "expected active blur over the striped prefix, got {sample:?}"
    );
    ensure!(
        actual.get_pixel(44, 24).0 == sample,
        "the later opaque quad contaminated the backdrop halo"
    );
    ensure!(
        actual.get_pixel(56, 24).0 == [255, 0, 0, 255],
        "the later opaque quad must remain visible above the backdrop"
    );

    let mut reordered = Scene::default();
    for quad in &replay.quads {
        reordered.insert_primitive(*quad);
    }
    reordered.insert_primitive(gpui::Primitive::Backdrop(replay.backdrops[0].clone()));
    reordered.finish();
    let reordered = renderer
        .borrow_mut()
        .render_scene_to_image(&reordered, extent)?;
    ensure!(
        reordered.get_pixel(44, 24).0 != sample,
        "the check must detect a quad drawn early into the sampling halo"
    );
    Ok(())
}

#[::core::prelude::v1::test]
fn native_backdrop_reads_only_its_prefix_after_replay() -> Result<()> {
    verify(gpui_platform::current_headless_renderer()?.expect("native renderer"))
}

#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_backdrop_reads_only_its_prefix_after_replay() -> Result<()> {
    verify(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))
}
