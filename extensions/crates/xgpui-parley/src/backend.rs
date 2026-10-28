use crate::{FaceRegistry, GlyphRasterizer, line_layout};
use anyhow::{Context, Result, ensure};
use gpui::{
    Bounds, DevicePixels, Font, FontId, FontMetrics, FontRun, GlyphId, LineLayout, Pixels,
    PlatformTextSystem, RenderGlyphParams, ShapedGlyph, ShapedRun, Size, TextRenderingMode, point,
    px,
};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, Layout, LayoutContext,
    StyleProperty,
};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    ops::Range,
    sync::{Arc, Mutex, RwLock},
};

/// Experimental flat-LTR backend with an opt-in paragraph path. Select fonts before constructing GPUI;
/// GPUI owns the font-metric, line-layout, wrapping and final raster caches.
pub struct ParleyTextSystem {
    state: Mutex<State>,
    faces: RwLock<FaceRegistry>,
    raster: Mutex<GlyphRasterizer>,
    #[cfg(test)]
    raster_calls: std::sync::atomic::AtomicUsize,
    rejected: RejectedFace,
}

// Permanent .notdef instance; never aliases a requested descriptor.
struct RejectedFace {
    id: FontId,
    metrics: FontMetrics,
    advance: f32,
}

/// Called on an uncached rejected layout, outside the backend lock. No source
/// text is included in the diagnostic. Install before constructing GPUI caches.
pub type LayoutErrorHandler = Arc<dyn Fn(&anyhow::Error) + Send + Sync>;

struct State {
    fonts: FontContext,
    layout: LayoutContext<()>,
    default_family: String,
    // Identity bindings, not a second layout/raster cache. Old bindings remain
    // valid for in-flight layouts after GPUI invalidates its font generation.
    bindings: HashMap<FontId, Font>,
    // Reuse historical aliases when a descriptor resolves to the same face.
    aliases: HashMap<Font, HashMap<FontId, FontId>>,
    // Only current-generation descriptors; old ID bindings above remain valid.
    descriptors: HashMap<Font, FontId>,
    generation: u64,
    missing_sink: Option<Arc<dyn gpui::MissingGlyphSink>>,
    layout_error_handler: LayoutErrorHandler,
    #[cfg(test)]
    shape_calls: usize,
}

impl ParleyTextSystem {
    /// Use an existing Parley font context and an explicit application default.
    /// `.SystemUIFont` maps to this family; native system-family discovery is not
    /// implied. FontContext system discovery requires the `system-fonts` feature.
    pub fn new(mut fonts: FontContext, default_family: impl Into<String>) -> Result<Self> {
        let default_family = default_family.into();
        ensure!(
            fonts.collection.family_by_name(&default_family).is_some(),
            "default family is unavailable"
        );
        let mut state = State {
            fonts,
            default_family,
            layout: LayoutContext::new(),
            bindings: HashMap::new(),
            aliases: HashMap::new(),
            descriptors: HashMap::new(),
            generation: 0,
            missing_sink: None,
            layout_error_handler: Arc::new(|error| eprintln!("Parley layout rejected: {error:#}")),
            #[cfg(test)]
            shape_calls: 0,
        };
        // Resolve once, in the same registry as every shaped/rasterized face.
        // Construction fails if the explicit default cannot provide real metrics.
        let layout = state.shape(" ", px(16.0), &[])?;
        let run = layout
            .lines()
            .next()
            .context("missing default line")?
            .runs()
            .next()
            .context("missing default face")?;
        let mut faces = FaceRegistry::default();
        let id = faces.register(&run)?;
        let face = faces.get(id)?;
        let metrics = face.metrics()?;
        let advance = face.advance(GlyphId(0))?.width;
        ensure!(
            advance.is_finite() && advance > 0.0,
            "invalid default .notdef advance"
        );
        let rejected = RejectedFace {
            id,
            metrics,
            advance,
        };
        Ok(Self {
            state: Mutex::new(state),
            faces: RwLock::new(faces),
            raster: Mutex::new(GlyphRasterizer::default()),
            #[cfg(test)]
            raster_calls: std::sync::atomic::AtomicUsize::new(0),
            rejected,
        })
    }

    // Clone the immutable face handle under a short read lock; font parsing and
    // raster work must never retain the registry lock or acquire shaping state.
    fn face(&self, id: FontId) -> Result<Arc<crate::RegisteredFace>> {
        self.faces.read().unwrap().shared(id)
    }

