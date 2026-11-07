//! `typeset()`: a text element shaped by the installed Parley engine.
//!
//! The element inherits GPUI's text style (font, size, line height, color,
//! `white_space`, `text_overflow`, `line_clamp`, `text_align`) and adds explicit
//! base direction and content language. It takes part in layout through
//! `Window::request_measured_layout`, paints through GPUI's glyph atlas, and
//! needs no GPUI core API. Without an installed engine it renders as plain GPUI
//! text, so components can use it unconditionally.
use crate::{ParagraphLayout, ParagraphOptions, ParagraphShape, ParleyTextSystem};
use crate::{TextDirection, TextLanguage};
use anyhow::{Context as _, Result};
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, FontId, FontRun,
    Global, GlobalElementId, Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, SharedString,
    Size, Style, TextAlign, TextOverflow, WhiteSpace, Window, point, px, size,
};
use parley::Alignment;
use std::{cell::RefCell, collections::HashMap, hash::Hash, ops::Range, rc::Rc, sync::Arc};

/// Install `parley` as the engine behind [`typeset`] elements.
///
/// `parley` must be the text system installed with
/// `Application::with_platform_and_text_system`, so the font IDs GPUI resolves
/// from text styles belong to it and its glyphs reach the shared atlas.
pub fn install(parley: Arc<ParleyTextSystem>, cx: &mut App) {
    cx.set_global(TypesetEngine::new(parley));
}

/// Text shaped by the installed Parley engine; see [`install`].
pub fn typeset(text: impl Into<SharedString>) -> Typeset {
    Typeset {
        id: None,
        text: text.into(),
        direction: TextDirection::Auto,
        language: None,
    }
}

/// A [`typeset`] element.
#[derive(Clone, Debug)]
pub struct Typeset {
    id: Option<ElementId>,
    text: SharedString,
    direction: TextDirection,
    language: Option<TextLanguage>,
}

impl Typeset {
    /// Give the text an ID, which exposes it to screen readers as a label
    /// (as `gpui::Text` does). Without one it is presentational: set accessible
    /// properties on the containing control instead.
    pub fn with_id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Base direction; the default [`TextDirection::Auto`] uses the first strong character.
    pub fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Right-to-left base direction.
    pub fn rtl(self) -> Self {
        self.direction(TextDirection::RightToLeft)
    }

    /// Left-to-right base direction.
    pub fn ltr(self) -> Self {
        self.direction(TextDirection::LeftToRight)
    }

    /// Content language for language-specific shaping and line breaking.
    pub fn language(mut self, language: TextLanguage) -> Self {
        self.language = Some(language);
        self
    }

    /// The full text, including any part hidden by truncation.
    pub fn text(&self) -> &SharedString {
        &self.text
    }
}

impl IntoElement for Typeset {
    type Element = Self;
    fn into_element(self) -> Self::Element {
        self
    }
}

/// Request-layout state: the fitted text, or plain GPUI text without an engine.
pub struct TypesetLayout {
    fitted: Rc<RefCell<Option<Fitted>>>,
    fallback: Option<AnyElement>,
    color: Hsla,
}

impl Element for Typeset {
    type RequestLayoutState = TypesetLayout;
    type PrepaintState = Option<(Arc<ParagraphLayout>, Pixels)>;

    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<gpui::accesskit::Role> {
        self.id.as_ref().map(|_| gpui::accesskit::Role::Label)
    }

