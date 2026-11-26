use super::*;
use xgpui_shader::brush::Brush;

#[derive(Clone, Copy, Debug)]
enum PathPaint {
    Stock,
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
                let path = builder.build().unwrap();
                let clip = Bounds::new(point(px(12.), px(4.)), size(px(40.), px(48.)));
                window.with_content_mask(
                    Some(ContentMask { bounds: clip }),
                    |window| match paint {
                        PathPaint::Stock => window.paint_path(path, white()),
                        PathPaint::ShaderMask => brush.paint_path_native(window, path).unwrap(),
                        PathPaint::DirectBrush => brush.paint_path_direct(window, path).unwrap(),
                    },
                );
            },
        )
        .size_full()
    }
}

fn verify(
    create_renderer: impl Fn() -> Result<Option<Box<dyn PlatformHeadlessRenderer>>> + 'static,
) -> Result<()> {
    let mut cx = HeadlessAppContext::with_platform(
        Arc::new(NoopTextSystem::new()),
        Arc::new(()),
        create_renderer,
    );
    let brush = Brush::solid([1., 1., 1., 1.])?;
    for scale in [1., 1.5] {
        for paint in [
            PathPaint::Stock,
            PathPaint::ShaderMask,
            PathPaint::DirectBrush,
        ] {
            let window = cx.open_window(size(px(64.), px(56.)), |_, cx| {
                cx.new(|_| PathView {
                    paint,
                    brush: brush.clone(),
                })
            })?;
            cx.simulate_window_scale_factor_change(window.into(), scale);
            cx.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))?;
            let image = cx.capture_screenshot(window.into())?;
            for (x, y, expected) in [
                (16., 12., 255),
                (24., 24., 0),
                (10., 12., 0),
                (54., 12., 0),
                (16., 50., 0),
            ] {
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
