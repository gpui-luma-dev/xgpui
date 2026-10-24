use anyhow::Result;
use gpui::{FontId, px};
use parley::{Alignment, AlignmentOptions, FontContext, Layout, LayoutContext, StyleProperty};
use std::sync::Arc;
use xgpui_parley::line_layout;

fn shape(text: &str, width: Option<f32>) -> Layout<()> {
    let mut fonts = FontContext::new();
    fonts.collection.register_fonts(
        parley::fontique::Blob::new(Arc::new(
            include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../../../gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
            ))
            .to_vec(),
        )),
        None,
    );
    let mut context = LayoutContext::new();
    let mut builder = context.ranged_builder(&mut fonts, text, 1.0, true);
    builder.push_default(StyleProperty::FontSize(16.0));
    builder.push_default(StyleProperty::FontFamily(parley::FontFamily::from(
        "IBM Plex Sans",
    )));
    let mut layout = builder.build(text);
    layout.break_all_lines(width);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

#[test]
fn preserves_positions_clusters_and_resolved_face() -> Result<()> {
    let text = "office e\u{301} café";
    let layout = shape(text, None);
    let mut registered = 0;
    let converted = line_layout(&layout, px(16.0), |run| {
        assert!(!run.font().data.is_empty());
        registered += 1;
        Ok(FontId(registered))
    })?;
    assert_eq!(converted.runs.len(), registered);
    assert_eq!(converted.len, text.len());
    let line = layout.lines().next().unwrap();
    assert_eq!(f32::from(converted.width), line.metrics().advance);
    assert!(f32::from(converted.ascent) > 0.0);

    let actual: Vec<_> = converted.runs.iter().flat_map(|run| &run.glyphs).collect();
    // Independently use Parley's positioned iterator to catch double baselines
    // and missing run offsets in the GPUI conversion.
    let mut expected = Vec::new();
    for item in line.items() {
        if let parley::PositionedLayoutItem::GlyphRun(run) = item {
            expected.extend(run.positioned_glyphs());
        }
    }
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual.id.0, expected.id);
        assert!((f32::from(actual.position.x) - expected.x).abs() < 0.001);
        assert!(
            (f32::from(actual.position.y) - (expected.y - line.metrics().baseline)).abs() < 0.001
        );
        assert!(text.is_char_boundary(actual.index));
    }
    let indices: Vec<_> = line
        .runs()
        .flat_map(|run| {
            run.visual_clusters()
                .flat_map(|cluster| cluster.glyphs().map(move |_| cluster.text_range().start))
        })
        .collect();
    assert_eq!(
        actual.iter().map(|glyph| glyph.index).collect::<Vec<_>>(),
        indices
    );
    for (index, run) in converted.runs.iter().enumerate() {
        assert_eq!(run.font_id, FontId(index + 1));
    }
    Ok(())
}

#[test]
fn rejects_wrapping_before_registering_faces() {
    let layout = shape("one two three four", Some(30.0));
    let result = line_layout(&layout, px(16.0), |_| panic!("must not register a face"));
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("one unwrapped line")
    );
}

#[test]
fn rejects_rtl_before_registering_faces() {
    let layout = shape("שלום", None);
    let result = line_layout(&layout, px(16.0), |_| panic!("must not register a face"));
    assert!(result.unwrap_err().to_string().contains("RTL"));
}

#[test]
fn propagates_face_registration_failure() {
    let layout = shape("hello", None);
    let result = line_layout(&layout, px(16.0), |_| anyhow::bail!("unsupported face"));
    assert_eq!(result.unwrap_err().to_string(), "unsupported face");
}
