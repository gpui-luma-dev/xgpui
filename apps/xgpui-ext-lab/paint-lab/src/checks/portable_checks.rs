use super::*;
use xgpui_shader::shader_paint::ShaderPaint;

#[derive(Debug)]
struct LogicalSource(ShaderPaint);
impl gpui::content::ContentSource for LogicalSource {
    fn describe(
        &self,
        cx: &mut gpui::content::ContentBuilder,
        request: &gpui::content::ContentRequest,
    ) -> std::result::Result<gpui::content::ImageRef, gpui::content::ContentError> {
        cx.source(
            &self.0.content(),
            &gpui::content::ContentRequest::new(size(px(160.), px(80.)), request.extent(), 0.5)?,
        )
    }
}
fn source(paint: ShaderPaint) -> Arc<dyn CustomContent> {
    gpui::content::ContentDraw::new(paint.content()).into_scene_content()
}

fn scene(content: Arc<dyn CustomContent>, clipped: bool, opacity: f32) -> Scene {
    let mut scene = base();
    let mut draw = record(rect(10., 20., 80., 40.), content, None);
    draw.corner_radii = Corners::default();
    draw.opacity = opacity;
    if clipped {
        draw.content_mask.bounds = rect(68., 35., 10., 10.);
    }
    scene.insert_primitive(draw);
    scene.finish();
    scene
}
fn check(pixel: [u8; 4], expected: [u8; 4]) -> Result<()> {
    ensure!(
        pixel
            .into_iter()
            .zip(expected)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "portable pixel {pixel:?}, expected {expected:?}"
    );
    Ok(())
}
pub(super) fn verify(renderer: &mut dyn PlatformHeadlessRenderer) -> Result<()> {
    let program = crate::portable::program()?;
    let paint = crate::portable::paint(&program, 0.25, 0.5)?;
    let extent = size(DevicePixels(128), DevicePixels(80));
    let original = scene(source(paint.clone()), false, 1.);
    let image = ready_image(renderer, &original, extent)?;
    // At local pixel (60.5,19.5), blend clamps to 1. The explicit
    // logical size gives accent=(.1,19.5/160,80/400), then alpha=.5.
    check(image.get_pixel(70, 39).0, [13, 16, 26, 255])?;
    let clipped = scene(source(paint.clone()), true, 0.5);
    let image = ready_image(renderer, &clipped, extent)?;
    check(image.get_pixel(70, 39).0, [6, 8, 13, 255])?;
    check(image.get_pixel(30, 25).0, [0, 0, 0, 255])?;
    // The same paint and raster extent with different logical geometry must
    // produce different cached output. No app-side OS/backend branching.
    let logical = scene(
        gpui::content::ContentDraw::new(gpui::content::Content::new(LogicalSource(paint.clone())))
            .into_scene_content(),
        false,
        1.,
    );
    let image = ready_image(renderer, &logical, extent)?;
    check(image.get_pixel(70, 39).0, [13, 31, 51, 255])?;
    let image = ready_image(renderer, &original, extent)?;
    check(image.get_pixel(70, 39).0, [13, 16, 26, 255])?;
    let next = crate::portable::paint(&program, 1., 1.)?;
    let image = ready_image(renderer, &scene(source(next), false, 1.), extent)?;
    check(image.get_pixel(70, 39).0, [26, 31, 51, 255])?;
    // Prove the old snapshot still renders after parameter edits.
    let image = ready_image(renderer, &original, extent)?;
    check(image.get_pixel(70, 39).0, [13, 16, 26, 255])?;
    // Unused uniforms must remain legal: the backend owns explicit bindings,
    // even when a shader compiler optimizes every user parameter away.
    let constant = xgpui_shader::shader_paint::ShaderProgram::new(
        "struct Parameters { ignored: f32 } fn paint(input: PaintInput) -> vec4<f32> { return vec4(0.25,0.5,0.75,1.); }",
    )?;
    let constant = constant.paint(gpui::content::UniformValues::new().float("ignored", 1.))?;
    let image = ready_image(renderer, &scene(source(constant), false, 1.), extent)?;
    check(image.get_pixel(70, 39).0, [64, 128, 191, 255])?;
    Ok(())
}
