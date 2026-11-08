//! `typeset()`: a text element shaped by the installed Parley engine.
//!
//! The element inherits GPUI's text style (font, size, line height, color,
//! `white_space`, `text_overflow`, `line_clamp`, `text_align`, decorations) and
//! adds explicit base direction and content language. Styled ranges use GPUI's
//! own model: `with_highlights` and `with_runs`, as on `StyledText`. It takes part in layout through
//! `Window::request_measured_layout`, paints through GPUI's glyph atlas, and
//! needs no GPUI core API. Without an installed engine it renders as plain GPUI
//! text, so components can use it unconditionally.
use crate::{ParagraphLayout, ParagraphOptions, ParagraphShape, ParleyTextSystem};
use crate::{TextDirection, TextLanguage};
use anyhow::{Context as _, Result};
use gpui::{
    AnyElement, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, FontId, FontRun,
    Global, GlobalElementId, HighlightStyle, Hsla, InspectorElementId, IntoElement, LayoutId,
    ParagraphPosition, Pixels, SharedString, Size, StrikethroughStyle, Style, TextAffinity,
    TextAlign, TextOverflow, TextRun, TextStyle, UnderlineStyle, WhiteSpace, Window, fill, point,
    px, size,
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
        styling: Styling::Inherited,
    }
}

/// A [`typeset`] element.
#[derive(Clone, Debug)]
pub struct Typeset {
    id: Option<ElementId>,
    text: SharedString,
    direction: TextDirection,
    language: Option<TextLanguage>,
    styling: Styling,
}

/// How the text's runs are styled.
#[derive(Clone, Debug)]
enum Styling {
    /// One run in the inherited text style.
    Inherited,
    /// Byte ranges restyled over the inherited text style.
    Highlights(Arc<[(Range<usize>, HighlightStyle)]>),
    /// Complete runs that replace the inherited style.
    Runs(Arc<[TextRun]>),
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

    /// Restyle byte ranges over the inherited text style, as
    /// `StyledText::with_highlights`. Ranges must be sorted, non-overlapping and
    /// on character boundaries; color, weight, style, background, underline,
    /// strikethrough and fade apply.
    pub fn with_highlights(
        mut self,
        highlights: impl IntoIterator<Item = (Range<usize>, HighlightStyle)>,
    ) -> Self {
        self.styling = Styling::Highlights(highlights.into_iter().collect());
        self
    }

    /// Replace the inherited style with explicit runs, as `StyledText::with_runs`.
    /// Run lengths must cover the text exactly on character boundaries; runs may
    /// use different fonts but share the inherited font size.
    pub fn with_runs(mut self, runs: Vec<TextRun>) -> Self {
        self.styling = Styling::Runs(runs.into());
        self
    }

    /// The full text, including any part hidden by truncation.
    pub fn text(&self) -> &SharedString {
        &self.text
    }

    /// Source runs for `base`; invalid highlights or runs fall back to one run.
    fn source_runs(&self, base: &TextStyle) -> Vec<TextRun> {
        let text = self.text.as_ref();
        let single = || vec![base.to_run(text.len())];
        match &self.styling {
            Styling::Inherited => single(),
            Styling::Runs(runs) => {
                let mut end = 0usize;
                let valid = runs.iter().all(|run| {
                    end += run.len;
                    run.len > 0 && text.is_char_boundary(end.min(text.len())) && end <= text.len()
                }) && end == text.len();
                debug_assert!(valid, "typeset runs must cover the text: {text:?}");
                if valid { runs.to_vec() } else { single() }
            }
            Styling::Highlights(highlights) => {
                let mut runs = Vec::new();
                let mut index = 0;
                for (range, highlight) in highlights.iter() {
                    let valid = range.start >= index
                        && range.start < range.end
                        && range.end <= text.len()
                        && text.is_char_boundary(range.start)
                        && text.is_char_boundary(range.end);
                    debug_assert!(valid, "invalid typeset highlight {range:?} in {text:?}");
                    if !valid {
                        continue;
                    }
                    if index < range.start {
                        runs.push(base.to_run(range.start - index));
                    }
                    runs.push(base.clone().highlight(*highlight).to_run(range.len()));
                    index = range.end;
                }
                if index < text.len() || runs.is_empty() {
                    runs.push(base.to_run(text.len() - index));
                }
                runs
            }
        }
    }
}

