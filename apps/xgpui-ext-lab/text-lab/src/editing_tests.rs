//! Same editor and GPUI input bridge, backed by the actual text engines.
use super::{input, samples};
use gpui::{
    Bounds, ElementInputHandler, Focusable, InputHandler, Modifiers, MouseButton,
    PlatformTextSystem, TestAppContext, TestDispatcher, VisualTestContext, div, point, prelude::*,
    px, size,
};
use std::{borrow::Cow, sync::Arc};

fn editing_checks(backend: Arc<dyn PlatformTextSystem>) {
    backend
        .add_fonts(vec![Cow::Borrowed(samples::PLEX)])
        .unwrap();
    let mut cx = TestAppContext::build_with_text_system(TestDispatcher::new(0), None, backend);
    cx.update(input::bind_keys);
    let window = cx.open_window(size(px(400.), px(80.)), |_, cx| {
        input::TextInput::new("alpha beta", cx)
    });
    let editor = window.root(&mut cx).unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), &cx);
    visual.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    let draw = |visual: &mut VisualTestContext| {
        visual.draw(
            point(px(100.), px(100.)),
            size(px(400.), px(80.)),
            |_, _| {
                div()
                    .w(px(400.))
                    .font_family("IBM Plex Sans")
                    .child(editor.clone())
            },
        );
    };
    draw(&mut visual);
    let frame = visual.debug_bounds("example-text-input").unwrap();
    let bounds = Bounds::new(
        frame.origin + point(px(4.), px(4.)),
        size(frame.size.width - px(8.), px(30.)),
    );
    visual.simulate_click(
        point(bounds.left() + px(1.), bounds.center().y),
        Modifiers::default(),
    );
    visual.simulate_keystrokes("right shift-right");
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        assert_eq!(
            handler
                .selected_text_range(false, window, cx)
                .unwrap()
                .range,
            1..2
        );
    });
    // Drag from the beginning to the space after "alpha" using shaped geometry.
    let end = visual.update(|window, cx| {
        ElementInputHandler::new(bounds, editor.clone())
            .bounds_for_range(6..6, window, cx)
            .unwrap()
            .left()
    });
    visual.simulate_mouse_down(
        point(bounds.left(), bounds.center().y),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.simulate_mouse_move(
        point(end, bounds.center().y),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.simulate_mouse_up(
        point(end, bounds.center().y),
        MouseButton::Left,
        Modifiers::default(),
    );
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        assert_eq!(
            handler
                .selected_text_range(false, window, cx)
                .unwrap()
                .range,
            0..6
        );
    });
    visual.simulate_keystrokes(if cfg!(target_os = "macos") {
        "cmd-c"
    } else {
        "ctrl-c"
    });
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("alpha ")
    );
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        let caret = handler.bounds_for_range(6..6, window, cx).unwrap();
        assert!(caret.left() > bounds.left());
        assert_eq!(
            handler.character_index_for_point(point(caret.left(), bounds.center().y), window, cx),
            Some(6)
        );
        handler.set_selected_text_range(6..10, window, cx);
        assert_eq!(
            handler
                .selected_text_range(false, window, cx)
                .unwrap()
                .range,
            6..10
        );
        // Composition replaces a nonempty selection after a prefix. Its selected
        // range is relative to the inserted UTF-16 text, including a surrogate pair.
        handler.replace_and_mark_text_in_range(None, "𝄞e\u{301}", Some(2..4), window, cx);
        assert_eq!(handler.marked_text_range(window, cx), Some(6..10));
        assert_eq!(
            handler.character_index_for_point(bounds.center(), window, cx),
            None
        );
        assert_eq!(
            handler
                .selected_text_range(false, window, cx)
                .unwrap()
                .range,
            8..10
        );
    });
    // Paint the actual marked text/underline before the IME commits it.
    draw(&mut visual);
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        handler.replace_text_in_range(None, "é", window, cx);
        assert_eq!(handler.marked_text_range(window, cx), None);
        assert_eq!(
            handler
                .selected_text_range(false, window, cx)
                .unwrap()
                .range,
            7..7
        );
        assert_eq!(
            handler
                .text_for_range(0..100, &mut None, window, cx)
                .as_deref(),
            Some("alpha é")
        );
        handler.replace_and_mark_text_in_range(None, "a", Some(1..1), window, cx);
        handler.unmark_text(window, cx);
        assert_eq!(handler.marked_text_range(window, cx), None);
        handler.replace_text_in_range(Some(0..100), "a\nb", window, cx);
        assert_eq!(
            handler
                .text_for_range(0..100, &mut None, window, cx)
                .as_deref(),
            Some("a b")
        );
        handler.replace_text_in_range(Some(0..100), "e\u{301}", window, cx);
    });
    // Candidate replacement/commit/cancel through the IME bridge. This checks
    // the callbacks and geometry, not the operating system's candidate window.
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        handler.replace_text_in_range(Some(0..100), "prefix ", window, cx);
        handler.replace_and_mark_text_in_range(None, "にほん", Some(0..3), window, cx);
        assert_eq!(handler.marked_text_range(window, cx), Some(7..10));
    });
    draw(&mut visual);
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        let anchor = handler.bounds_for_range(7..10, window, cx).unwrap();
        assert!(anchor.left() >= bounds.left() && anchor.right() <= bounds.right());
        // A different candidate replaces the marked span, not the prefix.
        handler.replace_and_mark_text_in_range(None, "二本", Some(0..2), window, cx);
        assert_eq!(handler.marked_text_range(window, cx), Some(7..9));
        assert_eq!(
            handler
                .text_for_range(0..100, &mut None, window, cx)
                .as_deref(),
            Some("prefix 二本")
        );
        handler.replace_text_in_range(None, "日本", window, cx);
        assert_eq!(handler.marked_text_range(window, cx), None);
        assert_eq!(
            handler
                .text_for_range(0..100, &mut None, window, cx)
                .as_deref(),
            Some("prefix 日本")
        );
        handler.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx);
        // An IME cancellation that clears preedit must retain committed text.
        handler.replace_and_mark_text_in_range(None, "", Some(0..0), window, cx);
        assert_eq!(handler.marked_text_range(window, cx), None);
        assert_eq!(
            handler
                .text_for_range(0..100, &mut None, window, cx)
                .as_deref(),
            Some("prefix 日本")
        );
        assert_eq!(
            handler
                .selected_text_range(false, window, cx)
                .unwrap()
                .range,
            9..9
        );
        handler.replace_text_in_range(Some(0..100), "e\u{301}", window, cx);
    });
    // The existing grapheme-aware deletion must delete a combining sequence once.
    draw(&mut visual);
    visual.simulate_keystrokes("backspace");
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        assert_eq!(
            handler
                .text_for_range(0..100, &mut None, window, cx)
                .as_deref(),
            Some("")
        );
        handler.replace_text_in_range(None, &"long text ".repeat(30), window, cx);
    });
    draw(&mut visual);
    visual.update(|window, cx| {
        let mut handler = ElementInputHandler::new(bounds, editor.clone());
        let caret = handler.bounds_for_range(300..300, window, cx).unwrap();
        assert!(caret.left() >= bounds.left() && caret.right() <= bounds.right());
    });
}

#[test]
fn parley_editing_and_ime_bridge() {
    let mut fonts = parley::FontContext::new();
    fonts.collection.register_fonts(
        parley::fontique::Blob::new(Arc::new(samples::PLEX.to_vec())),
        None,
    );
    editing_checks(Arc::new(
        xgpui_parley::ParleyTextSystem::new(fonts, "IBM Plex Sans").unwrap(),
    ));
}

#[cfg(target_os = "macos")]
#[test]
fn native_editing_and_ime_bridge() {
    editing_checks(Arc::new(gpui_apple::AppleTextSystem::new()));
}
