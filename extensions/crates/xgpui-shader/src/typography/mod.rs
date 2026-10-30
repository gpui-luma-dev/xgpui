//! Selectable read-only text with paint-only typewriter, wave and scramble snapshots.
//!
//! Single-line LTR text only; wrapping and bidirectional selection
//! are intentionally deferred. Source text is never modified by interaction.
mod motion;
mod render;
#[cfg(feature = "portable-shaders")]
mod style;
#[cfg(feature = "portable-shaders")]
pub use style::TextShadow;
mod scramble;
mod selection;
use anyhow::{Result, ensure};
use gpui::{prelude::*, *};
pub use motion::KineticEffect;
use selection::{Selection, caret_positions};
use std::ops::Range;
use std::{cell::RefCell, rc::Rc};
use unicode_bidi::{BidiClass, bidi_class};
use unicode_segmentation::UnicodeSegmentation;

/// Retained selection/focus state for a single-line read-only text element.
/// Construct with `cx.new(|cx| SelectableText::new(text, cx).unwrap())` and retain
/// the entity across renders. Font, size, color and line height are inherited.
pub struct SelectableText {
    text: SharedString,
    selection: Selection,
    focus: FocusHandle,
    selecting: bool,
    geometry: Option<Geometry>,
    scroll_x: Pixels,
    accessible_run: Rc<RefCell<accesskit::Node>>,
    effect: KineticEffect,
    scramble_alphabets: Vec<Rc<scramble::Alphabet>>,
    #[cfg(feature = "portable-shaders")]
    fill: Option<std::sync::Arc<dyn GlyphBrush>>,
    #[cfg(feature = "portable-shaders")]
    shadow: Option<TextShadow>,
}
struct Geometry {
    bounds: Bounds<Pixels>,
    carets: Vec<f32>,
}
impl SelectableText {
    /// Rejects line breaks, bidi controls and RTL characters instead of exposing
    /// incorrect hit testing through GPUI's flat-LTR line layout APIs.
    pub fn new(text: impl Into<SharedString>, cx: &mut Context<Self>) -> Result<Self> {
        let text = text.into();
        validate(&text)?;
        let accessible_run = Rc::new(RefCell::new(accessible_run(&text)));
        Ok(Self {
            accessible_run,
            effect: KineticEffect::Static,
            scramble_alphabets: Vec::new(),
            #[cfg(feature = "portable-shaders")]
            fill: None,
            #[cfg(feature = "portable-shaders")]
            shadow: None,
            selection: Selection::new(&text),
            text,
            focus: cx.focus_handle(),
            selecting: false,
            geometry: None,
            scroll_x: px(0.),
        })
    }
    /// Update the visual snapshot without changing text, selection or canonical carets.
    /// The host supplies time/progress. `App::reduce_motion()` paints the static source.
    pub fn set_effect(&mut self, effect: KineticEffect, cx: &mut Context<Self>) -> Result<()> {
        effect.validate()?;
        if self.effect != effect {
            self.effect = effect;
            cx.notify();
        }
        Ok(())
    }
    /// Retain a glyph-atlas brush, anchored to the full canonical source line.
    #[cfg(feature = "portable-shaders")]
    pub fn set_fill(&mut self, fill: Option<crate::Brush>, cx: &mut Context<Self>) -> Result<()> {
        self.fill = fill.map(crate::Brush::text_brush).transpose()?;
        cx.notify();
        Ok(())
    }
    #[cfg(feature = "portable-shaders")]
    pub fn set_shadow(&mut self, shadow: Option<TextShadow>, cx: &mut Context<Self>) {
        self.shadow = shadow;
        cx.notify();
    }
    fn shadow_padding(&self) -> Pixels {
        #[cfg(feature = "portable-shaders")]
        {
            self.shadow
                .as_ref()
                .map(TextShadow::padding)
                .unwrap_or(px(0.))
        }
        #[cfg(not(feature = "portable-shaders"))]
        {
            px(0.)
        }
    }
    pub fn effect(&self) -> KineticEffect {
        self.effect
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    /// Selected UTF-8 byte range, always at extended-grapheme boundaries.
    pub fn selection(&self) -> Range<usize> {
        self.selection.range()
    }
    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.selection.anchor = 0;
        self.selection.focus = self.text.len();
        cx.notify();
    }
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.move_to(self.selection.focus, false);
        cx.notify();
    }
    /// Copies authentic source text. Empty selections leave the clipboard alone.
    pub fn copy_selection(&self, cx: &mut Context<Self>) -> bool {
        if self.selection().is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text().into()));
        true
    }
    fn index_at(&self, point: Point<Pixels>) -> usize {
        let Some(g) = &self.geometry else {
            return 0;
        };
        let x = f32::from(point.x - g.bounds.left() + self.scroll_x);
        let i = g
            .carets
            .windows(2)
            .position(|pair| x < (pair[0] + pair[1]) / 2.)
            .unwrap_or(g.carets.len() - 1);
        self.selection.boundaries[i]
    }
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let offset = self.index_at(event.position);
        self.selecting = true;
        if event.click_count >= 3 {
            self.select_all(cx);
        } else if event.click_count == 2 {
            self.selection.select_word(&self.text, offset);
        } else {
            self.selection.move_to(offset, event.modifiers.shift);
        }
        cx.stop_propagation();
        cx.notify();
    }
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke.key;
        let m = event.keystroke.modifiers;
        let shortcut = if cfg!(target_os = "macos") {
            m.platform
        } else {
            m.control || m.platform
        };
        let word = if cfg!(target_os = "macos") {
            m.alt
        } else if cfg!(target_family = "wasm") {
            m.alt || m.control
        } else {
            m.control
        };
        match key.as_str() {
            "a" if shortcut && !m.alt => self.select_all(cx),
            "c" if shortcut && !m.alt => {
                self.copy_selection(cx);
            }
            "left" | "right" => {
                let forward = key == "right";
                if m.platform {
                    self.selection
                        .move_to(if forward { self.text.len() } else { 0 }, m.shift);
                } else if word {
                    self.selection.word(&self.text, forward, m.shift);
                } else {
                    self.selection.step(forward, m.shift);
                }
            }
            "home" | "end" => self
                .selection
                .move_to(if key == "end" { self.text.len() } else { 0 }, m.shift),
            "escape" => self.clear_selection(cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
fn validate(text: &str) -> Result<()> {
    ensure!(
        !text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}')),
        "SelectableText requires one line without tabs"
    );
    ensure!(
        !text.chars().any(|c| matches!(
            bidi_class(c),
            BidiClass::R
                | BidiClass::AL
                | BidiClass::AN
                | BidiClass::LRE
                | BidiClass::RLE
                | BidiClass::LRO
                | BidiClass::RLO
                | BidiClass::PDF
                | BidiClass::LRI
                | BidiClass::RLI
                | BidiClass::FSI
                | BidiClass::PDI
        )),
        "SelectableText milestone one supports left-to-right text without bidi controls"
    );
    ensure!(
        text.graphemes(true).all(|g| g.len() <= u8::MAX as usize),
        "a selectable grapheme must fit AccessKit character length metadata (255 UTF-8 bytes)"
    );
    Ok(())
}
impl Focusable for SelectableText {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Render for SelectableText {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selection = self.selection.clone();
        let accessible_run = self.accessible_run.clone();
        let entity = cx.entity();
        let run_id = std::rc::Rc::new(std::cell::Cell::new(None));
        let action_run_id = run_id.clone();
        div().id("selectable-text").w_full().min_w_0().px_3().py_3()
            .border_1().border_color(rgb(0x60738a)).rounded_md()
            .focus(|style| style.border_color(rgb(0x56dac8)))
            .track_focus(&self.focus).tab_index(0).cursor(CursorStyle::IBeam)
            .role(Role::TextInput).aria_label("Selectable text").aria_value(self.text.clone())
            .aria_description("Read-only text. Shift with arrows selects. Select all and copy use your system shortcuts.")
            .a11y_synthetic_children(move |builder| {
                let id = builder.synthetic_node_id("text");
                run_id.set(Some(id));
                let run = accessible_run.borrow().clone();
                builder.push_child(id, run);
                let parent = builder.parent_node();
                parent.set_read_only();
                parent.set_text_selection(accesskit::TextSelection {
                    anchor: accesskit::TextPosition { node: id, character_index: selection.boundaries.binary_search(&selection.anchor).unwrap() },
                    focus: accesskit::TextPosition { node: id, character_index: selection.boundaries.binary_search(&selection.focus).unwrap() },
                });
            })
            .on_a11y_action(AccessibleAction::SetTextSelection, {
                let entity = entity.clone();
                move |data, _, cx| {
                    if let Some(accesskit::ActionData::SetTextSelection(s)) = data {
                        entity.update(cx, |this, cx| {
                            // Both endpoints must address this element's synthetic text run.
                            if Some(s.anchor.node) != action_run_id.get() || Some(s.focus.node) != action_run_id.get() { return; }
                            let byte = |n| this.selection.boundaries.get(n).copied();
                            if let (Some(a), Some(f)) = (byte(s.anchor.character_index), byte(s.focus.character_index)) {
                                this.selection.move_to(a, false); this.selection.move_to(f, true); cx.notify();
                            }
                        });
                    }
                }
            })
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_key_down(cx.listener(Self::key_down))
            .child(TextLine { entity })
    }
}
fn accessible_run(text: &str) -> accesskit::Node {
    let mut node = accesskit::Node::new(Role::TextRun);
    node.set_value(text.to_owned());
    node.set_character_lengths(
        text.graphemes(true)
            .map(|g| g.len() as u8)
            .collect::<Vec<_>>(),
    );
    node.set_text_direction(accesskit::TextDirection::LeftToRight);
    node
}
struct TextLine {
    entity: Entity<SelectableText>,
}
impl IntoElement for TextLine {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for TextLine {
    type RequestLayoutState = ();
    type PrepaintState = ShapedLine;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height()
            + self.entity.read(cx).effect.padding() * 2.
            + self.entity.read(cx).shadow_padding() * 2.)
            .into();
        (window.request_layout(style, [], cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> ShapedLine {
        let halo = self.entity.read(cx).shadow_padding();
        let bounds = Bounds::new(
            bounds.origin + point(halo, halo),
            size(
                (bounds.size.width - halo * 2.).max(px(0.)),
                (bounds.size.height - halo * 2.).max(px(0.)),
            ),
        );
        let text = self.entity.read(cx).text.clone();
        let style = window.text_style();
        let run = TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window.text_system().shape_line(
            text,
            style.font_size.to_pixels(window.rem_size()),
            &[run],
            None,
        );
        self.entity.update(cx, |this, _| {
            if matches!(this.effect, KineticEffect::Scramble { .. }) {
                this.scramble_alphabets.retain(|alphabet| {
                    alphabet.size == line.font_size
                        && line.runs.iter().any(|run| run.font_id == alphabet.font)
                });
                for run in &line.runs {
                    if !this
                        .scramble_alphabets
                        .iter()
                        .any(|alphabet| alphabet.font == run.font_id)
                    {
                        this.scramble_alphabets
                            .push(Rc::new(scramble::Alphabet::prepare(
                                run.font_id,
                                line.font_size,
                                window,
                            )));
                    }
                }
            }
            let mut anchors = vec![(0, 0.)];
            for glyph in line.runs.iter().flat_map(|run| &run.glyphs) {
                if this
                    .selection
                    .boundaries
                    .binary_search(&glyph.index)
                    .is_ok()
                    && glyph.index > anchors.last().unwrap().0
                {
                    anchors.push((glyph.index, f32::from(glyph.position.x)));
                }
            }
            if this.text.len() > anchors.last().unwrap().0 {
                anchors.push((this.text.len(), f32::from(line.width)));
            }
            let carets = caret_positions(&this.selection.boundaries, &anchors);
            let caret = px(carets[this
                .selection
                .boundaries
                .binary_search(&this.selection.focus)
                .unwrap()]);
            let width = (bounds.size.width - px(2.)).max(px(0.));
            this.scroll_x = this
                .scroll_x
                .min(caret)
                .max(caret - width)
                .max(px(0.))
                .min((line.width - width).max(px(0.)));
            let mut node = this.accessible_run.borrow_mut();
            node.set_bounds(accesskit::Rect::new(
                f64::from(bounds.left() - this.scroll_x),
                f64::from(bounds.top()),
                f64::from(bounds.left() - this.scroll_x + line.width),
                f64::from(bounds.bottom()),
            ));
            node.set_character_positions(
                carets
                    .iter()
                    .take(carets.len() - 1)
                    .copied()
                    .collect::<Vec<_>>(),
            );
            node.set_character_widths(
                carets
                    .windows(2)
                    .map(|p| (p[1] - p[0]).max(0.))
                    .collect::<Vec<_>>(),
            );
            this.geometry = Some(Geometry { bounds, carets });
        });
        line
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        line: &mut ShapedLine,
        window: &mut Window,
        cx: &mut App,
    ) {
        let this = self.entity.read(cx);
        let g = this.geometry.as_ref().unwrap();
        let viewport = bounds;
        let bounds = g.bounds;
        let x = |byte| {
            px(g.carets[this.selection.boundaries.binary_search(&byte).unwrap()]) - this.scroll_x
        };
        let range = this.selection();
        let focused = this.focus.is_focused(window);

        let caret_x = x(this.selection.focus);
        let scroll_x = this.scroll_x;
        let padding = this.effect.padding();
        let effect = if cx.reduce_motion() {
            KineticEffect::Static
        } else {
            this.effect
        };
        let boundaries = this.selection.boundaries.clone();
        let groups = if effect == KineticEffect::Static {
            Vec::new()
        } else {
            motion::clusters(
                &boundaries,
                line.runs
                    .iter()
                    .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.index)),
            )
        };
        let cutoff = effect.reveal_byte(&boundaries);
        let revealed_end = if matches!(effect, KineticEffect::Typewriter { .. }) {
            groups
                .iter()
                .filter(|(_, end, _)| *end <= cutoff)
                .map(|(_, end, _)| *end)
                .max()
                .unwrap_or(0)
        } else {
            this.text.len()
        };
        // Logical selection survives mode changes; visual chrome follows emitted clusters.
        let visible_range = range.start.min(revealed_end)..range.end.min(revealed_end);
        let start_x = x(visible_range.start);
        let end_x = x(visible_range.end);
        let caret_visible = this.selection.focus <= revealed_end;
        let color = window.text_style().color;
        let source = this.text.clone();
        let alphabets = this.scramble_alphabets.clone();
        let carets = g.carets.clone();
        #[cfg(feature = "portable-shaders")]
        let shadow = this.shadow.clone();
        #[cfg(feature = "portable-shaders")]
        let needs_commands = effect != KineticEffect::Static || shadow.is_some();
        #[cfg(not(feature = "portable-shaders"))]
        let needs_commands = effect != KineticEffect::Static;
        let commands = if needs_commands {
            render::GlyphFrame {
                line,
                effect,
                boundaries: &boundaries,
                groups: &groups,
                source: &source,
                alphabets: &alphabets,
                carets: &carets,
                bounds,
                origin: bounds.origin - point(scroll_x, -padding),
                scroll_x,
                line_height: window.line_height(),
            }
            .commands()
        } else {
            Vec::new()
        };
        #[cfg(feature = "portable-shaders")]
        let fill_brush = this.fill.clone();
        #[cfg(feature = "portable-shaders")]
        if let Some(shadow) = shadow {
            shadow.paint(window, viewport, bounds, &commands);
        }
        #[cfg(not(feature = "portable-shaders"))]
        let _ = viewport;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if !visible_range.is_empty() {
                window.paint_quad(fill(
                    Bounds::from_corners(
                        bounds.origin + point(start_x, px(0.)),
                        point(bounds.left() + end_x, bounds.bottom()),
                    ),
                    if focused {
                        rgba(0x299eae66)
                    } else {
                        rgba(0x75869a44)
                    },
                ));
            }
            let origin = bounds.origin - point(scroll_x, -padding);
            let mut paint_foreground = |window: &mut Window| {
                if effect == KineticEffect::Static {
                    line.paint(
                        origin,
                        window.line_height(),
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .unwrap();
                } else {
                    render::paint(&commands, window, color, point(px(0.), px(0.)), false).unwrap();
                }
            };
            #[cfg(feature = "portable-shaders")]
            if let Some(brush) = fill_brush {
                let field = Bounds::new(
                    bounds.origin - point(scroll_x, px(0.)),
                    size(line.width, bounds.size.height),
                );
                window.with_glyph_brush(brush, field, paint_foreground);
            } else {
                paint_foreground(window);
            }
            #[cfg(not(feature = "portable-shaders"))]
            {
                paint_foreground(window);
            }
            if focused && range.is_empty() && caret_visible {
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.origin + point(caret_x, px(0.)),
                        size(px(1.), bounds.size.height),
                    ),
                    rgb(0x56dac8),
                ));
            }
        });
        let weak = self.entity.downgrade();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                let _ = weak.update(cx, |this, cx| {
                    if this.selecting && event.pressed_button == Some(MouseButton::Left) {
                        this.selection.move_to(this.index_at(event.position), true);
                        cx.notify();
                    } else {
                        this.selecting = false;
                    }
                });
            }
        });
        let weak = self.entity.downgrade();
        window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
            let _ = weak.update(cx, |this, _| this.selecting = false);
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn validates_supported_subset_without_mutating_unicode() {
        for text in ["", "office café e\u{301} 👩🏽‍💻 🇺🇸", "你好 · こんにちは"]
        {
            assert!(validate(text).is_ok());
        }
        for text in [
            "two\nlines",
            "a\tb",
            "שלום",
            "مرحبا",
            "x\u{202e}abc",
            "a\0b",
        ] {
            assert!(validate(text).is_err());
        }
    }
    #[::core::prelude::v1::test]
    fn accessible_text_preserves_source_and_grapheme_byte_lengths() {
        let text = "e\u{301} 👩🏽‍💻";
        let node = accessible_run(text);
        assert!(validate(&format!("e{}", "\u{301}".repeat(128))).is_err());
        assert_eq!(node.value(), Some(text));
        assert_eq!(node.character_lengths(), &[3, 1, 15]);
        assert_eq!(
            node.character_lengths()
                .iter()
                .map(|n| usize::from(*n))
                .sum::<usize>(),
            text.len()
        );
    }
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    use std::sync::Arc;
    #[::core::prelude::v1::test]
    fn real_shaping_keyboard_clipboard_drag_and_resize_preserve_source() {
        let mut app = TestAppContext::build_with_text_system(
            TestDispatcher::new(0),
            None,
            Arc::new(gpui_wgpu::CosmicTextSystem::new_without_system_fonts(
                "IBM Plex Sans",
            )),
        );
        app.text_system().add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"), "/../../../gpui/crates/gpui-web/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
        )).as_slice())]).unwrap();
        let source = "office cafe\u{301} 👩🏽‍💻 🇺🇸";
        let (text, cx) = app.add_window_view(|_, cx| SelectableText::new(source, cx).unwrap());
        cx.update(|window, cx| window.focus(&text.focus_handle(cx), cx));
        cx.simulate_keystrokes("end shift-left");
        text.read_with(cx, |text, _| assert_eq!(text.selected_text(), "🇺🇸"));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-c"
        } else {
            "ctrl-c"
        });
        assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "🇺🇸");
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-a"
        } else {
            "ctrl-a"
        });
        text.read_with(cx, |text, _| assert_eq!(text.selected_text(), source));
        // Editing shortcuts and typed characters must not mutate this read-only view.
        cx.simulate_keystrokes("backspace delete x");
        text.read_with(cx, |text, _| assert_eq!(text.text(), source));
        cx.simulate_keystrokes("home shift-right shift-right shift-left");
        text.read_with(cx, |text, _| assert_eq!(text.selected_text(), "o"));
        let bounds = text.read_with(cx, |text, _| text.geometry.as_ref().unwrap().bounds);
        cx.simulate_mouse_down(bounds.origin, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(
            point(bounds.right() + px(100.), bounds.top()),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_up(
            point(bounds.right() + px(100.), bounds.top()),
            MouseButton::Left,
            Modifiers::default(),
        );
        text.read_with(cx, |text, _| assert_eq!(text.selected_text(), source));
        let window = cx.update(|window, _| window.window_handle());
        cx.simulate_window_resize(window, size(px(80.), px(100.)));
        cx.run_until_parked();
        text.read_with(cx, |text, _| assert_eq!(text.selected_text(), source));
        cx.simulate_keystrokes("end");
        cx.update(|window, cx| window.draw(cx).clear(cx));
        text.read_with(cx, |text, _| {
            assert!(
                text.scroll_x > px(0.),
                "scroll={:?}, bounds={:?}, carets={:?}",
                text.scroll_x,
                text.geometry.as_ref().unwrap().bounds,
                text.geometry.as_ref().unwrap().carets
            )
        });
        cx.simulate_keystrokes("escape");
        cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("sentinel".into())));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-c"
        } else {
            "ctrl-c"
        });
        assert_eq!(
            cx.read_from_clipboard().unwrap().text().unwrap(),
            "sentinel"
        );
        let before = text.read_with(cx, |text, _| text.geometry.as_ref().unwrap().carets.clone());
        for effect in [
            KineticEffect::Typewriter { progress: 0. },
            KineticEffect::Typewriter { progress: 0.5 },
            KineticEffect::Scramble {
                progress: 0.5,
                seed: 42,
                tick: 9,
            },
            KineticEffect::Wave {
                amplitude: px(8.),
                phase: 1.5,
                cluster_phase: 0.65,
            },
        ] {
            text.update(cx, |text, cx| {
                text.set_effect(effect, cx).unwrap();
                text.select_all(cx);
                text.copy_selection(cx);
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            text.read_with(cx, |text, _| {
                assert_eq!(text.text(), source);
                assert_eq!(text.selected_text(), source);
                assert_eq!(text.geometry.as_ref().unwrap().carets, before);
                assert_eq!(text.accessible_run.borrow().value(), Some(source));
            });
            assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), source);
        }
        let effect = text.read_with(cx, |text, _| text.effect());
        text.update(cx, |text, cx| {
            assert!(
                text.set_effect(KineticEffect::Typewriter { progress: f32::NAN }, cx)
                    .is_err()
            )
        });
        text.read_with(cx, |text, _| assert_eq!(text.effect(), effect));
        cx.update(|window, cx| {
            cx.set_reduce_motion(true);
            window.draw(cx).clear(cx);
        });
        text.read_with(cx, |text, _| {
            let node = text.accessible_run.borrow();
            assert_eq!(node.value(), Some(source));
            assert_eq!(
                node.character_lengths().len(),
                text.selection.boundaries.len() - 1
            );
            assert_eq!(
                node.character_positions().unwrap().len(),
                node.character_lengths().len()
            );
            assert_eq!(
                node.character_widths().unwrap().len(),
                node.character_lengths().len()
            );
        });
    }
}