/// Paint attributes of one display run; fonts live in the shaping spec.
#[derive(Clone, Debug)]
struct PaintRun {
    len: usize,
    color: Hsla,
    background: Option<Hsla>,
    underline: Option<UnderlineStyle>,
    strikethrough: Option<StrikethroughStyle>,
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
    paint_runs: Arc<[PaintRun]>,
}

impl Element for Typeset {
    type RequestLayoutState = TypesetLayout;
    type PrepaintState = Option<Prepared>;

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
        let source_runs = self.source_runs(&style);
        if !cx.has_global::<TypesetEngine>() {
            let mut fallback = gpui::StyledText::new(self.text.clone())
                .with_runs(source_runs)
                .into_any_element();
            let layout_id = fallback.request_layout(window, cx);
            let state = TypesetLayout {
                fitted,
                fallback: Some(fallback),
                paint_runs: Arc::new([]),
            };
            return (layout_id, state);
        }
        let (text, display_runs) = hard_breaks(&self.text, source_runs);
        let mut fonts: Vec<FontRun> = Vec::with_capacity(display_runs.len());
        for run in &display_runs {
            let font_id = window.text_system().resolve_font(&run.font);
            match fonts.last_mut() {
                Some(last) if last.font_id == font_id => last.len += run.len,
                _ => fonts.push(FontRun {
                    len: run.len,
                    font_id,
                }),
            }
        }
        let paint_runs: Arc<[PaintRun]> = display_runs
            .iter()
            .map(|run| PaintRun {
                len: run.len,
                color: run.color,
                background: run.background_color,
                underline: run.underline,
                strikethrough: run.strikethrough,
            })
            .collect();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(
            style
                .line_height
                .to_pixels(font_size.into(), window.rem_size()),
        );
        let spec = TypesetSpec {
            text,
            fonts: fonts.into(),
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
            paint_runs,
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
                let clamp_height = fitted
                    .line_clamp
                    .and_then(|clamp| clamp_height(&layout, clamp));
                Some(Prepared {
                    layout,
                    offset,
                    pieces: fitted.pieces.clone(),
                    clamp_height,
                })
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
        let Some(prepared) = prepaint.as_ref() else {
            return;
        };
        let layout = prepared.layout.as_ref();
        let origin = bounds.origin + point(prepared.offset, px(0.0));
        let runs = state.paint_runs.clone();
        let pieces = prepared.pieces.clone();
        let paint = |window: &mut Window| {
            paint_backgrounds(layout, &runs, &pieces, origin, window);
            let color_at = |index| run_at(&runs, source_index(&pieces, index)).color;
            if let Err(error) = gpui::paint_paragraph(layout, origin, color_at, window)
                && let Some(engine) = cx.try_global::<TypesetEngine>()
            {
                engine.parley.report_layout_error(&error);
            }
            paint_decorations(layout, &runs, &pieces, origin, window);
        };
        if let Some(height) = prepared.clamp_height {
            let mut visible = bounds;
            visible.size.height = visible.size.height.min(height);
            window.with_content_mask(Some(ContentMask { bounds: visible }), paint);
        } else {
            paint(window);
        }
    }
}

/// Prepaint state: the reflowed layout, its horizontal placement and its map
/// back to the source runs.
pub struct Prepared {
    layout: Arc<ParagraphLayout>,
    offset: Pixels,
    pieces: Arc<[Piece]>,
    /// Height of the lines a line clamp allows, when the layout has more.
    clamp_height: Option<Pixels>,
}

/// Lines beyond a clamp are laid out but must not show: the height of the
/// allowed lines when the layout has more, whatever the element's height.
fn clamp_height(layout: &ParagraphLayout, clamp: usize) -> Option<Pixels> {
    let first_hidden = layout.parley_layout().lines().nth(clamp)?;
    Some(px(first_hidden.metrics().block_min_coord))
}

/// The paint run containing a display index of the untruncated text.
fn run_at(runs: &[PaintRun], index: usize) -> &PaintRun {
    let mut end = 0;
    for run in runs {
        end += run.len;
        if index < end {
            return run;
        }
    }
    runs.last().expect("typeset text has at least one run")
}

/// Start of each paint run in display coordinates of the untruncated text.
fn run_ranges(runs: &[PaintRun]) -> impl Iterator<Item = (Range<usize>, &PaintRun)> {
    runs.iter().scan(0, |start, run| {
        let range = *start..*start + run.len;
        *start += run.len;
        Some((range, run))
    })
}