    fn write_a11y_info(&self, node: &mut gpui::accesskit::Node) {
        node.set_value(self.text.to_string());
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let style = window.text_style();
        let fitted = Rc::new(RefCell::new(None));
        if !cx.has_global::<TypesetEngine>() {
            let mut fallback = gpui::Text::new_inaccessible(self.text.clone()).into_any_element();
            let layout_id = fallback.request_layout(window, cx);
            let state = TypesetLayout {
                fitted,
                fallback: Some(fallback),
                color: style.color,
            };
            return (layout_id, state);
        }
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let spec = TypesetSpec {
            text: hard_breaks(&self.text),
            font: window.text_system().resolve_font(&style.font()),
            font_size,
            line_height,
            options: ParagraphOptions {
                direction: self.direction,
                language: self.language,
                line_height: Some(line_height),
            },
            wrap: style.white_space == WhiteSpace::Normal,
            overflow: style.text_overflow.as_ref().map(OverflowKey::from),
            line_clamp: style.line_clamp.filter(|lines| *lines > 0),
            align: AlignKey::from(style.text_align),
        };
        let layout_id = window.request_measured_layout(Style::default(), {
            let fitted = fitted.clone();
            move |known, available, _window, cx| {
                let Some(engine) = cx.try_global::<TypesetEngine>() else {
                    return Size::default();
                };
                match engine.fit(&spec, known, available) {
                    Ok(fit) => {
                        let size = fit.size;
                        *fitted.borrow_mut() = Some(fit);
                        size
                    }
                    Err(error) => {
                        engine.parley.report_layout_error(&error);
                        *fitted.borrow_mut() = None;
                        Size::default()
                    }
                }
            }
        });
        let state = TypesetLayout {
            fitted,
            fallback: None,
            color: style.color,
        };
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if let Some(fallback) = &mut state.fallback {
            fallback.prepaint(window, cx);
            return None;
        }
        let engine = cx.try_global::<TypesetEngine>()?;
        let fitted = state.fitted.borrow();
        let fitted = fitted.as_ref()?;
        // Wrapped text reflows at its final width; unbroken lines are placed here.
        let break_width = fitted.wrap.then_some(bounds.size.width);
        match engine.layout(&fitted.shaped, break_width, fitted.align) {
            Ok(layout) => {
                let offset = if fitted.wrap {
                    px(0.0)
                } else {
                    let free = (bounds.size.width - layout.content_width()).max(px(0.0));
                    fitted.align.offset(free, fitted.shaped.shape.is_rtl())
                };
                Some((layout, offset))
            }
            Err(error) => {
                engine.parley.report_layout_error(&error);
                None
            }
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(fallback) = &mut state.fallback {
            fallback.paint(window, cx);
            return;
        }
        let Some((layout, offset)) = prepaint.as_ref() else {
            return;
        };
        let clamped = state
            .fitted
            .borrow()
            .as_ref()
            .and_then(|fitted| fitted.line_clamp)
            .is_some_and(|lines| layout.line_count() > lines);
        let color = state.color;
        let origin = bounds.origin + point(*offset, px(0.0));
        let paint = |window: &mut Window| {
            if let Err(error) = gpui::paint_paragraph(layout.as_ref(), origin, |_| color, window)
                && let Some(engine) = cx.try_global::<TypesetEngine>()
            {
                engine.parley.report_layout_error(&error);
            }
        };
        if clamped {
            // Lines beyond the clamp are laid out but must not show.
            window.with_content_mask(Some(ContentMask { bounds }), paint);
        } else {
            paint(window);
        }
    }
}

/// Parley expresses hard line breaks inside a paragraph as U+2028.
fn hard_breaks(text: &SharedString) -> SharedString {
    if text.contains(['\n', '\r', '\u{2029}']) {
        text.replace("\r\n", "\u{2028}")
            .replace(['\n', '\r', '\u{2029}'], "\u{2028}")
            .into()
    } else {
        text.clone()
    }
}

/// Everything that determines a typeset element's shaping and fitting.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TypesetSpec {
    pub text: SharedString,
    pub font: FontId,
    pub font_size: Pixels,
    pub line_height: Pixels,
    pub options: ParagraphOptions,
    pub wrap: bool,
    pub overflow: Option<OverflowKey>,
    pub line_clamp: Option<usize>,
    pub align: AlignKey,
}

/// `TextOverflow` with `Hash`, for cache keys.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OverflowKey {
    End(SharedString),
    Start(SharedString),
    Middle(SharedString),
}

impl From<&TextOverflow> for OverflowKey {
    fn from(overflow: &TextOverflow) -> Self {
        match overflow {
            TextOverflow::Truncate(affix) => Self::End(affix.clone()),
            TextOverflow::TruncateStart(affix) => Self::Start(affix.clone()),
            TextOverflow::TruncateMiddle(affix) => Self::Middle(affix.clone()),
        }
    }
}

/// Direction-aware alignment. GPUI's default `TextAlign::Left` means "natural",
/// so `Left`/`Right` map to start/end and mirror for right-to-left text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum AlignKey {
    Start,
    Center,
    End,
}

impl From<TextAlign> for AlignKey {
    fn from(align: TextAlign) -> Self {
        match align {
            TextAlign::Left => Self::Start,
            TextAlign::Center => Self::Center,
            TextAlign::Right => Self::End,
        }
    }
}

