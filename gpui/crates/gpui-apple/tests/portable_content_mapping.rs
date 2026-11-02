#![cfg(all(target_os = "macos", feature = "test-support"))]
use gpui_apple::metal_renderer::MetalHeadlessRenderer as Renderer;
fn new_renderer() -> Renderer {
    Renderer::new()
}
use gpui_apple::content_executor::{
    MetalContentLimits as ContentLimits, MetalContentTestGate as ContentGate,
};
include!("../../gpui/tests/support/content_mapping_pixels.rs");

#[test]
fn wide_color_mapping_keeps_linear_p3_values_and_letterboxing() {
    let (mut renderer, rx) = setup();
    renderer
        .set_color_mode(gpui::WindowColorMode::WideGamutSdr)
        .unwrap();
    let descriptor =
        ImageDescriptor::linear_p3_f16(ImageExtent::new(4, 2).unwrap(), AlphaMode::Premultiplied)
            .unwrap();
    let bytes: Vec<u8> = (0..8)
        .flat_map(|_| {
            [0x3400u16, 0, 0, 0x3800]
                .into_iter()
                .flat_map(u16::to_le_bytes)
        })
        .collect();
    let content = Content::image(ImageAsset::ready(
        ImagePixels::new(descriptor, bytes.into(), 64).unwrap(),
    ));
    let requested = scene(
        ContentDraw::new(content)
            .mapping(nearest(ImageFit::Contain))
            .into_scene_content(),
    );
    let extent = size(DevicePixels(TARGET), DevicePixels(TARGET));
    renderer
        .render_scene_to_linear_p3(&requested, extent)
        .unwrap();
    completed(&rx);
    let image = renderer
        .render_scene_to_linear_p3(&requested, extent)
        .unwrap();
    for y in 0..8 {
        for x in 0..8 {
            let pixel = image.pixels[((y + 2) * TARGET + x + 2) as usize];
            assert_eq!(
                pixel,
                if (2..6).contains(&y) {
                    [0x3400, 0, 0, 0x3c00]
                } else {
                    [0, 0, 0, 0x3c00]
                }
            );
        }
    }
    assert_eq!(renderer.content_presentation_stats().execution.readbacks, 0);
}