/// Rectangles covering a shown byte range, per line.
fn range_rects(layout: &ParagraphLayout, range: Range<usize>) -> Vec<Bounds<Pixels>> {
    let at = |byte_index, affinity| ParagraphPosition {
        byte_index,
        affinity,
    };
    layout
        .selection_bounds(
            at(range.start, TextAffinity::Downstream),
            at(range.end, TextAffinity::Upstream),
        )
        .unwrap_or_default()
}

fn paint_backgrounds(
    layout: &ParagraphLayout,
    runs: &[PaintRun],
    pieces: &[Piece],
    origin: gpui::Point<Pixels>,
    window: &mut Window,
) {
    for (range, run) in run_ranges(runs) {
        let Some(background) = run.background else {
            continue;
        };
        for shown in shown_ranges(pieces, range) {
            for rect in range_rects(layout, shown) {
                window.paint_quad(fill(
                    Bounds::new(origin + rect.origin, rect.size),
                    background,
                ));
            }
        }
    }
}

/// Underlines and strikethroughs, positioned with GPUI's own formulas per line.
fn paint_decorations(
    layout: &ParagraphLayout,
    runs: &[PaintRun],
    pieces: &[Piece],
    origin: gpui::Point<Pixels>,
    window: &mut Window,
) {
    for (range, run) in run_ranges(runs) {
        if run.underline.is_none() && run.strikethrough.is_none() {
            continue;
        }
        for shown in shown_ranges(pieces, range) {
            for rect in range_rects(layout, shown) {
                let Some(line) = line_metrics(layout, rect) else {
                    continue;
                };
                let x = origin.x + rect.origin.x;
                if let Some(underline) = run.underline {
                    let y = origin.y
                        + line.top
                        + gpui::underline_y_offset(line.height, line.ascent, line.descent);
                    let style = UnderlineStyle {
                        color: underline.color.or(Some(run.color)),
                        ..underline
                    };
                    window.paint_underline(point(x, y), rect.size.width, &style);
                }
                if let Some(strikethrough) = run.strikethrough {
                    let baseline = line.top + line.padding + line.ascent;
                    let y = origin.y + line.top + (line.ascent * 0.5 + (baseline - line.top)) * 0.5;
                    let style = StrikethroughStyle {
                        color: strikethrough.color.or(Some(run.color)),
                        ..strikethrough
                    };
                    window.paint_strikethrough(point(x, y), rect.size.width, &style);
                }
            }
        }
    }
}

struct LineMetrics {
    top: Pixels,
    height: Pixels,
    ascent: Pixels,
    descent: Pixels,
    padding: Pixels,
}

/// Metrics of the line whose box contains a rectangle's vertical center.
fn line_metrics(layout: &ParagraphLayout, rect: Bounds<Pixels>) -> Option<LineMetrics> {
    let center = f32::from(rect.origin.y + rect.size.height / 2.0);
    layout.parley_layout().lines().find_map(|line| {
        let m = line.metrics();
        (center >= m.block_min_coord && center <= m.block_max_coord).then(|| {
            let ascent = m.baseline - m.content_block_min_coord;
            let descent = m.content_block_max_coord - m.baseline;
            LineMetrics {
                top: px(m.block_min_coord),
                height: px(m.line_height),
                ascent: px(ascent),
                descent: px(descent),
                padding: px(m.content_block_min_coord - m.block_min_coord),
            }
        })
    })
}

/// Parley's hard line break inside a paragraph.
const LINE_SEPARATOR: char = '\u{2028}';

/// Parley expresses hard line breaks inside a paragraph as U+2028. The whole
/// text is normalized at once, so a CRLF is one break even when a run boundary
/// falls between `\r` and `\n`; the break takes the style of the run holding
/// `\r`. Run lengths follow the normalized text, and runs left empty are dropped.
fn hard_breaks(text: &SharedString, runs: Vec<TextRun>) -> (SharedString, Vec<TextRun>) {
    if !text.contains(['\n', '\r', '\u{2029}']) {
        return (text.clone(), runs);
    }
    // Normalized offset at each source byte boundary that runs may use.
    let mut offsets = vec![0; text.len() + 1];
    let mut normalized = String::with_capacity(text.len() + 8);
    let mut after_cr = false;
    for (index, ch) in text.char_indices() {
        offsets[index] = normalized.len();
        match ch {
            '\n' if after_cr => {}
            '\n' | '\r' | '\u{2029}' => normalized.push(LINE_SEPARATOR),
            ch => normalized.push(ch),
        }
        after_cr = ch == '\r';
    }
    offsets[text.len()] = normalized.len();
    let mut start = 0;
    let runs = runs
        .into_iter()
        .filter_map(|mut run| {
            let end = start + run.len;
            run.len = offsets[end] - offsets[start];
            start = end;
            (run.len > 0).then_some(run)
        })
        .collect();
    (normalized.into(), runs)
}

