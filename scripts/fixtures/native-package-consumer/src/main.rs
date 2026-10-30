use gpui::prelude::*;
use std::hint::black_box;

#[derive(gpui_macros::IntoElement)]
struct ConsumerView;

impl gpui::RenderOnce for ConsumerView {
    fn render(self, _: &mut gpui::Window, _: &mut gpui::App) -> impl gpui::IntoElement {
        gpui::div().child("isolated consumer")
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    black_box(ConsumerView);
    black_box(gpui::size(gpui::px(320.), gpui::px(240.)));
    let color = xgpui_color::EncodedRgb::new(xgpui_color::RgbSpace::Srgb, [0.2, 0.5, 0.8])?;
    black_box(color.decode()?.to_oklab()?);
    let fill = xgpui_shader::Brush::solid([0.2, 0.5, 0.8, 1.])?;
    black_box(
        xgpui_shader::painted()
            .bg_brush(fill)
            .child("isolated package consumer"),
    );
    black_box(xgpui_gamut::WideColor::linear_p3([0.2, 0.5, 0.8, 1.])?);
    let _ = black_box(xgpui_media::ImageFilter::new().brightness(1.1));
    let adobe = xgpui_media::pipeline::ImageColor::ADOBE_RGB_1998;
    black_box(xgpui_media::pipeline::rgba16f_data_with_mapping(
        1,
        1,
        vec![[0., 1., 1., 0.5]],
        adobe,
        xgpui_color::GamutMapping::ReduceChroma,
    )?);
    black_box(
        xgpui_media::MediaLoader::default().load_rgba16f_with_mapping(
            1,
            1,
            vec![[0., 1., 0., 1.]],
            adobe,
            xgpui_color::GamutMapping::Clip,
        )?,
    );
    let rec2020 = xgpui_media::pipeline::rec2020_rgba16f_data(
        1,
        1,
        vec![[0., 1., 0., 1.]],
        adobe,
        xgpui_color::GamutMapping::Clip,
    )?;
    assert_eq!(
        rec2020.format(),
        gpui::ImageTextureFormat::LinearRec2020Rgba16Float
    );
    black_box(xgpui_media::MediaLoader::default().load_rec2020_rgba16f(
        1,
        1,
        vec![[0., 1., 1., 0.5]],
        adobe,
        xgpui_color::GamutMapping::Clip,
    )?);
    black_box(xgpui_svg::SvgScheduler::default());
    black_box(std::mem::size_of::<xgpui_parley::ParleyTextSystem>());
    black_box(std::mem::size_of::<gpui_apple::AppleDispatcher>());
    black_box(std::mem::size_of::<gpui_macos::MacPlatform>());
    black_box(gpui_wgpu::wgpu::Features::empty());
    black_box(gpui_platform::application as fn() -> gpui::Application);
    println!("Native package consumer passed without creating an application or window.");
    Ok(())
}
