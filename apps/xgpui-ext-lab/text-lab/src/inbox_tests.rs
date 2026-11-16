//! The multilingual inbox (#355) on Parley and, on macOS, the native backend.
use super::{inbox, paragraph, samples};
use gpui::{
    Entity, Modifiers, PlatformTextSystem, TestAppContext, TestDispatcher, VisualTestContext, div,
    point, prelude::*, px,
};
use std::sync::Arc;
use xgpui_parley::ParleyTextSystem;

const WINDOW: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(1000.),
    height: px(900.),
};

fn parley() -> Arc<ParleyTextSystem> {
    let mut fonts = parley::FontContext::new();
    for data in std::iter::once(samples::PLEX).chain(paragraph::FONTS) {
        fonts
            .collection
            .register_fonts(parley::fontique::Blob::new(Arc::new(data.to_vec())), None);
    }
    Arc::new(ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap())
}

#[cfg(target_os = "macos")]
fn native() -> Arc<dyn PlatformTextSystem> {
    let backend: Arc<dyn PlatformTextSystem> = Arc::new(gpui_apple::AppleTextSystem::new());
    backend
        .add_fonts(
            std::iter::once(samples::PLEX)
                .chain(paragraph::FONTS)
                .map(std::borrow::Cow::Borrowed)
                .collect(),
        )
        .unwrap();
    backend
}

struct Inbox {
    lab: Entity<inbox::InboxLab>,
    visual: VisualTestContext,
}

impl Inbox {
    fn open(
        backend: Arc<dyn PlatformTextSystem>,
        parley: Option<Arc<ParleyTextSystem>>,
        adjust: impl FnOnce(&mut inbox::InboxLab),
    ) -> Self {
        let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
        let window = cx.open_window(WINDOW, |_, cx| {
            let mut lab = inbox::InboxLab::new("test".into(), parley, cx);
            adjust(&mut lab);
            lab
        });
        let lab = window.root(&mut cx).unwrap();
        let visual = VisualTestContext::from_window(window.into(), &cx);
        let mut inbox = Self { lab, visual };
        inbox.draw();
        inbox
    }

    fn draw(&mut self) {
        let lab = self.lab.clone();
        self.visual.draw(point(px(0.), px(0.)), WINDOW, |_, _| {
            div().size_full().child(lab)
        });
    }

    fn bounds(&mut self, selector: &str) -> gpui::Bounds<gpui::Pixels> {
        self.visual
            .debug_bounds(selector.to_owned().leak())
            .unwrap_or_else(|| panic!("{selector} not drawn"))
    }

    fn click(&mut self, selector: &str) {
        let center = self.bounds(selector).center();
        self.visual.simulate_click(center, Modifiers::default());
        self.draw();
    }

    fn body(&mut self) -> gpui::TextLayout {
        self.visual
            .update(|_, cx| self.lab.read(cx).body_layout.clone())
            .expect("message body drawn")
    }
}

/// Every message, the list and the message view are drawn; the UI-language
/// buttons mirror the layout and back; selecting rows switches messages.
fn layout_checks(mut inbox: Inbox) -> Inbox {
    for ix in 0..inbox::MESSAGES.len() {
        let row = inbox.bounds(&format!("inbox-row-{ix}"));
        assert!(row.size.height > px(30.), "row {ix}");
    }
    let list = inbox.bounds("inbox-list");
    let message = inbox.bounds("inbox-message");
    assert!(list.left() < message.left(), "English: list on the left");

    inbox.click("inbox-ui-1");
    inbox
        .visual
        .update(|_, cx| assert_eq!(inbox.lab.read(cx).ui(), inbox::UiLanguage::Hebrew));
    let list = inbox.bounds("inbox-list");
    let message = inbox.bounds("inbox-message");
    assert!(list.left() > message.left(), "Hebrew: list on the right");

    inbox.click("inbox-ui-0");
    assert!(inbox.bounds("inbox-list").left() < inbox.bounds("inbox-message").left());

    inbox.click("inbox-row-2");
    inbox
        .visual
        .update(|_, cx| assert_eq!(inbox.lab.read(cx).selected(), 2));
    inbox
}

#[test]
fn inbox_on_parley_wraps_in_logical_order_mirrors_and_hit_tests() {
    let parley = parley();
    let mut inbox = layout_checks(Inbox::open(parley.clone(), Some(parley.clone()), |_| {}));

    // The first message is Hebrew: its first words start the first line, at
    // the right edge of the paragraph.
    inbox.click("inbox-row-0");
    let body = inbox.body();
    let bounds = body.bounds();
    assert!(body.line_layouts().is_empty(), "laid out by Parley");
    let first = body.position_for_index(0).unwrap();
    assert_eq!(first.y, bounds.top());
    assert!(first.x > bounds.center().x, "{first:?} in {bounds:?}");
    assert!(bounds.size.height > px(40.), "wraps");

    // A truncated right-to-left preview keeps one line.
    let preview = inbox.bounds("inbox-preview-0");
    assert!(preview.size.height < px(24.), "{preview:?}");

    // Clicking the link in the release message reaches the link.
    inbox.click("inbox-row-4");
    let text = inbox::MESSAGES[4].body.split("\n\n").next().unwrap();
    let range = inbox::clickable_ranges(text)[0].clone();
    let body = inbox.body();
    let at = body.position_for_index(range.start + 3).unwrap() + point(px(1.), px(8.));
    inbox.visual.simulate_click(at, Modifiers::default());
    inbox.draw();
    inbox.visual.update(|_, cx| {
        assert_eq!(
            inbox.lab.read(cx).clicked().map(|target| target.as_ref()),
            Some(&text[range.clone()])
        )
    });

    // Switched to GPUI lines, plain text is back on flat lines.
    let mut flat = Inbox::open(parley.clone(), Some(parley), |lab| {
        lab.set_plain_on_parley(false)
    });
    assert!(!flat.body().line_layouts().is_empty(), "flat lines");
}

#[cfg(target_os = "macos")]
#[test]
fn inbox_on_native_lays_out_and_mirrors() {
    let mut inbox = layout_checks(Inbox::open(native(), None, |_| {}));
    assert!(!inbox.body().line_layouts().is_empty(), "native flat lines");
}

#[test]
fn clickable_ranges_find_mentions_and_links() {
    let text = "Hi @Noa, see https://xgpui.dev/a.html. Thanks @Sara!";
    let found: Vec<_> = inbox::clickable_ranges(text)
        .into_iter()
        .map(|range| &text[range])
        .collect();
    assert_eq!(found, ["@Noa", "https://xgpui.dev/a.html", "@Sara"]);
    assert!(inbox::clickable_ranges("שלום עולם").is_empty());
}