/// Everything that determines a typeset element's shaping and fitting.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TypesetSpec {
    pub text: SharedString,
    /// Font runs covering `text`.
    pub fonts: Arc<[FontRun]>,
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

impl OverflowKey {
    fn affix(&self) -> &SharedString {
        match self {
            Self::End(affix) | Self::Start(affix) | Self::Middle(affix) => affix,
        }
    }
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
    /// Map from the shown text back to the untruncated text.
    pub pieces: Arc<[Piece]>,
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
    fonts: Arc<[FontRun]>,
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
    /// Backend font generation the entries were computed under.
    generation: u64,
    shapes: Lru<ShapeKey, Shaped>,
    layouts: Lru<(ShapeKey, Option<Pixels>, AlignKey), Arc<ParagraphLayout>>,
    fits: Lru<(TypesetSpec, Option<Pixels>, Option<Pixels>, SpaceKey), Fitted>,
}

impl Caches {
    fn new(generation: u64) -> Self {
        Self {
            generation,
            shapes: Lru::new(SHAPE_CAPACITY),
            layouts: Lru::new(SHAPE_CAPACITY),
            fits: Lru::new(SHAPE_CAPACITY),
        }
    }
}

/// The installed engine and its caches, stored as a GPUI global.
pub(crate) struct TypesetEngine {
    pub parley: Arc<ParleyTextSystem>,
    caches: RefCell<Caches>,
}

impl Global for TypesetEngine {}

impl TypesetEngine {
    pub fn new(parley: Arc<ParleyTextSystem>) -> Self {
        let generation = parley.font_generation();
        Self {
            parley,
            caches: RefCell::new(Caches::new(generation)),
        }
    }

