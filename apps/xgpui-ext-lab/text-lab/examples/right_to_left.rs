//! The smallest app with right-to-left text on Parley, as shown on
//! xgpui.dev/right-to-left-text.html: plain GPUI text, whose direction comes
//! from the text, next to `typeset()` with an explicit right-to-left direction.
//!
//! Run: `just rtl-example`.
use anyhow::Context as _;
use gpui::{
    App, Application, Bounds, Context, SharedString, Window, WindowBounds, WindowOptions, div,
    prelude::*, px, rgb, size,
};
use parley::fontique::{Blob, Script};
use std::sync::Arc;
use xgpui_parley::{ParleyTextSystem, install_text_layout, typeset};

// The app's fonts: a Latin UI font and a font that covers Hebrew.
const FONTS: [&[u8]; 2] = [
    include_bytes!(
        "../../../../gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
    ),
    include_bytes!("../../../../extensions/crates/xgpui-parley/tests/fonts/NotoSansHebrew.ttf"),
];

const HEBREW: &str = "טקסט עברי ארוך עובר לשורה הבאה, והמילים הראשונות נשארות בשורה העליונה מימין.";
const MIXED: &str = "Release notes: גרסה 4.2 זמינה עכשיו";

fn main() -> anyhow::Result<()> {
    // 1. Parley as the text system, with a fallback font for Hebrew.
    let mut fonts = parley::FontContext::new();
    for data in FONTS {
        fonts
            .collection
            .register_fonts(Blob::new(Arc::new(data.to_vec())), None);
    }
    let hebrew = fonts
        .collection
        .family_id("Noto Sans Hebrew")
        .context("Noto Sans Hebrew is not registered")?;
    fonts
        .collection
        .append_fallbacks(Script::from_bytes(*b"Hebr"), std::iter::once(hebrew));
    let parley = Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans")?);

    let platform = gpui_platform::current_platform(false);
    Application::with_platform_and_text_system(platform, parley.clone()).run(
        move |cx: &mut App| {
            // 2. Lay out plain GPUI text with Parley (also installs `typeset()`).
            install_text_layout(parley.clone(), cx);

            let bounds = Bounds::centered(None, size(px(620.), px(460.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(|_| RightToLeft),
            )
            .expect("open window");
            cx.activate(true);
        },
    );
    Ok(())
}

struct RightToLeft;

impl Render for RightToLeft {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let heading = |text: &'static str| div().text_sm().text_color(rgb(0x6b7280)).child(text);
        let boxed = || div().w(px(260.)).p_2().bg(rgb(0xf3f4f6));
        div()
            .size_full()
            .p_6()
            .flex()
            .flex_col()
            .gap_3()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x111827))
            .font_family("IBM Plex Sans")
            .text_size(px(16.))
            // 3. Default: plain GPUI text. Each paragraph's direction comes
            //    from its first strong character.
            .child(heading("Default: plain GPUI text"))
            .child(boxed().child(HEBREW))
            .child(boxed().child(MIXED))
            .child(boxed().line_clamp(2).text_ellipsis().child(HEBREW))
            // 4. Right to left: `typeset()` sets the direction explicitly, for
            //    text that starts with Latin in a right-to-left UI.
            .child(heading("Right to left: typeset(…).rtl()"))
            .child(boxed().child(typeset(SharedString::from(MIXED)).rtl()))
    }
}