impl AlignKey {
    fn parley(self) -> Alignment {
        match self {
            Self::Start => Alignment::Start,
            Self::Center => Alignment::Center,
            Self::End => Alignment::End,
        }
    }

    /// Horizontal offset for an unbroken line given the free space beside it.
    fn offset(self, free: Pixels, rtl: bool) -> Pixels {
        match (self, rtl) {
            (Self::Start, false) | (Self::End, true) => px(0.0),
            (Self::Start, true) | (Self::End, false) => free,
            (Self::Center, _) => free / 2.0,
        }
    }
}

/// A cached shape together with the key it was cached under.
#[derive(Clone)]
pub(crate) struct Shaped {
    key: ShapeKey,
    pub shape: Arc<ParagraphShape>,
}

/// The result of fitting a spec to its layout constraints.
#[derive(Clone)]
pub(crate) struct Fitted {
    /// The displayed text's shape; differs from the source when truncated.
    pub shaped: Shaped,
    pub size: Size<Pixels>,
    pub wrap: bool,
    pub align: AlignKey,
    pub line_clamp: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SpaceKey {
    Definite(Pixels),
    MinContent,
    MaxContent,
}

impl From<AvailableSpace> for SpaceKey {
    fn from(space: AvailableSpace) -> Self {
        match space {
            AvailableSpace::Definite(width) => Self::Definite(width),
            AvailableSpace::MinContent => Self::MinContent,
            AvailableSpace::MaxContent => Self::MaxContent,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct ShapeKey {
    text: SharedString,
    font: FontId,
    font_size: Pixels,
    options: ParagraphOptions,
}

/// A small least-recently-used map: when full, the older half is dropped.
struct Lru<K, V> {
    entries: HashMap<K, (V, u64)>,
    tick: u64,
    capacity: usize,
}

impl<K: Hash + Eq, V: Clone> Lru<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            tick: 0,
            capacity,
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(value, used)| {
            *used = tick;
            value.clone()
        })
    }

    fn insert(&mut self, key: K, value: V) {
        self.tick += 1;
        if self.entries.len() >= self.capacity {
            let keep_after = self.tick.saturating_sub(self.capacity as u64 / 2);
            self.entries.retain(|_, (_, used)| *used > keep_after);
        }
        self.entries.insert(key, (value, self.tick));
    }
}

const SHAPE_CAPACITY: usize = 4096;

struct Caches {
    shapes: Lru<ShapeKey, Shaped>,
    layouts: Lru<(ShapeKey, Option<Pixels>, AlignKey), Arc<ParagraphLayout>>,
    fits: Lru<(TypesetSpec, Option<Pixels>, Option<Pixels>, SpaceKey), Fitted>,
}

/// The installed engine and its caches, stored as a GPUI global.
pub(crate) struct TypesetEngine {
    pub parley: Arc<ParleyTextSystem>,
    caches: RefCell<Caches>,
}

impl Global for TypesetEngine {}

impl TypesetEngine {
    pub fn new(parley: Arc<ParleyTextSystem>) -> Self {
        Self {
            parley,
            caches: RefCell::new(Caches {
                shapes: Lru::new(SHAPE_CAPACITY),
                layouts: Lru::new(SHAPE_CAPACITY),
                fits: Lru::new(SHAPE_CAPACITY),
            }),
        }
    }

    fn shape_key(spec: &TypesetSpec, text: &str) -> ShapeKey {
        ShapeKey {
            text: if text == spec.text.as_ref() {
                spec.text.clone()
            } else {
                SharedString::from(text.to_owned())
            },
            font: spec.font,
            font_size: spec.font_size,
            options: spec.options,
        }
    }

    /// Shape `text` with the spec's font and options, reusing earlier results.
    pub fn shape(&self, spec: &TypesetSpec, text: &str) -> Result<Shaped> {
        let key = Self::shape_key(spec, text);
        if let Some(shaped) = self.caches.borrow_mut().shapes.get(&key) {
            return Ok(shaped);
        }
        let runs = [FontRun {
            len: text.len(),
            font_id: spec.font,
        }];
        // Shaping runs outside the cache borrow.
        let shape = Arc::new(self.parley.prepare_paragraph_with_options(
            text,
            spec.font_size,
            &runs,
            spec.options,
        )?);
        let shaped = Shaped { key, shape };
        self.caches
            .borrow_mut()
            .shapes
            .insert(shaped.key.clone(), shaped.clone());
        Ok(shaped)
    }