#[cfg(all(test, target_os = "macos", feature = "wgpu"))]
mod motion_render_tests {
    use super::*;
    use std::sync::Arc;
    struct View {
        text: Entity<SelectableText>,
    }
    impl Render for View {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .bg(white())
                .text_color(black())
                .text_size(px(24.))
                .line_height(px(40.))
                .child(self.text.clone())
        }
    }
    #[::core::prelude::v1::test]
    fn full_reveal_matches_native_line_and_reduced_motion_reveals_hidden_source() {
        let mut app = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(()),
            || Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))),
        );
        let window = app
            .open_window(size(px(480.), px(120.)), |_, cx| {
                cx.new(|cx| View {
                    text: cx.new(|cx| SelectableText::new("office cafe\u{301} 👩🏽‍💻", cx).unwrap()),
                })
            })
            .unwrap();
        let text = window
            .update(&mut app, |view, _, _| view.text.clone())
            .unwrap();
        app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
        let reference = app.capture_screenshot(window.into()).unwrap();
        for (effect, reduced, same) in [
            (KineticEffect::Typewriter { progress: 1. }, false, true),
            (KineticEffect::Typewriter { progress: 0. }, false, false),
            (KineticEffect::Typewriter { progress: 0. }, true, true),
            (
                KineticEffect::Scramble {
                    progress: 1.,
                    seed: 42,
                    tick: 99,
                },
                false,
                true,
            ),
            (
                KineticEffect::Scramble {
                    progress: 0.,
                    seed: 42,
                    tick: 99,
                },
                false,
                false,
            ),
            (
                KineticEffect::Scramble {
                    progress: 0.,
                    seed: 42,
                    tick: 99,
                },
                true,
                true,
            ),
        ] {
            app.update(|cx| {
                cx.set_reduce_motion(reduced);
                text.update(cx, |text, cx| text.set_effect(effect, cx).unwrap());
            });
            app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            let actual = app.capture_screenshot(window.into()).unwrap();
            let differences = reference
                .pixels()
                .zip(actual.pixels())
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                differences == 0,
                same,
                "{effect:?}, reduced={reduced}, differing pixels={differences}"
            );
        }
        let mut frames = Vec::new();
        let mut identities = Vec::new();
        for (seed, tick) in [(42, 9), (43, 9), (42, 10), (42, 9)] {
            app.update(|cx| {
                cx.set_reduce_motion(false);
                text.update(cx, |text, cx| {
                    text.set_effect(
                        KineticEffect::Scramble {
                            progress: 0.,
                            seed,
                            tick,
                        },
                        cx,
                    )
                    .unwrap()
                });
            });
            app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            frames.push(app.capture_screenshot(window.into()).unwrap());
            identities.push(app.update(|cx| {
                text.read(cx)
                    .scramble_alphabets
                    .iter()
                    .map(Rc::as_ptr)
                    .collect::<Vec<_>>()
            }));
        }
        assert!(
            frames[0].as_raw() != frames[1].as_raw(),
            "seed must affect unresolved glyphs"
        );
        assert!(
            frames[0].as_raw() != frames[2].as_raw(),
            "tick must affect unresolved glyphs"
        );
        assert!(
            frames[0].as_raw() == frames[3].as_raw(),
            "revisiting a snapshot must reproduce identical pixels"
        );
        assert!(
            identities.windows(2).all(|pair| pair[0] == pair[1]),
            "animation must retain its prepared font alphabets"
        );
    }
    #[::core::prelude::v1::test]
    fn hidden_typewriter_selection_does_not_paint_ghost_highlight() {
        let mut app = HeadlessAppContext::with_platform(
            Arc::new(gpui_apple::AppleTextSystem::new()),
            Arc::new(()),
            || Ok(Some(Box::new(gpui_wgpu::WgpuHeadlessRenderer::new()?))),
        );
        let source = "Good typography keeps its meaning.";
        let window = app
            .open_window(size(px(480.), px(120.)), |_, cx| {
                cx.new(|cx| View {
                    text: cx.new(|cx| SelectableText::new(source, cx).unwrap()),
                })
            })
            .unwrap();
        let text = window
            .update(&mut app, |view, _, _| view.text.clone())
            .unwrap();
        let meaning = source.find("meaning").unwrap();
        let mut capture = |effect, selection: Range<usize>| {
            app.update(|cx| {
                text.update(cx, |text, cx| {
                    text.set_effect(effect, cx).unwrap();
                    text.selection.move_to(selection.start, false);
                    text.selection.move_to(selection.end, true);
                    cx.notify();
                })
            });
            app.update_window(window.into(), |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
            app.capture_screenshot(window.into()).unwrap()
        };
        let selected_static = capture(KineticEffect::Static, meaning..meaning + 7);
        let hidden = capture(
            KineticEffect::Typewriter { progress: 0.45 },
            meaning..meaning + 7,
        );
        let unselected = capture(KineticEffect::Typewriter { progress: 0.45 }, 0..0);
        assert!(
            hidden.as_raw() == unselected.as_raw(),
            "unrevealed meaning must not leave a selection rectangle"
        );
        let restored = capture(
            KineticEffect::Typewriter { progress: 1. },
            meaning..meaning + 7,
        );
        assert!(
            restored.as_raw() == selected_static.as_raw(),
            "selection returns as its text becomes visible"
        );
        let all = capture(
            KineticEffect::Typewriter { progress: 0.16 },
            0..source.len(),
        );
        let visible_prefix = capture(KineticEffect::Typewriter { progress: 0.16 }, 0..5);
        assert!(
            all.as_raw() == visible_prefix.as_raw(),
            "partly revealed selections highlight only visible clusters"
        );
        capture(
            KineticEffect::Typewriter { progress: 0.45 },
            meaning..meaning + 7,
        );
        app.update(|cx| {
            text.update(cx, |text, cx| {
                assert_eq!(text.selected_text(), "meaning");
                assert!(text.copy_selection(cx));
                assert_eq!(cx.read_from_clipboard().unwrap().text().unwrap(), "meaning");
                assert_eq!(text.accessible_run.borrow().value(), Some(source));
            })
        });
    }
}

#[cfg(all(test, target_os = "macos"))]
mod wide_tests;