    /// Replace the rejection diagnostic (default: stderr). Rejections remain
    /// fallible through `try_layout_line`; only GPUI's infallible hook substitutes.
    pub fn set_layout_error_handler(&self, handler: LayoutErrorHandler) {
        self.state.lock().unwrap().layout_error_handler = handler;
    }

    fn rejected_line(&self, text: &str, size: Pixels) -> LineLayout {
        let size = if f32::from(size).is_finite() && size > px(0.0) {
            size
        } else {
            px(16.0)
        };
        let count = text.chars().count();
        let mut size = size;
        let mut scale = f32::from(size) / self.rejected.metrics.units_per_em as f32;
        let max_units = (self.rejected.advance * count as f32)
            .max(self.rejected.metrics.ascent.abs())
            .max(self.rejected.metrics.descent.abs());
        if !(max_units * scale).is_finite() {
            size = px(16.0);
            scale = f32::from(size) / self.rejected.metrics.units_per_em as f32;
        }
        let advance = px(self.rejected.advance * scale);
        let glyphs = text
            .char_indices()
            .enumerate()
            .map(|(offset, (index, _))| ShapedGlyph {
                id: GlyphId(0),
                position: point(advance * offset as f32, px(0.0)),
                index,
                is_emoji: false,
            })
            .collect();
        LineLayout {
            font_size: size,
            width: advance * count as f32,
            ascent: px(self.rejected.metrics.ascent * scale),
            descent: px(self.rejected.metrics.descent.abs() * scale),
            runs: vec![ShapedRun {
                font_id: self.rejected.id,
                glyphs,
            }],
            len: text.len(),
        }
    }

    /// Fallible equivalent of the infallible GPUI layout hook. Validate unsupported
    /// input here before using the backend through GPUI (RTL, synthetic styles,
    /// paragraph breaks and invalid style ranges return errors).
    pub fn try_layout_line(
        &self,
        text: &str,
        size: Pixels,
        runs: &[FontRun],
    ) -> Result<LineLayout> {
        let mut state = self.state.lock().unwrap();
        let styles = state.styles(text, runs)?;
        let layout = state.shape(text, size, &styles)?;
        let result = line_layout(&layout, size, |run| {
            self.faces.write().unwrap().register(run)
        })?;
        let sink = state.missing_sink.clone();
        let mut missing = Vec::new();
        if sink.is_some() && !text.is_empty() {
            use skrifa::MetadataProvider;
            for line in layout.lines() {
                for run in line.runs() {
                    let font =
                        skrifa::FontRef::from_index(run.font().data.data(), run.font().index)?;
                    let class = if font
                        .metrics(
                            skrifa::instance::Size::unscaled(),
                            skrifa::instance::LocationRef::default(),
                        )
                        .is_monospace
                    {
                        gpui::FallbackFontClass::Monospace
                    } else {
                        gpui::FallbackFontClass::Proportional
                    };
                    for cluster in run.visual_clusters() {
                        if cluster.glyphs().any(|glyph| glyph.id == 0) {
                            missing.push(gpui::MissingGlyph::new(
                                text[cluster.text_range()].to_owned().into(),
                                class,
                            ));
                        }
                    }
                }
            }
        }
        drop(state); // Never call an external sink while holding backend state.
        if let Some(sink) = sink
            && !missing.is_empty()
        {
            sink.report(missing);
        }
        Ok(result)
    }
    /// Extension-only prototype, automatic base direction, one logical paragraph.
    /// U+2028 is a hard line break; document/paragraph separators are rejected.
    /// Reuses backend contexts/face registry. GPUI owns paragraph caching through
    /// the optional PlatformTextSystem hook; this method itself adds no cache.
    pub fn prepare_paragraph(
        &self,
        text: &str,
        size: Pixels,
        runs: &[FontRun],
    ) -> Result<crate::ParagraphShape> {
        ensure!(
            !text.contains(['\n', '\r', '\u{2029}']),
            "expected one logical paragraph"
        );
        let mut state = self.state.lock().unwrap();
        let styles = state.styles(text, runs)?;
        let mut layout = state.build(text, size, &styles)?;
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        crate::validate_layout_geometry(&layout)?;
        let mut faces = Vec::new();
        let mut seen_faces = HashSet::new();
        let mut face_runs = Vec::new();
        let mut registry = self.faces.write().unwrap();
        for line in layout.lines() {
            for run in line.runs() {
                let id = registry.register(&run)?;
                face_runs.push((run.text_range(), id));
                if seen_faces.insert(id) {
                    faces.push(id);
                }
            }
        }
        drop(registry);
        face_runs.sort_by_key(|(range, _)| range.start);
        Ok(crate::ParagraphShape::new(
            Arc::from(text),
            layout,
            faces,
            face_runs,
            size,
        ))
    }
}

