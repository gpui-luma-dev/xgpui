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
                Some(Prepared {
                    layout,
                    offset,
                    pieces: fitted.pieces.clone(),
                    clamp: fitted.line_clamp,
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
        let clamped = prepared
            .clamp
            .is_some_and(|lines| layout.line_count() > lines);
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
        if clamped {
            // Lines beyond the clamp are laid out but must not show.
            window.with_content_mask(Some(ContentMask { bounds }), paint);
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
    clamp: Option<usize>,
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

/// Parley expresses hard line breaks inside a paragraph as U+2028. Run lengths
/// are adjusted to the normalized text; a CRLF split across runs normalizes per run.
fn hard_breaks(text: &SharedString, runs: Vec<TextRun>) -> (SharedString, Vec<TextRun>) {
    if !text.contains(['\n', '\r', '\u{2029}']) {
        return (text.clone(), runs);
    }
    let normalize = |part: &str| {
        part.replace("\r\n", "\u{2028}")
            .replace(['\n', '\r', '\u{2029}'], "\u{2028}")
    };
    let mut normalized = String::with_capacity(text.len() + 8);
    let mut start = 0;
    let runs = runs
        .into_iter()
        .map(|mut run| {
            let part = normalize(&text[start..start + run.len]);
            start += run.len;
            run.len = part.len();
            normalized.push_str(&part);
            run
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
        if let Some(shaped) = self.caches.borrow_mut().shapes.get(&key) {
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
            let mut shape = (full, identity(spec.text.len()));
            let mut width = max;
            if let Some(overflow) = &spec.overflow
                && let Some(limit) = limit
                && max > limit
            {
                shape = self.truncate_line(spec, &shape.0, limit, overflow)?;
                width = limit;
            }
            let lines = self.layout(&shape.0, None, spec.align)?.line_count();
            (shape, known.width.unwrap_or(width.ceil()), lines)
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

    /// Shorten an unbroken line to fit `limit`, keeping the logical start, end or
    /// both. Truncation is logical, so the base direction places the affix on the
    /// correct side. Advances estimate the cut; reshaping confirms it.
    fn truncate_line(
        &self,
        spec: &TypesetSpec,
        full: &Shaped,
        limit: Pixels,
        overflow: &OverflowKey,
    ) -> Result<(Shaped, Arc<[Piece]>)> {
        let text = spec.text.as_ref();
        let (affix, from) = match overflow {
            OverflowKey::End(affix) => (affix, Cut::End),
            OverflowKey::Start(affix) => (affix, Cut::Start),
            OverflowKey::Middle(affix) => (affix, Cut::Middle),
        };
        let spec = &resolved_direction(spec, full);
        let clusters = full.shape.logical_clusters();
        let anchor = match from {
            Cut::Start => 0,
            Cut::End | Cut::Middle => text.len().saturating_sub(1),
        };
        let budget = f32::from(limit - self.affix_width(spec, affix, anchor)?);
        let mut keep = estimate_kept(&clusters, budget, from);
        loop {
            let (candidate, pieces) = truncated(text, &clusters, keep, affix, from);
            let fonts = mapped_fonts(&spec.fonts, &pieces);
            let shaped = self.shape(spec, &candidate, fonts)?;
            if shaped.shape.content_widths().1 <= limit || keep == 0 {
                return Ok((shaped, pieces.into()));
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