    /// The caches, emptied first if fonts were added since they were filled:
    /// adding a fallback can keep a descriptor's `FontId` while changing the
    /// faces its text resolves to, so no cache key would otherwise notice.
    fn caches(&self) -> std::cell::RefMut<'_, Caches> {
        let generation = self.parley.font_generation();
        let mut caches = self.caches.borrow_mut();
        if caches.generation != generation {
            *caches = Caches::new(generation);
        }
        caches
    }

    /// Shape `text` with font runs covering it and the spec's size and options,
    /// reusing earlier results.
    pub fn shape(&self, spec: &TypesetSpec, text: &str, fonts: Arc<[FontRun]>) -> Result<Shaped> {
        let key = ShapeKey {
            text: if text == spec.text.as_ref() {
                spec.text.clone()
            } else {
                SharedString::from(text.to_owned())
            },
            fonts,
            font_size: spec.font_size,
            options: spec.options,
        };
        if let Some(shaped) = self.caches().shapes.get(&key) {
            return Ok(shaped);
        }
        // Shaping runs outside the cache borrow.
        let shape = Arc::new(self.parley.prepare_paragraph_with_options(
            text,
            spec.font_size,
            &key.fonts,
            spec.options,
        )?);
        let shaped = Shaped { key, shape };
        self.caches()
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
        if let Some(layout) = self.caches().layouts.get(&key) {
            return Ok(layout);
        }
        let layout = Arc::new(shaped.shape.layout_with(break_width, align.parley())?);
        self.caches().layouts.insert(key, layout.clone());
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
        if let Some(fitted) = self.caches().fits.get(&key) {
            return Ok(fitted);
        }
        let fitted = self.fit_uncached(spec, known, available)?;
        self.caches().fits.insert(key, fitted.clone());
        Ok(fitted)
    }

    fn fit_uncached(
        &self,
        spec: &TypesetSpec,
        known: Size<Option<Pixels>>,
        available: Size<AvailableSpace>,
    ) -> Result<Fitted> {
        let full = self.shape(spec, &spec.text, spec.fonts.clone())?;
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
            let mut shape = (full, identity(spec.text.len()));
            let mut lines = self.layout(&shape.0, Some(width), spec.align)?.line_count();
            if let Some(clamp) = spec.line_clamp
                && lines > clamp
            {
                if let Some(overflow) = &spec.overflow {
                    shape = self.truncate_lines(spec, &shape.0, width, clamp, overflow)?;
                }
                lines = clamp;
            }
            (shape, width, lines)
        } else {
            let hard_lines = hard_line_ranges(&spec.text);
            let clamped = spec.line_clamp.filter(|clamp| hard_lines.len() > *clamp);
            let kept = clamped.unwrap_or(hard_lines.len());
            let overflows = limit.is_some_and(|limit| max > limit);
            match &spec.overflow {
                Some(overflow) if overflows || clamped.is_some() => {
                    let shape = self.truncate_hard_lines(
                        spec,
                        &full,
                        &hard_lines[..kept],
                        limit,
                        overflow,
                    )?;
                    let width = if overflows {
                        limit.unwrap_or(max)
                    } else {
                        shape.0.shape.content_widths().1
                    };
                    (shape, known.width.unwrap_or(width.ceil()), kept)
                }
                // Without an affix, lines beyond the clamp stay in the layout and
                // paint masks them to the allowed lines.
                _ => (
                    (full, identity(spec.text.len())),
                    known.width.unwrap_or(max.ceil()),
                    kept,
                ),
            }
        };
        let (shape, pieces) = shape;
        let height = known
            .height
            .unwrap_or(spec.line_height * lines.max(1) as f32);
        Ok(Fitted {
            shaped: shape,
            pieces,
            size: size(width, height),
            wrap: spec.wrap,
            align: spec.align,
            line_clamp: spec.line_clamp,
        })
    }

    /// Width of the affix in the font of the text it joins.
    fn affix_width(&self, spec: &TypesetSpec, affix: &str, anchor: usize) -> Result<Pixels> {
        if affix.is_empty() {
            return Ok(px(0.0));
        }
        let fonts: Arc<[FontRun]> = Arc::new([FontRun {
            len: affix.len(),
            font_id: font_at(&spec.fonts, anchor),
        }]);
        Ok(self.shape(spec, affix, fonts)?.shape.content_widths().1)
    }

    /// Shorten unbroken text line by line. `lines` are the hard-break lines to
    /// keep; when they are fewer than the text's, the last kept line ends in the
    /// affix, as wrapped text does under a line clamp. Each line wider than
    /// `limit` is cut on its own, keeping its logical start, end or both, so a
    /// cut never removes or merges other lines. Truncation is logical, so the
    /// base direction places the affix on the correct side.
    fn truncate_hard_lines(
        &self,
        spec: &TypesetSpec,
        full: &Shaped,
        lines: &[Range<usize>],
        limit: Option<Pixels>,
        overflow: &OverflowKey,
    ) -> Result<(Shaped, Arc<[Piece]>)> {
        let text = spec.text.as_ref();
        let spec = &resolved_direction(spec, full);
        let clusters = full.shape.logical_clusters();
        let dropped = lines.last().is_some_and(|last| last.end < text.len());
        let mut shown = String::new();
        let mut pieces: Vec<Piece> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                // The separator between this line and the previous one.
                let separator = lines[index - 1].end..line.start;
                push_copied(&mut pieces, shown.len(), separator.clone());
                shown.push_str(&text[separator]);
            }
            let ends_clamp = dropped && index + 1 == lines.len();
            let (line_text, line_pieces) = self.truncate_hard_line(
                spec,
                &clusters,
                line.clone(),
                limit,
                overflow,
                ends_clamp,
            )?;
            for piece in line_pieces {
                let len = piece.shown.len();
                let start = shown.len() + piece.shown.start;
                if piece.copied {
                    push_copied(&mut pieces, start, piece.source..piece.source + len);
                } else {
                    pieces.push(Piece {
                        shown: start..start + len,
                        ..piece
                    });
                }
            }
            shown.push_str(&line_text);
        }
        let fonts = mapped_fonts(&spec.fonts, &pieces);
        let shaped = self.shape(spec, &shown, fonts)?;
        Ok((shaped, pieces.into()))
    }

    /// One hard-break line of `spec.text`, cut to `limit` if it is wider, or
    /// ending in the affix if `ends_clamp`. Pieces map back to `spec.text`.
    fn truncate_hard_line(
        &self,
        spec: &TypesetSpec,
        clusters: &[(Range<usize>, f32)],
        line: Range<usize>,
        limit: Option<Pixels>,
        overflow: &OverflowKey,
        ends_clamp: bool,
    ) -> Result<(String, Vec<Piece>)> {
        let text = &spec.text[line.clone()];
        let local: Vec<(Range<usize>, f32)> = clusters
            .iter()
            .filter(|(range, _)| range.start >= line.start && range.end <= line.end)
            .map(|(range, advance)| (range.start - line.start..range.end - line.start, *advance))
            .collect();
        let natural: f32 = local.iter().map(|(_, advance)| advance).sum();
        let overflows = limit.is_some_and(|limit| px(natural) > limit);
        let whole = || {
            let piece = Piece {
                shown: 0..text.len(),
                source: line.start,
                copied: true,
            };
            Ok((text.to_owned(), vec![piece]))
        };
        if !overflows && !ends_clamp {
            return whole();
        }
        // A clamp always marks the end of the last kept line, as in wrapped text.
        let (affix, from) = match overflow {
            _ if ends_clamp => (overflow.affix(), Cut::End),
            OverflowKey::End(affix) => (affix, Cut::End),
            OverflowKey::Start(affix) => (affix, Cut::Start),
            OverflowKey::Middle(affix) => (affix, Cut::Middle),
        };
        if ends_clamp && affix.is_empty() && !overflows {
            return whole();
        }
        let anchor = match from {
            Cut::Start => line.start,
            Cut::End | Cut::Middle => line.end.saturating_sub(1).max(line.start),
        };
        let mut keep = match limit {
            Some(limit) => {
                let budget = f32::from(limit - self.affix_width(spec, affix, anchor)?);
                estimate_kept(&local, budget, from)
            }
            None => local.len(),
        };
        loop {
            let (candidate, mut pieces) = truncated(text, &local, keep, affix, from);
            for piece in &mut pieces {
                piece.source += line.start;
            }
            let Some(limit) = limit else {
                return Ok((candidate, pieces));
            };
            let fonts = mapped_fonts(&spec.fonts, &pieces);
            let shaped = self.shape(spec, &candidate, fonts)?;
            if shaped.shape.content_widths().1 <= limit || keep == 0 {
                return Ok((candidate, pieces));
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
    ) -> Result<(Shaped, Arc<[Piece]>)> {
        let text = spec.text.as_ref();
        let affix = overflow.affix();
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
        let anchor = last.end.saturating_sub(1);
        let budget = f32::from(width - self.affix_width(spec, affix, anchor)?);
        let mut keep = line_start + estimate_kept(&clusters[line_start..], budget, Cut::End);
        loop {
            let (candidate, pieces) = truncated(text, &clusters, keep, affix, Cut::End);
            let fonts = mapped_fonts(&spec.fonts, &pieces);
            let shaped = self.shape(spec, &candidate, fonts)?;
            let lines = self.layout(&shaped, Some(width), spec.align)?.line_count();
            if lines <= clamp || keep == 0 {
                return Ok((shaped, pieces.into()));
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

/// A span of the shown text: copied from the untruncated text at `source`, or
/// an affix styled like the untruncated text at `source`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Piece {
    pub shown: Range<usize>,
    pub source: usize,
    pub copied: bool,
}

/// Byte ranges of the hard-break lines in normalized text, without separators.
fn hard_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, separator) in text.match_indices(LINE_SEPARATOR) {
        lines.push(start..index);
        start = index + separator.len();
    }
    lines.push(start..text.len());
    lines
}

/// Append a copied piece at `shown_start`, extending the previous piece when
/// both copy contiguous source text.
fn push_copied(pieces: &mut Vec<Piece>, shown_start: usize, source: Range<usize>) {
    if source.is_empty() {
        return;
    }
    if let Some(last) = pieces.last_mut()
        && last.copied
        && last.shown.end == shown_start
        && last.source + last.shown.len() == source.start
    {
        last.shown.end += source.len();
        return;
    }
    pieces.push(Piece {
        shown: shown_start..shown_start + source.len(),
        source: source.start,
        copied: true,
    });
}

fn identity(len: usize) -> Arc<[Piece]> {
    Arc::new([Piece {
        shown: 0..len,
        source: 0,
        copied: true,
    }])
}

/// The untruncated index whose style applies at a shown index.
fn source_index(pieces: &[Piece], shown: usize) -> usize {
    pieces
        .iter()
        .find(|piece| piece.shown.contains(&shown))
        .or(pieces.last())
        .map_or(shown, |piece| {
            if piece.copied {
                piece.source + (shown - piece.shown.start)
            } else {
                piece.source
            }
        })
}

/// Shown ranges that display part of an untruncated range.
fn shown_ranges(pieces: &[Piece], source: Range<usize>) -> Vec<Range<usize>> {
    pieces
        .iter()
        .filter_map(|piece| {
            if piece.copied {
                let start = source.start.max(piece.source);
                let end = source.end.min(piece.source + piece.shown.len());
                (start < end).then(|| {
                    let shift = piece.shown.start as isize - piece.source as isize;
                    (start as isize + shift) as usize..(end as isize + shift) as usize
                })
            } else {
                source.contains(&piece.source).then(|| piece.shown.clone())
            }
        })
        .collect()
}

/// The font covering an untruncated index.
fn font_at(fonts: &[FontRun], index: usize) -> FontId {
    let mut end = 0;
    for run in fonts {
        end += run.len;
        if index < end {
            return run.font_id;
        }
    }
    fonts
        .last()
        .map(|run| run.font_id)
        .expect("typeset text has a font run")
}

/// Font runs for shown text, following each piece back to the untruncated runs.
fn mapped_fonts(fonts: &[FontRun], pieces: &[Piece]) -> Arc<[FontRun]> {
    let mut mapped: Vec<FontRun> = Vec::new();
    let mut push = |len: usize, font_id: FontId| {
        if len == 0 {
            return;
        }
        match mapped.last_mut() {
            Some(last) if last.font_id == font_id => last.len += len,
            _ => mapped.push(FontRun { len, font_id }),
        }
    };
    for piece in pieces {
        if !piece.copied {
            push(piece.shown.len(), font_at(fonts, piece.source));
            continue;
        }
        let (mut at, end) = (piece.source, piece.source + piece.shown.len());
        let mut run_start = 0;
        for run in fonts {
            let run_end = run_start + run.len;
            if run_end > at && run_start < end {
                let next = run_end.min(end);
                push(next - at, run.font_id);
                at = next;
            }
            run_start = run_end;
        }
    }
    mapped.into()
}

/// The text keeping `keep` clusters plus the affix, trimming whitespace at the
/// cut, with the pieces mapping it back to `text`.
fn truncated(
    text: &str,
    clusters: &[(Range<usize>, f32)],
    keep: usize,
    affix: &str,
    from: Cut,
) -> (String, Vec<Piece>) {
    let keep = keep.min(clusters.len());
    let head_end = |count: usize| {
        let end = clusters
            .get(count.wrapping_sub(1))
            .map_or(0, |(r, _)| r.end);
        text[..end].trim_end().len()
    };
    let tail_start = |count: usize| {
        let start = clusters
            .len()
            .checked_sub(count)
            .and_then(|index| clusters.get(index))
            .map_or(text.len(), |(r, _)| r.start);
        text.len() - text[start..].trim_start().len()
    };
    // The affix takes the style of the character it follows (or precedes).
    let before = |end: usize| {
        text[..end]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    };
    let mut shown = String::new();
    let mut pieces = Vec::new();
    let mut copy = |shown: &mut String, range: Range<usize>| {
        if !range.is_empty() {
            pieces.push(Piece {
                shown: shown.len()..shown.len() + range.len(),
                source: range.start,
                copied: true,
            });
            shown.push_str(&text[range]);
        }
    };
    let (head, tail) = match from {
        Cut::End => (head_end(keep), text.len()),
        Cut::Start => (0, tail_start(keep)),
        Cut::Middle => (head_end(keep.div_ceil(2)), tail_start(keep / 2)),
    };
    copy(&mut shown, 0..head);
    let anchor = if head > 0 {
        before(head)
    } else {
        tail.min(text.len().saturating_sub(1))
    };
    let affix_start = shown.len();
    shown.push_str(affix);
    copy(&mut shown, tail..text.len());
    if !affix.is_empty() {
        pieces.push(Piece {
            shown: affix_start..affix_start + affix.len(),
            source: anchor,
            copied: false,
        });
        pieces.sort_by_key(|piece| piece.shown.start);
    }
    (shown, pieces)
}

#[cfg(test)]
#[path = "typeset/tests.rs"]
mod tests;