    /// Reflow a shape, reusing results for the same break width and alignment.
    pub fn layout(
        &self,
        shaped: &Shaped,
        break_width: Option<Pixels>,
        align: AlignKey,
    ) -> Result<Arc<ParagraphLayout>> {
        let key = (shaped.key.clone(), break_width, align);
        if let Some(layout) = self.caches.borrow_mut().layouts.get(&key) {
            return Ok(layout);
        }
        let layout = Arc::new(shaped.shape.layout_with(break_width, align.parley())?);
        self.caches.borrow_mut().layouts.insert(key, layout.clone());
        Ok(layout)
    }

    /// Size the text for taffy's measure call, truncating if the style asks.
    pub fn fit(
        &self,
        spec: &TypesetSpec,
        known: Size<Option<Pixels>>,
        available: Size<AvailableSpace>,
    ) -> Result<Fitted> {
        let key = (
            spec.clone(),
            known.width,
            known.height,
            SpaceKey::from(available.width),
        );
        if let Some(fitted) = self.caches.borrow_mut().fits.get(&key) {
            return Ok(fitted);
        }
        let fitted = self.fit_uncached(spec, known, available)?;
        self.caches.borrow_mut().fits.insert(key, fitted.clone());
        Ok(fitted)
    }

    fn fit_uncached(
        &self,
        spec: &TypesetSpec,
        known: Size<Option<Pixels>>,
        available: Size<AvailableSpace>,
    ) -> Result<Fitted> {
        let full = self.shape(spec, &spec.text)?;
        let (min, max) = full.shape.content_widths();
        let limit = known.width.or(match available.width {
            AvailableSpace::Definite(width) => Some(width),
            _ => None,
        });
        let (shape, width, lines) = if spec.wrap {
            // As in taffy's Parley example: min-/max-content probes report the
            // narrowest and widest layouts; a definite width is clamped between them.
            let width = known
                .width
                .unwrap_or(match available.width {
                    AvailableSpace::MinContent => min,
                    AvailableSpace::MaxContent => max,
                    AvailableSpace::Definite(width) => width.min(max).max(min),
                })
                .ceil();
            let mut shape = full;
            let mut lines = self.layout(&shape, Some(width), spec.align)?.line_count();
            if let Some(clamp) = spec.line_clamp
                && lines > clamp
            {
                if let Some(overflow) = &spec.overflow {
                    shape = self.truncate_lines(spec, &shape, width, clamp, overflow)?;
                }
                lines = clamp;
            }
            (shape, width, lines)
        } else {
            let mut shape = full;
            let mut width = max;
            if let Some(overflow) = &spec.overflow
                && let Some(limit) = limit
                && max > limit
            {
                shape = self.truncate_line(spec, &shape, limit, overflow)?;
                width = limit;
            }
            let lines = self.layout(&shape, None, spec.align)?.line_count();
            (shape, known.width.unwrap_or(width.ceil()), lines)
        };
        let height = known
            .height
            .unwrap_or(spec.line_height * lines.max(1) as f32);
        Ok(Fitted {
            shaped: shape,
            size: size(width, height),
            wrap: spec.wrap,
            align: spec.align,
            line_clamp: spec.line_clamp,
        })
    }

    fn affix_width(&self, spec: &TypesetSpec, affix: &str) -> Result<Pixels> {
        if affix.is_empty() {
            return Ok(px(0.0));
        }
        Ok(self.shape(spec, affix)?.shape.content_widths().1)
    }

    /// Shorten an unbroken line to fit `limit`, keeping the logical start, end or
    /// both. Truncation is logical, so the base direction places the affix on the
    /// correct side. Advances estimate the cut; reshaping confirms it.
    fn truncate_line(
        &self,
        spec: &TypesetSpec,
        full: &Shaped,
        limit: Pixels,
        overflow: &OverflowKey,
    ) -> Result<Shaped> {
        let text = spec.text.as_ref();
        let (affix, from) = match overflow {
            OverflowKey::End(affix) => (affix, Cut::End),
            OverflowKey::Start(affix) => (affix, Cut::Start),
            OverflowKey::Middle(affix) => (affix, Cut::Middle),
        };
        let spec = &resolved_direction(spec, full);
        let clusters = full.shape.logical_clusters();
        let budget = f32::from(limit - self.affix_width(spec, affix)?);
        let mut keep = estimate_kept(&clusters, budget, from);
        loop {
            let candidate = truncated(text, &clusters, keep, affix, from);
            let shaped = self.shape(spec, &candidate)?;
            if shaped.shape.content_widths().1 <= limit || keep == 0 {
                return Ok(shaped);
            }
            keep -= 1;
        }
    }

