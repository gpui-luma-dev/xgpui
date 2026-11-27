//! Real-window probe for the installed plain-text provider (#428).
use gpui::{
    Application, Context, Render, StyledText, TextLayout, Window, WindowOptions, div, prelude::*,
    px,
};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};
use xgpui_parley::{
    BaseDir, ParleyTextSystem, TextLayoutDirection, install_text_layout, set_text_layout_direction,
};

#[derive(Default)]
struct Frame {
    generation: usize,
    labels: Option<[TextLayout; 2]>,
}

struct Probe(Rc<RefCell<Frame>>);

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let labels = [StyledText::new("42"), StyledText::new("Luma הגדרות")];
        let mut frame = self.0.borrow_mut();
        frame.generation += 1;
        frame.labels = Some(labels.each_ref().map(|label| label.layout().clone()));
        div()
            .flex()
            .flex_col()
            .items_start()
            .children(labels.map(|label| {
                div()
                    .w(px(280.))
                    .text_left()
                    .font_family("IBM Plex Sans")
                    .child(label)
            }))
    }
}

fn main() -> anyhow::Result<()> {
    let plex = include_bytes!(
        "../../../gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
    );
    let hebrew =
        include_bytes!("../../../extensions/crates/xgpui-parley/tests/fonts/NotoSansHebrew.ttf");
    let mut fonts = parley::FontContext::new();
    for bytes in [plex.as_slice(), hebrew.as_slice()] {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(bytes.to_vec())), None);
    }
    let family = fonts
        .collection
        .family_id("Noto Sans Hebrew")
        .expect("Hebrew font");
    fonts.collection.append_fallbacks(
        parley::fontique::Script::from_bytes(*b"Hebr"),
        std::iter::once(family),
    );
    let parley = Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans")?);
    Application::with_platform_and_text_system(
        gpui_platform::current_platform(false),
        parley.clone(),
    )
    .run(move |cx| {
        install_text_layout(parley, cx);
        let frame = Rc::new(RefCell::new(Frame::default()));
        let _window = cx
            .open_window(WindowOptions::default(), |_, cx| {
                cx.new(|_| Probe(frame.clone()))
            })
            .expect("open direction probe window");
        cx.activate(true);
        cx.spawn(async move |cx| {
            let mut last_generation = 0;
            for (policy, expected) in [
                (TextLayoutDirection::Auto, [false, false]),
                (TextLayoutDirection::AutoOr(BaseDir::Rtl), [true, false]),
                (TextLayoutDirection::Fixed(BaseDir::Rtl), [true, true]),
                (TextLayoutDirection::Auto, [false, false]),
            ] {
                cx.update(|cx| set_text_layout_direction(policy, cx));
                let mut actual = None;
                for _ in 0..100 {
                    cx.background_executor()
                        .timer(Duration::from_millis(30))
                        .await;
                    let frame = frame.borrow();
                    if frame.generation > last_generation {
                        last_generation = frame.generation;
                        actual = frame.labels.as_ref().map(|labels| {
                            labels.each_ref().map(|layout| {
                                layout
                                    .position_for_index(0)
                                    .map(|start| start.x > layout.bounds().center().x)
                            })
                        });
                        break;
                    }
                }
                if actual != Some(expected.map(Some)) {
                    eprintln!("{policy:?}: expected fresh alignment {expected:?}, got {actual:?}");
                    std::process::exit(1);
                }
            }
            println!("Plain text direction window probe passed");
            cx.update(|cx| cx.quit());
        })
        .detach();
    });
    Ok(())
}
