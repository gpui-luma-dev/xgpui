use super::*;
use std::{cell::Cell, rc::Rc};
use xgpui_shader::brush::Brush;

#[derive(Clone, Copy, Debug)]
enum PathPaint {
    Stock,
    EditedStock,
    CurvedStroke,
    ShaderMask,
    DirectBrush,
}

struct PathView {
    paint: PathPaint,
    brush: Brush,
}

impl Render for PathView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let paint = self.paint;
        let brush = self.brush.clone();
        canvas(
            |_, _, _| (),
            move |bounds, (), window, _| {
                window.paint_quad(fill(bounds, black()));
                let mut builder = PathBuilder::fill();
                for (left, top, right, bottom) in [(8., 8., 56., 48.), (20., 20., 36., 36.)] {
                    builder.move_to(point(px(left), px(top)));
                    builder.line_to(point(px(right), px(top)));
                    builder.line_to(point(px(right), px(bottom)));
                    builder.line_to(point(px(left), px(bottom)));
                    builder.close();
                }
                let mut path = builder.build().unwrap();
                if matches!(paint, PathPaint::EditedStock) {
                    // Keep the original bounds to catch a fallback that only
                    // looks for mutations of the public bounding box.
                    for vertex in &mut path.vertices {
                        vertex.xy_position.x += px(8.);
                    }
                }
                if matches!(paint, PathPaint::CurvedStroke) {
                    let mut stroke = PathBuilder::stroke(px(6.));
                    stroke.move_to(point(px(10.), px(30.)));
                    stroke.curve_to(point(px(54.), px(30.)), point(px(32.), px(2.)));
                    path = stroke.build().unwrap();
                }
                let clip = Bounds::new(point(px(12.), px(4.)), size(px(40.), px(48.)));
                window.with_content_mask(
                    Some(ContentMask { bounds: clip }),
                    |window| match paint {
                        PathPaint::Stock | PathPaint::EditedStock | PathPaint::CurvedStroke => {
                            window.paint_path(path, white());
                        }
                        PathPaint::ShaderMask => brush.paint_path_native(window, path).unwrap(),
                        PathPaint::DirectBrush => brush.paint_path_direct(window, path).unwrap(),
                    },
                );
            },
        )
        .size_full()
    }
}

struct Readback {
    renderer: Box<dyn PlatformHeadlessRenderer>,
    expected: Rc<Cell<PathPaint>>,
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
        match self.expected.get() {
            PathPaint::Stock | PathPaint::CurvedStroke => {
                ensure!(
                    scene.paths.len() == 1 && scene.paths[0].winding_tiles().is_some(),
                    "stock path should record winding tiles"
                );
            }
            PathPaint::EditedStock => {
                ensure!(
                    scene.paths.len() == 1 && scene.paths[0].winding_tiles().is_none(),
                    "edited vertex data must take the compatibility path"
                );
            }
            PathPaint::ShaderMask | PathPaint::DirectBrush => {
                ensure!(
                    scene.paths.is_empty() && scene.shader_draws.len() == 1,
                    "brush should retain its original shader draw"
                );
                let mask = scene.shader_draws[0].path_mask.as_ref();
                ensure!(
                    mask.is_some() == matches!(self.expected.get(), PathPaint::ShaderMask),
                    "unexpected brush coverage mode"
                );
                ensure!(
                    mask.is_none_or(|path| path.winding_tiles().is_none()),
                    "shader mask must preserve triangle coverage"
                );
            }
        }
        let image = self.renderer.render_scene_to_image(scene, extent)?;
        let mut replay = Scene::default();
        replay.replay(0..scene.len(), scene);
        replay.finish();
        let replayed = self.renderer.render_scene_to_image(&replay, extent)?;
        ensure!(
            image == replayed,
            "{:?} changed under retained replay",
            self.expected.get()
        );
        Ok(image)
    }
}

fn verify(
    create_renderer: impl Fn() -> Result<Option<Box<dyn PlatformHeadlessRenderer>>> + 'static,
) -> Result<()> {
    let expected = Rc::new(Cell::new(PathPaint::Stock));
    let mut cx =
        HeadlessAppContext::with_platform(Arc::new(NoopTextSystem::new()), Arc::new(()), {
            let expected = expected.clone();
            move || {
                Ok(create_renderer()?.map(|renderer| {
                    Box::new(Readback {
                        renderer,
                        expected: expected.clone(),
                    }) as _
                }))
            }
        });
    let brush = Brush::solid([1., 1., 1., 1.])?;
    for scale in [1., 1.5] {
        for paint in [
            PathPaint::Stock,
            PathPaint::EditedStock,
            PathPaint::CurvedStroke,
            PathPaint::ShaderMask,
            PathPaint::DirectBrush,
        ] {
            expected.set(paint);
            let window = cx.open_window(size(px(64.), px(56.)), |_, cx| {
                cx.new(|_| PathView {
                    paint,
                    brush: brush.clone(),
                })
            })?;
            cx.simulate_window_scale_factor_change(window.into(), scale);
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
            let image = cx.capture_screenshot(window.into())?;
            let samples = match paint {
                PathPaint::CurvedStroke => [
                    (32., 16., 255),
                    (32., 24., 0),
                    (10., 30., 0),
                    (54., 30., 0),
                    (16., 50., 0),
                ],
                PathPaint::EditedStock => [
                    (20., 12., 255),
                    (24., 24., 255),
                    (32., 24., 0),
                    (14., 12., 0),
                    (54., 12., 0),
                ],
                _ => [
                    (16., 12., 255),
                    (24., 24., 0),
                    (10., 12., 0),
                    (54., 12., 0),
                    (16., 50., 0),
                ],
            };
            for (x, y, expected) in samples {
                let pixel = image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
                assert_eq!(
                    pixel,
                    [expected, expected, expected, 255],
                    "{paint:?} at {scale}x, ({x}, {y})"
                );
            }
        }
    }
    Ok(())
}

#[::core::prelude::v1::test]
fn native_path_paints_preserve_hole_and_clip() -> Result<()> {
    verify(gpui_platform::current_headless_renderer)
}

#[cfg(feature = "wgpu")]
#[::core::prelude::v1::test]
fn wgpu_path_paints_preserve_hole_and_clip() -> Result<()> {
    verify(|| Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))))
}
