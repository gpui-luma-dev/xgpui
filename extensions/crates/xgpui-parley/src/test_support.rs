use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, StyleProperty};
use std::sync::Arc;

pub(crate) const PLEX: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
));
pub(crate) const INTER: &[u8] = include_bytes!("../tests/fonts/InterVariable.ttf");

pub(crate) fn fonts() -> FontContext {
    let mut fonts = FontContext::new();
    for data in [INTER, PLEX] {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
    }
    fonts
}

pub(crate) fn shape(fonts: &mut FontContext, text: &str, family: &str, weight: f32) -> Layout<()> {
    let mut context = LayoutContext::new();
    let mut builder = context.ranged_builder(fonts, text, 1.0, true);
    builder.push_default(StyleProperty::FontFamily(parley::FontFamily::from(family)));
    builder.push_default(StyleProperty::FontSize(16.0));
    builder.push_default(StyleProperty::FontWeight(parley::FontWeight::new(weight)));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}