    /// Shorten wrapped text so it fits `clamp` lines with the affix at the end.
    /// Start and middle truncation of wrapped text also cut at the end.
    fn truncate_lines(
        &self,
        spec: &TypesetSpec,
        full: &Shaped,
        width: Pixels,
        clamp: usize,
        overflow: &OverflowKey,
    ) -> Result<Shaped> {
        let text = spec.text.as_ref();
        let affix = match overflow {
            OverflowKey::End(affix) | OverflowKey::Start(affix) | OverflowKey::Middle(affix) => {
                affix
            }
        };
        let layout = self.layout(full, Some(width), spec.align)?;
        let last = layout
            .parley_layout()
            .lines()
            .nth(clamp - 1)
            .context("clamped line missing")?
            .text_range();
        let spec = &resolved_direction(spec, full);
        let clusters = full.shape.logical_clusters();
        let line_start = clusters.partition_point(|(range, _)| range.start < last.start);
        let budget = f32::from(width - self.affix_width(spec, affix)?);
        let mut keep = line_start + estimate_kept(&clusters[line_start..], budget, Cut::End);
        loop {
            let candidate = truncated(text, &clusters, keep, affix, Cut::End);
            let shaped = self.shape(spec, &candidate)?;
            let lines = self.layout(&shaped, Some(width), spec.align)?.line_count();
            if lines <= clamp || keep == 0 {
                return Ok(shaped);
            }
            keep -= 1;
        }
    }
}

/// Truncation removes text, which can change what `Auto` would infer (cutting
/// "office مرحبا" at the start leaves Arabic first). Candidates keep the full
/// text's resolved direction instead.
fn resolved_direction(spec: &TypesetSpec, full: &Shaped) -> TypesetSpec {
    let mut spec = spec.clone();
    if spec.options.direction == TextDirection::Auto {
        spec.options.direction = if full.shape.is_rtl() {
            TextDirection::RightToLeft
        } else {
            TextDirection::LeftToRight
        };
    }
    spec
}

#[derive(Clone, Copy)]
enum Cut {
    End,
    Start,
    Middle,
}

/// How many clusters fit in `budget`, counted from the kept side(s).
fn estimate_kept(clusters: &[(Range<usize>, f32)], budget: f32, from: Cut) -> usize {
    let take = |iter: &mut dyn Iterator<Item = f32>, budget: f32| {
        let mut used = 0.0;
        let mut count = 0;
        for advance in iter {
            if used + advance > budget {
                break;
            }
            used += advance;
            count += 1;
        }
        count
    };
    if budget <= 0.0 {
        return 0;
    }
    match from {
        Cut::End => take(&mut clusters.iter().map(|(_, a)| *a), budget),
        Cut::Start => take(&mut clusters.iter().rev().map(|(_, a)| *a), budget),
        Cut::Middle => {
            let head = take(&mut clusters.iter().map(|(_, a)| *a), budget / 2.0);
            let tail = take(
                &mut clusters[head..].iter().rev().map(|(_, a)| *a),
                budget / 2.0,
            );
            head + tail
        }
    }
}

/// The text keeping `keep` clusters plus the affix, trimming whitespace at the cut.
fn truncated(
    text: &str,
    clusters: &[(Range<usize>, f32)],
    keep: usize,
    affix: &str,
    from: Cut,
) -> String {
    let keep = keep.min(clusters.len());
    let head_end = |count: usize| {
        clusters
            .get(count.wrapping_sub(1))
            .map_or(0, |(r, _)| r.end)
    };
    let tail_start = |count: usize| {
        clusters
            .len()
            .checked_sub(count)
            .and_then(|index| clusters.get(index))
            .map_or(text.len(), |(r, _)| r.start)
    };
    match from {
        Cut::End => format!("{}{affix}", text[..head_end(keep)].trim_end()),
        Cut::Start => format!("{affix}{}", text[tail_start(keep)..].trim_start()),
        Cut::Middle => {
            let head = keep.div_ceil(2);
            let tail = keep / 2;
            format!(
                "{}{affix}{}",
                text[..head_end(head)].trim_end(),
                text[tail_start(tail)..].trim_start()
            )
        }
    }
}

#[cfg(test)]
#[path = "typeset/tests.rs"]
mod tests;