impl State {
    fn styles(&self, text: &str, runs: &[FontRun]) -> Result<Vec<(Font, Range<usize>)>> {
        let mut start: usize = 0;
        let mut styles = Vec::with_capacity(runs.len());
        for run in runs {
            let end = start.checked_add(run.len).context("font run overflow")?;
            ensure!(
                end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end),
                "invalid font run range"
            );
            let font = self
                .bindings
                .get(&run.font_id)
                .context("unknown requested font ID")?;
            styles.push((font.clone(), start..end));
            start = end;
        }
        ensure!(
            runs.is_empty() || start == text.len(),
            "font runs must cover the line"
        );
        Ok(styles)
    }

    fn build(
        &mut self,
        text: &str,
        size: Pixels,
        styles: &[(Font, Range<usize>)],
    ) -> Result<Layout<()>> {
        ensure!(
            f32::from(size).is_finite() && size > px(0.0),
            "invalid font size"
        );
        #[cfg(test)]
        {
            self.shape_calls += 1;
        }
        let mut builder = self.layout.ranged_builder(&mut self.fonts, text, 1.0, true);
        builder.push_default(StyleProperty::FontFamily(FontFamily::named(
            &self.default_family,
        )));
        builder.push_default(StyleProperty::FontSize(f32::from(size)));
        for (font, range) in styles {
            let primary = if font.family.as_ref() == ".SystemUIFont" {
                &self.default_family
            } else {
                font.family.as_ref()
            };
            let fallbacks = font
                .fallbacks
                .as_ref()
                .map(|f| f.fallback_list())
                .unwrap_or(&[]);
            let family = if fallbacks.is_empty() {
                FontFamily::named(primary)
            } else {
                let mut families = Vec::with_capacity(1 + fallbacks.len());
                families.push(FontFamilyName::named(primary));
                families.extend(fallbacks.iter().map(|name| FontFamilyName::named(name)));
                FontFamily::List(Cow::Owned(families))
            };
            builder.push(StyleProperty::FontFamily(family), range.clone());
            builder.push(
                StyleProperty::FontWeight(parley::FontWeight::new(font.weight.0)),
                range.clone(),
            );
            let style = match font.style {
                gpui::FontStyle::Normal => parley::FontStyle::Normal,
                gpui::FontStyle::Italic => parley::FontStyle::Italic,
                gpui::FontStyle::Oblique => parley::FontStyle::Oblique(None),
            };
            builder.push(StyleProperty::FontStyle(style), range.clone());
            let features = font
                .features
                .tag_value_list()
                .iter()
                .map(|(tag, value)| {
                    Ok(parley::FontFeature::new(
                        parley::setting::Tag::parse(tag).context("invalid feature tag")?,
                        (*value).try_into()?,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            builder.push(
                StyleProperty::FontFeatures(parley::FontFeatures::List(Cow::Owned(features))),
                range.clone(),
            );
        }
        Ok(builder.build(text))
    }

    fn shape(
        &mut self,
        text: &str,
        size: Pixels,
        styles: &[(Font, Range<usize>)],
    ) -> Result<Layout<()>> {
        ensure!(
            !text.contains(['\n', '\r', '\u{2028}', '\u{2029}']),
            "expected a single logical line"
        );
        let mut layout = self.build(text, size, styles)?;
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        Ok(layout)
    }
}

impl PlatformTextSystem for ParleyTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        // Validate the entire batch before changing the collection or generation.
        let mut validation =
            parley::fontique::Collection::new(parley::fontique::CollectionOptions {
                shared: false,
                system_fonts: false,
            });
        let blobs = fonts
            .into_iter()
            .map(|bytes| {
                let blob = parley::fontique::Blob::new(Arc::new(bytes.into_owned()));
                let registered = validation.register_fonts(blob.clone(), None);
                ensure!(!registered.is_empty(), "invalid or unnamed font data");
                let families = registered
                    .into_iter()
                    .map(|(id, fonts)| {
                        let name = validation
                            .family_name(id)
                            .context("missing validated family")?
                            .to_owned();
                        let indices = fonts.iter().map(|font| font.index()).collect::<Vec<_>>();
                        Ok((name, indices))
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok((blob, families))
            })
            .collect::<Result<Vec<_>>>()?;
        let mut state = self.state.lock().unwrap();
        for (blob, families) in blobs {
            // Fontique appends even identical memory fonts. Check every face of
            // the blob against its target family, including collection indices.
            // Do not keep a separate index that would itself retain font bytes.
            let duplicate = families.iter().all(|(name, indices)| {
                state.fonts.collection.family_by_name(name).is_some_and(|family| {
                    indices.iter().all(|index| family.fonts().iter().any(|font| {
                        font.index() == *index && matches!(font.source().kind(),
                            parley::fontique::SourceKind::Memory(data) if data.data() == blob.data())
                    }))
                })
            });
            if !duplicate {
                state.fonts.collection.register_fonts(blob, None);
            }
        }
        state.generation += 1;
        state.descriptors.clear();
        Ok(())
    }

    fn set_missing_glyph_sink(&self, sink: Option<Arc<dyn gpui::MissingGlyphSink>>) {
        self.state.lock().unwrap().missing_sink = sink;
    }

    fn all_font_names(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap()
            .fonts
            .collection
            .family_names()
            .map(str::to_owned)
            .collect()
    }

    fn font_id(&self, descriptor: &Font) -> Result<FontId> {
        let mut state = self.state.lock().unwrap();
        if let Some(&id) = state.descriptors.get(descriptor) {
            return Ok(id);
        }
        let family = if descriptor.family.as_ref() == ".SystemUIFont" {
            state.default_family.clone()
        } else {
            descriptor.family.to_string()
        };
        ensure!(
            state.fonts.collection.family_by_name(&family).is_some(),
            "font family is unavailable: {family}"
        );
        let layout = state.shape(" ", px(16.0), &[(descriptor.clone(), 0..1)])?;
        let line = layout
            .lines()
            .next()
            .context("font probe produced no line")?;
        let run = line.runs().next().context("font probe produced no face")?;
        let resolved = self.faces.write().unwrap().register(&run)?;
        let id = if let Some(&id) = state
            .aliases
            .get(descriptor)
            .and_then(|ids| ids.get(&resolved))
        {
            id
        } else {
            let id = self.faces.write().unwrap().alias(resolved)?;
            state.bindings.insert(id, descriptor.clone());
            state
                .aliases
                .entry(descriptor.clone())
                .or_default()
                .insert(resolved, id);
            id
        };
        state.descriptors.insert(descriptor.clone(), id);
        Ok(id)
    }

    fn font_metrics(&self, id: FontId) -> FontMetrics {
        self.face(id)
            .and_then(|face| face.metrics())
            .expect("invalid Parley font metrics")
    }
    fn typographic_bounds(&self, id: FontId, glyph: GlyphId) -> Result<Bounds<f32>> {
        self.face(id)?.typographic_bounds(glyph)
    }
    fn advance(&self, id: FontId, glyph: GlyphId) -> Result<Size<f32>> {
        self.face(id)?.advance(glyph)
    }
    fn glyph_for_char(&self, id: FontId, ch: char) -> Option<GlyphId> {
        self.face(id).ok()?.glyph_for_char(ch)
    }
    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let face = self.face(params.font_id)?;
        self.raster
            .lock()
            .unwrap()
            .raster_bounds_for_face(&face, params)
    }
    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        let face = self.face(params.font_id)?;
        #[cfg(test)]
        self.raster_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.raster
            .lock()
            .unwrap()
            .rasterize_face(&face, params, bounds)
    }
    fn layout_line(&self, text: &str, size: Pixels, runs: &[FontRun]) -> LineLayout {
        match self.try_layout_line(text, size, runs) {
            Ok(line) => line,
            Err(error) => {
                let handler = self.state.lock().unwrap().layout_error_handler.clone();
                handler(&error);
                self.rejected_line(text, size)
            }
        }
    }
    fn shape_paragraph(
        &self,
        request: gpui::ParagraphRequest<'_>,
    ) -> Result<Arc<dyn gpui::PlatformParagraphShape>> {
        Ok(Arc::new(self.prepare_paragraph(
            request.text,
            request.font_size,
            request.runs,
        )?))
    }
    fn recommended_rendering_mode(&self, _: FontId, _: Pixels) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "paragraph/tests.rs"]
mod paragraph_tests;
