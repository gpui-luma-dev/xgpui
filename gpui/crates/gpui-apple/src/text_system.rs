use anyhow::anyhow;
use collections::{HashMap, HashSet};
use core_foundation::{
    array::{CFArray, CFArrayRef},
    attributed_string::CFMutableAttributedString,
    base::{CFHash, CFRange, TCFType},
    dictionary::{CFDictionary, CFDictionaryRef, CFMutableDictionary},
    number::CFNumber,
    string::CFString,
};
use core_graphics::{
    base::{CGFloat, CGGlyph, kCGImageAlphaPremultipliedLast},
    color_space::CGColorSpace,
    context::{CGContext, CGTextDrawingMode},
    geometry::CGPoint,
};
use core_text::{
    font::{CTFont, CTFontRef},
    font_collection::CTFontCollectionRef,
    font_descriptor::{
        CTFontDescriptor, kCTFontSizeAttribute, kCTFontSlantTrait, kCTFontSymbolicTrait,
        kCTFontVariationAttribute, kCTFontWeightTrait, kCTFontWidthTrait,
    },
    line::CTLine,
    string_attributes::kCTFontAttributeName,
};
use font_kit::{
    font::Font as FontKitFont,
    handle::Handle,
    hinting::HintingOptions,
    metrics::Metrics,
    properties::{Style as FontkitStyle, Weight as FontkitWeight},
    source::SystemSource,
    sources::mem::MemSource,
};
use gpui::{
    Bounds, DevicePixels, Font, FontFallbacks, FontFeatures, FontId, FontMetrics, FontRun,
    FontStyle, FontWeight, GlyphId, LineLayout, Pixels, PlatformTextSystem, RenderGlyphParams,
    Result, SUBPIXEL_VARIANTS_X, ShapedGlyph, ShapedRun, SharedString, Size, TextRenderingMode,
    point, px, size, swap_rgba_pa_to_bgra,
};
use parking_lot::{RwLock, RwLockUpgradableReadGuard};
use pathfinder_geometry::{
    rect::{RectF, RectI},
    transform2d::Transform2F,
    vector::Vector2F,
};
use smallvec::SmallVec;
use std::{
    borrow::Cow,
    char,
    convert::TryFrom,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[cfg(target_os = "macos")]
use {
    core_foundation::base::CFType,
    gpui::{Hsla, Rgba},
    std::sync::OnceLock,
};

use crate::open_type::apply_features_and_fallbacks;

#[allow(non_upper_case_globals)]
const kCGImageAlphaOnly: u32 = 7;

/// Apple text system using CoreText for font shaping.
pub struct AppleTextSystem(RwLock<AppleTextSystemState>);

#[derive(Clone, PartialEq, Eq, Hash)]
struct FontKey {
    font_family: SharedString,
    font_features: FontFeatures,
    font_fallbacks: Option<FontFallbacks>,
}

// Descriptor identity retains features/cascade lists and effective variations.
// Point size belongs to the glyph cache key, not the font instance identity.
#[derive(PartialEq, Eq)]
struct NativeFontKey {
    descriptor: CTFontDescriptor,
    coordinates: Vec<(i64, u64)>,
    freeze_variations: bool,
}

impl Hash for NativeFontKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        unsafe { CFHash(self.descriptor.as_CFTypeRef()) }.hash(state);
        self.coordinates.hash(state);
        self.freeze_variations.hash(state);
    }
}

unsafe extern "C" {
    fn CTFontCopyVariation(font: CTFontRef) -> CFDictionaryRef;
    fn CTFontCreateCopyWithAttributes(
        font: CTFontRef,
        size: CGFloat,
        matrix: *const std::ffi::c_void,
        descriptor: core_text::font_descriptor::CTFontDescriptorRef,
    ) -> CTFontRef;
    static kCTFontVariationAxisIdentifierKey: core_foundation::string::CFStringRef;
    static kCTFontOpticalSizeAttribute: core_foundation::string::CFStringRef;
    static kCTFontVariationAxisDefaultValueKey: core_foundation::string::CFStringRef;
    static kCTFontVariationAxisMinimumValueKey: core_foundation::string::CFStringRef;
    static kCTFontVariationAxisMaximumValueKey: core_foundation::string::CFStringRef;
}

fn variations(font: &CTFont) -> CFMutableDictionary<CFNumber, CFNumber> {
    let raw = unsafe { CTFontCopyVariation(font.as_concrete_TypeRef()) };
    if raw.is_null() {
        CFMutableDictionary::new()
    } else {
        let values = unsafe { CFDictionary::<CFNumber, CFNumber>::wrap_under_create_rule(raw) };
        // copy_with_capacity in core-foundation 0.10 uses the wrong ownership rule.
        unsafe {
            CFMutableDictionary::wrap_under_create_rule(
                core_foundation::dictionary::CFDictionaryCreateMutableCopy(
                    std::ptr::null(),
                    0,
                    values.as_concrete_TypeRef(),
                ),
            )
        }
    }
}

impl NativeFontKey {
    fn new(font: &CTFont) -> Self {
        let coords = variations(font);
        let attrs = CFDictionary::from_CFType_pairs(&[
            (
                unsafe { CFString::wrap_under_get_rule(kCTFontSizeAttribute) },
                CFNumber::from(0).as_CFType(),
            ),
            (
                unsafe { CFString::wrap_under_get_rule(kCTFontVariationAttribute) },
                coords.as_CFType(),
            ),
        ]);
        let (tags, values) = coords.get_keys_and_values();
        let mut coordinates = tags
            .into_iter()
            .zip(values)
            .filter_map(|(tag, value)| {
                let tag = unsafe { CFNumber::wrap_under_get_rule(tag as _) }.to_i64()?;
                let value = unsafe { CFNumber::wrap_under_get_rule(value as _) }.to_f64()?;
                Some((
                    tag,
                    if value == 0.0 {
                        0.0f64.to_bits()
                    } else {
                        value.to_bits()
                    },
                ))
            })
            .collect::<Vec<_>>();
        coordinates.sort_unstable();
        Self {
            descriptor: font
                .copy_descriptor()
                .create_copy_with_attributes(attrs.to_untyped())
                .unwrap(),
            coordinates,
            freeze_variations: false,
        }
    }
}

fn copy_with_descriptor(font: &CTFont, descriptor: &CTFontDescriptor) -> CTFont {
    // Copy the actual native face. Resolving a shaped font's descriptor afresh
    // can substitute Helvetica for an embedded face lacking a registered URL.
    unsafe {
        CTFont::wrap_under_create_rule(CTFontCreateCopyWithAttributes(
            font.as_concrete_TypeRef(),
            font.pt_size(),
            std::ptr::null(),
            descriptor.as_concrete_TypeRef(),
        ))
    }
}

fn sized_font(font: &CTFont, size: CGFloat) -> CTFont {
    let coords = variations(font);
    if coords.is_empty() {
        return font.clone_with_font_size(size);
    }
    let mut pairs = vec![(
        unsafe { CFString::wrap_under_get_rule(kCTFontVariationAttribute) },
        coords.as_CFType(),
    )];
    if let Some(optical) = coords.find(&CFNumber::from(u32::from_be_bytes(*b"opsz") as i64)) {
        pairs.push((
            unsafe { CFString::wrap_under_get_rule(kCTFontOpticalSizeAttribute) },
            optical.as_CFType(),
        ));
    }
    let attrs = CFDictionary::from_CFType_pairs(&pairs);
    let descriptor = font
        .copy_descriptor()
        .create_copy_with_attributes(attrs.to_untyped())
        .unwrap();
    unsafe {
        CTFont::wrap_under_create_rule(CTFontCreateCopyWithAttributes(
            font.as_concrete_TypeRef(),
            size,
            std::ptr::null(),
            descriptor.as_concrete_TypeRef(),
        ))
    }
}

fn with_weight(font: &CTFont, weight: FontWeight) -> Result<CTFont> {
    anyhow::ensure!(weight.0.is_finite(), "invalid font weight");
    if let Some(axes) = font.get_variation_axes() {
        for axis in axes.iter() {
            let number = |key| {
                axis.find(key)
                    .and_then(|v| v.downcast::<CFNumber>())
                    .and_then(|v| v.to_f64())
            };
            if number(unsafe { kCTFontVariationAxisIdentifierKey })
                != Some(u32::from_be_bytes(*b"wght") as f64)
            {
                continue;
            }
            let min = number(unsafe { kCTFontVariationAxisMinimumValueKey })
                .ok_or_else(|| anyhow!("missing weight minimum"))?;
            let max = number(unsafe { kCTFontVariationAxisMaximumValueKey })
                .ok_or_else(|| anyhow!("missing weight maximum"))?;
            anyhow::ensure!(
                min.is_finite() && max.is_finite() && min <= max,
                "invalid weight axis range"
            );
            let mut coords = variations(font);
            coords.set(
                CFNumber::from(i32::from_be_bytes(*b"wght")),
                CFNumber::from((weight.0 as f64).clamp(min, max)),
            );
            let attrs = CFDictionary::from_CFType_pairs(&[(
                unsafe { CFString::wrap_under_get_rule(kCTFontVariationAttribute) },
                coords.to_immutable(),
            )]);
            let descriptor = font
                .copy_descriptor()
                .create_copy_with_attributes(attrs.to_untyped())
                .map_err(|_| anyhow!("could not set font weight"))?;
            return Ok(copy_with_descriptor(font, &descriptor));
        }
    }
    Ok(font.clone())
}

fn with_variations(font: &CTFont, settings: &gpui::FontVariations) -> Result<CTFont> {
    let Some(axes) = font.get_variation_axes() else {
        return Ok(font.clone());
    };
    let mut coords = variations(font);
    let mut changed = false;
    for axis in axes.iter() {
        let number = |key| {
            axis.find(key)
                .and_then(|v| v.downcast::<CFNumber>())
                .and_then(|v| v.to_f64())
        };
        let Some(identifier) = number(unsafe { kCTFontVariationAxisIdentifierKey }) else {
            continue;
        };
        let Some((_, value)) = settings
            .settings()
            .iter()
            .find(|(tag, _)| u32::from_be_bytes(*tag.as_bytes()) as f64 == identifier)
        else {
            continue;
        };
        let min = number(unsafe { kCTFontVariationAxisMinimumValueKey })
            .ok_or_else(|| anyhow!("missing axis minimum"))?;
        let max = number(unsafe { kCTFontVariationAxisMaximumValueKey })
            .ok_or_else(|| anyhow!("missing axis maximum"))?;
        anyhow::ensure!(
            min.is_finite() && max.is_finite() && min <= max,
            "invalid axis range"
        );
        let key = CFNumber::from(identifier as i64);
        let target = (*value as f64).clamp(min, max);
        let current = coords
            .find(&key)
            .and_then(|value| value.to_f64())
            .or_else(|| number(unsafe { kCTFontVariationAxisDefaultValueKey }));
        if current != Some(target) {
            coords.set(key, CFNumber::from(target));
            changed = true;
        }
    }
    if !changed {
        return Ok(font.clone());
    }
    let attrs = CFDictionary::from_CFType_pairs(&[(
        unsafe { CFString::wrap_under_get_rule(kCTFontVariationAttribute) },
        coords.as_CFType(),
    )]);
    let descriptor = font
        .copy_descriptor()
        .create_copy_with_attributes(attrs.to_untyped())
        .map_err(|_| anyhow!("could not set font variations"))?;
    Ok(copy_with_descriptor(font, &descriptor))
}

struct AppleTextSystemState {
    memory_source: MemSource,
    system_source: SystemSource,
    fonts: Vec<FontKitFont>,
    font_selections: HashMap<Font, FontId>,
    requested_variations: HashMap<FontId, (gpui::FontVariations, FontWeight)>,
    frozen_variation_ids: HashSet<FontId>,
    font_ids_by_native_font: HashMap<NativeFontKey, FontId>,
    font_ids_by_font_key: HashMap<FontKey, SmallVec<[FontId; 4]>>,
    postscript_names_by_font_id: HashMap<FontId, String>,
}

impl AppleTextSystem {
    /// Create a new AppleTextSystem.
    pub fn new() -> Self {
        Self(RwLock::new(AppleTextSystemState {
            memory_source: MemSource::empty(),
            system_source: SystemSource::new(),
            fonts: Vec::new(),
            font_selections: HashMap::default(),
            requested_variations: HashMap::default(),
            frozen_variation_ids: HashSet::default(),
            font_ids_by_native_font: HashMap::default(),
            font_ids_by_font_key: HashMap::default(),
            postscript_names_by_font_id: HashMap::default(),
        }))
    }
}

impl Default for AppleTextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PlatformTextSystem for AppleTextSystem {
    fn add_fonts(&self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        self.0.write().add_fonts(fonts)
    }

    fn all_font_names(&self) -> Vec<String> {
        let mut names = Vec::new();
        let collection = core_text::font_collection::create_for_all_families();
        // NOTE: We intentionally avoid using `collection.get_descriptors()` here because
        // it has a memory leak bug in core-text v21.0.0. The upstream code uses
        // `wrap_under_get_rule` but `CTFontCollectionCreateMatchingFontDescriptors`
        // follows the Create Rule (caller owns the result), so it should use
        // `wrap_under_create_rule`. We call the function directly with correct memory management.
        unsafe extern "C" {
            fn CTFontCollectionCreateMatchingFontDescriptors(
                collection: CTFontCollectionRef,
            ) -> CFArrayRef;
        }
        let descriptors: Option<CFArray<CTFontDescriptor>> = unsafe {
            let array_ref =
                CTFontCollectionCreateMatchingFontDescriptors(collection.as_concrete_TypeRef());
            if array_ref.is_null() {
                None
            } else {
                Some(CFArray::wrap_under_create_rule(array_ref))
            }
        };
        let Some(descriptors) = descriptors else {
            return names;
        };
        for descriptor in descriptors.into_iter() {
            names.extend(lenient_font_attributes::family_name(&descriptor));
        }
        if let Ok(fonts_in_memory) = self.0.read().memory_source.all_families() {
            names.extend(fonts_in_memory);
        }
        names
    }

    fn font_id(&self, font: &Font) -> Result<FontId> {
        let lock = self.0.upgradable_read();
        if let Some(font_id) = lock.font_selections.get(font) {
            Ok(*font_id)
        } else {
            let mut lock = RwLockUpgradableReadGuard::upgrade(lock);
            let font_key = FontKey {
                font_family: font.family.clone(),
                font_features: font.features.clone(),
                font_fallbacks: font.fallbacks.clone(),
            };
            let candidates = if let Some(font_ids) = lock.font_ids_by_font_key.get(&font_key) {
                font_ids.as_slice()
            } else {
                let font_ids =
                    lock.load_family(&font.family, &font.features, font.fallbacks.as_ref())?;
                lock.font_ids_by_font_key.insert(font_key.clone(), font_ids);
                lock.font_ids_by_font_key[&font_key].as_ref()
            };

            let candidate_properties = candidates
                .iter()
                .map(|font_id| lock.fonts[font_id.0].properties())
                .collect::<SmallVec<[_; 4]>>();

            let ix = font_kit::matching::find_best_match(
                &candidate_properties,
                &font_kit::properties::Properties {
                    style: fontkit_style(font.style),
                    weight: fontkit_weight(font.weight),
                    stretch: Default::default(),
                },
            )?;

            let base_id = candidates[ix];
            let native = lock.fonts[base_id.0].native_font();
            let instance = with_variations(&with_weight(&native, font.weight)?, &font.variations)?;
            let font_id = if font.variations.settings().is_empty() {
                lock.id_for_native_font(instance, false)
            } else {
                // A requested ID must retain axes unsupported by the primary
                // face: a later fallback face may support them.
                let id = FontId(lock.fonts.len());
                lock.postscript_names_by_font_id
                    .insert(id, instance.postscript_name());
                lock.fonts
                    .push(unsafe { FontKitFont::from_native_font(&instance) });
                lock.requested_variations
                    .insert(id, (font.variations.clone(), font.weight));
                lock.frozen_variation_ids.insert(id);
                id
            };
            lock.font_selections.insert(font.clone(), font_id);
            Ok(font_id)
        }
    }

    fn font_capabilities(&self) -> gpui::FontCapabilities {
        gpui::FontCapabilities {
            feature_introspection: true,
            axis_introspection: true,
            static_variations: true,
        }
    }

    fn font_axes(&self, id: FontId) -> Result<gpui::FontQuery<Vec<gpui::VariationAxisInfo>>> {
        let state = self.0.read();
        let font = state
            .fonts
            .get(id.0)
            .ok_or_else(|| anyhow!("invalid font ID"))?
            .native_font();
        drop(state);
        let tables = font
            .get_available_font_tables()
            .ok_or_else(|| anyhow!("font table enumeration failed"))?;
        let table = if tables
            .iter()
            .any(|tag| *tag == u32::from_be_bytes(*b"fvar"))
        {
            Some(
                font.get_font_table(u32::from_be_bytes(*b"fvar"))
                    .ok_or_else(|| anyhow!("font table read failed"))?,
            )
        } else {
            None
        };
        Ok(gpui::FontQuery::Supported(
            gpui_font_inspect::variation_axes(table.as_ref().map(|table| table.bytes()))?,
        ))
    }

    fn opentype_features(
        &self,
        font_id: FontId,
    ) -> Result<gpui::FontQuery<Vec<gpui::OpenTypeFeatureInfo>>> {
        let lock = self.0.read();
        let font = lock
            .fonts
            .get(font_id.0)
            .ok_or_else(|| anyhow!("invalid font ID"))?
            .native_font();
        drop(lock);
        // Enumerate first so a failed read of an advertised table cannot be
        // mistaken for a font with no OpenType layout tables (e.g. AAT-only).
        let tables = font
            .get_available_font_tables()
            .ok_or_else(|| anyhow!("font table enumeration failed"))?;
        let read = |tag: &[u8; 4]| -> Result<_> {
            let tag = u32::from_be_bytes(*tag);
            if tables.iter().any(|value| *value == tag) {
                Ok(Some(
                    font.get_font_table(tag)
                        .ok_or_else(|| anyhow!("font table read failed"))?,
                ))
            } else {
                Ok(None)
            }
        };
        let gsub = read(b"GSUB")?;
        let gpos = read(b"GPOS")?;
        Ok(gpui::FontQuery::Supported(
            gpui_font_inspect::opentype_features(
                gsub.as_ref().map(|data| data.bytes()),
                gpos.as_ref().map(|data| data.bytes()),
            )?,
        ))
    }

    fn font_metrics(&self, font_id: FontId) -> FontMetrics {
        font_kit_metrics_to_metrics(self.0.read().fonts[font_id.0].metrics())
    }

    fn typographic_bounds(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Bounds<f32>> {
        Ok(bounds_from_rect(
            self.0.read().fonts[font_id.0].typographic_bounds(glyph_id.0)?,
        ))
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        self.0.read().advance(font_id, glyph_id)
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.0.read().glyph_for_char(font_id, ch)
    }

    fn glyph_raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        self.0.read().raster_bounds(params)
    }

    fn rasterize_glyph(
        &self,
        glyph_id: &RenderGlyphParams,
        raster_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        self.0.read().rasterize_glyph(glyph_id, raster_bounds)
    }

    fn layout_line(&self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        self.0.write().layout_line(text, font_size, font_runs)
    }

    fn recommended_rendering_mode(
        &self,
        _font_id: FontId,
        _font_size: Pixels,
    ) -> TextRenderingMode {
        TextRenderingMode::Grayscale
    }

    #[cfg(target_os = "macos")]
    fn glyph_dilation_for_color(&self, color: Hsla) -> u8 {
        // When font smoothing is enabled, CoreGraphics thickens glyph strokes by an amount that
        // depends on the foreground color's luminance. We replicate the logic used by CoreGraphics
        // to select between the different levels of dilation.
        if !font_smoothing_allowed_by_user() {
            return 0;
        }
        let rgba: Rgba = color.into();
        let luminance = 0.2126 * rgba.r + 0.7152 * rgba.g + 0.0722 * rgba.b;
        let level = ((4.0 * luminance) + 0.5).floor() as i32;
        level.clamp(0, 4) as u8
    }
}

#[cfg(target_os = "macos")]
fn font_smoothing_allowed_by_user() -> bool {
    static ALLOWED: OnceLock<bool> = OnceLock::new();
    *ALLOWED.get_or_init(|| {
        use core_foundation_sys::preferences::{
            CFPreferencesCopyAppValue, kCFPreferencesCurrentApplication,
        };

        let key = CFString::new("AppleFontSmoothing");
        let value_ref = unsafe {
            CFPreferencesCopyAppValue(key.as_concrete_TypeRef(), kCFPreferencesCurrentApplication)
        };
        if value_ref.is_null() {
            return true;
        }
        let value = unsafe { CFType::wrap_under_create_rule(value_ref) };
        let Some(number) = value.downcast_into::<CFNumber>() else {
            return true;
        };
        // Only an explicit value of `0` means that font smoothing is disabled.
        number.to_i64() != Some(0)
    })
}

impl AppleTextSystemState {
    fn add_fonts(&mut self, fonts: Vec<Cow<'static, [u8]>>) -> Result<()> {
        let fonts = fonts
            .into_iter()
            .map(|bytes| match bytes {
                Cow::Borrowed(embedded_font) => {
                    let data_provider = unsafe {
                        core_graphics::data_provider::CGDataProvider::from_slice(embedded_font)
                    };
                    let font = core_graphics::font::CGFont::from_data_provider(data_provider)
                        .map_err(|()| anyhow!("Could not load an embedded font."))?;
                    let font = font_kit::loaders::core_text::Font::from_core_graphics_font(font);
                    Ok(Handle::from_native(&font))
                }
                Cow::Owned(bytes) => Ok(Handle::from_memory(Arc::new(bytes), 0)),
            })
            .collect::<Result<Vec<_>>>()?;
        self.memory_source.add_fonts(fonts.into_iter())?;
        Ok(())
    }

    fn load_family(
        &mut self,
        name: &str,
        features: &FontFeatures,
        fallbacks: Option<&FontFallbacks>,
    ) -> Result<SmallVec<[FontId; 4]>> {
        let name = gpui::font_name_with_fallbacks(name, ".AppleSystemUIFont");

        let mut font_ids = SmallVec::new();
        let mut native_fonts_seen = HashSet::default();
        let family = self
            .memory_source
            .select_family_by_name(name)
            .or_else(|_| self.system_source.select_family_by_name(name))?;
        for font in family.fonts() {
            let mut font = font.load()?;

            apply_features_and_fallbacks(&mut font, features, fallbacks)?;
            // Family-only cascade descriptors cannot resolve fonts held solely
            // by MemSource. Prepend their actual descriptors, keeping system
            // fallbacks and existing matching behavior for installed families.
            if let Some(fallbacks) = fallbacks {
                let mut descriptors = Vec::new();
                for name in fallbacks.fallback_list() {
                    let Ok(family) = self.memory_source.select_family_by_name(name) else {
                        continue;
                    };
                    let mut candidates = family
                        .fonts()
                        .iter()
                        .filter_map(|handle| handle.load().ok())
                        .collect::<Vec<_>>();
                    let properties = candidates
                        .iter()
                        .map(|font| font.properties())
                        .collect::<Vec<_>>();
                    if let Ok(ix) =
                        font_kit::matching::find_best_match(&properties, &font.properties())
                    {
                        let mut fallback = candidates.swap_remove(ix);
                        apply_features_and_fallbacks(&mut fallback, features, None)?;
                        descriptors.push(fallback.native_font().copy_descriptor());
                    }
                }
                if !descriptors.is_empty() {
                    let native = font.native_font();
                    let key = unsafe {
                        CFString::wrap_under_get_rule(
                            core_text::font_descriptor::kCTFontCascadeListAttribute,
                        )
                    };
                    if let Some(existing) = native
                        .copy_descriptor()
                        .attributes()
                        .find(&key)
                        .and_then(|v| v.downcast::<CFArray>())
                    {
                        let existing = unsafe {
                            CFArray::<CTFontDescriptor>::wrap_under_get_rule(
                                existing.as_concrete_TypeRef(),
                            )
                        };
                        descriptors.extend(existing.iter().map(|descriptor| (*descriptor).clone()));
                    }
                    let attrs = CFDictionary::from_CFType_pairs(&[(
                        key,
                        CFArray::from_CFTypes(&descriptors).as_CFType(),
                    )]);
                    let descriptor = native
                        .copy_descriptor()
                        .create_copy_with_attributes(attrs.to_untyped())
                        .map_err(|_| anyhow!("could not set embedded font cascade"))?;
                    let native = copy_with_descriptor(&native, &descriptor);
                    font = unsafe { FontKitFont::from_native_font(&native) };
                }
            }
            // This block contains a precautionary fix to guard against loading fonts
            // that might cause panics due to `.unwrap()`s up the chain.
            {
                // We use the 'm' character for text measurements in various spots
                // (e.g., the editor). However, at time of writing some of those usages
                // will panic if the font has no 'm' glyph.
                //
                // Therefore, we check up front that the font has the necessary glyph.
                let has_m_glyph = font.glyph_for_char('m').is_some();

                // HACK: The 'Segoe Fluent Icons' font does not have an 'm' glyph,
                // but we need to be able to load it for rendering Windows icons in
                // the Storybook (on macOS).
                let is_segoe_fluent_icons = font.full_name() == "Segoe Fluent Icons";

                if !has_m_glyph && !is_segoe_fluent_icons {
                    // I spent far too long trying to track down why a font missing the 'm'
                    // character wasn't loading. This log statement will hopefully save
                    // someone else from suffering the same fate.
                    log::warn!(
                        "font '{}' has no 'm' character and was not loaded",
                        font.full_name()
                    );
                    continue;
                }
            }

            // We've seen a number of panics in production caused by calling font.properties()
            // which unwraps a downcast to CFNumber. This is an attempt to avoid the panic,
            // and to try and identify the incalcitrant font.
            let traits = font.native_font().all_traits();
            if unsafe {
                !(traits
                    .get(kCTFontSymbolicTrait)
                    .downcast::<CFNumber>()
                    .is_some()
                    && traits
                        .get(kCTFontWidthTrait)
                        .downcast::<CFNumber>()
                        .is_some()
                    && traits
                        .get(kCTFontWeightTrait)
                        .downcast::<CFNumber>()
                        .is_some()
                    && traits
                        .get(kCTFontSlantTrait)
                        .downcast::<CFNumber>()
                        .is_some())
            } {
                log::error!(
                    "Failed to read traits for font {:?} (PostScript name {:?})",
                    font.full_name(),
                    font.postscript_name(),
                );
                continue;
            }

            let Some(postscript_name) = font.postscript_name() else {
                log::warn!(
                    "font {:?} in family {:?} has no PostScript name; skipping",
                    font.full_name(),
                    name,
                );
                continue;
            };
            // Dedup is scoped to this single `load_family` call (issue #55472).
            // The same family can be reloaded later under a different `FontKey`
            // (different features/fallbacks); a global check against
            // `font_ids_by_native_font` would skip every already-registered
            // font and leave the second call's `font_ids` empty.
            if !native_fonts_seen.insert(NativeFontKey::new(&font.native_font())) {
                log::warn!(
                    "skipping duplicate font {:?} with PostScript name {:?} \
                     in family {:?}",
                    font.full_name(),
                    postscript_name,
                    name,
                );
                continue;
            }
            let font_id = FontId(self.fonts.len());
            font_ids.push(font_id);
            self.font_ids_by_native_font
                .insert(NativeFontKey::new(&font.native_font()), font_id);
            self.postscript_names_by_font_id
                .insert(font_id, postscript_name);
            self.fonts.push(font);
        }
        Ok(font_ids)
    }

    fn advance(&self, font_id: FontId, glyph_id: GlyphId) -> Result<Size<f32>> {
        Ok(size_from_vector2f(
            self.fonts[font_id.0].advance(glyph_id.0)?,
        ))
    }

    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId> {
        self.fonts[font_id.0].glyph_for_char(ch).map(GlyphId)
    }

    fn id_for_native_font(&mut self, requested_font: CTFont, freeze_variations: bool) -> FontId {
        let mut key = NativeFontKey::new(&requested_font);
        key.freeze_variations = freeze_variations;
        if let Some(font_id) = self.font_ids_by_native_font.get(&key) {
            *font_id
        } else {
            let font_id = FontId(self.fonts.len());
            self.font_ids_by_native_font.insert(key, font_id);
            if freeze_variations {
                self.frozen_variation_ids.insert(font_id);
            }
            self.postscript_names_by_font_id
                .insert(font_id, requested_font.postscript_name());
            // Keep variations, features and cascade attributes through rasterization.
            self.fonts
                .push(unsafe { FontKitFont::from_native_font(&requested_font) });
            font_id
        }
    }

    fn is_emoji(&self, font_id: FontId) -> bool {
        self.postscript_names_by_font_id
            .get(&font_id)
            .is_some_and(|postscript_name| {
                postscript_name == "AppleColorEmoji" || postscript_name == ".AppleColorEmojiUI"
            })
    }

    fn raster_bounds(&self, params: &RenderGlyphParams) -> Result<Bounds<DevicePixels>> {
        let font = &self.fonts[params.font_id.0];
        let scale = Transform2F::from_scale(params.scale_factor);
        let bounds: Bounds<DevicePixels> = bounds_from_rect_i(font.raster_bounds(
            params.glyph_id.0,
            params.font_size.into(),
            scale,
            HintingOptions::None,
            font_kit::canvas::RasterizationOptions::GrayscaleAa,
        )?);

        // Expand the bounds by 1 pixel on each side to give CG room for anti-aliasing.
        Ok(bounds.dilate(DevicePixels(1)))
    }

    fn rasterize_glyph(
        &self,
        params: &RenderGlyphParams,
        glyph_bounds: Bounds<DevicePixels>,
    ) -> Result<(Size<DevicePixels>, Vec<u8>)> {
        if glyph_bounds.size.width.0 == 0 || glyph_bounds.size.height.0 == 0 {
            anyhow::bail!("glyph bounds are empty");
        } else {
            // Add an extra pixel when the subpixel variant isn't zero to make room for anti-aliasing.
            let mut bitmap_size = glyph_bounds.size;
            if params.subpixel_variant.x > 0 {
                bitmap_size.width += DevicePixels(1);
            }
            if params.subpixel_variant.y > 0 {
                bitmap_size.height += DevicePixels(1);
            }
            let bitmap_size = bitmap_size;

            let mut bytes;
            let cx;
            if params.is_emoji {
                bytes = vec![0; bitmap_size.width.0 as usize * 4 * bitmap_size.height.0 as usize];
                cx = CGContext::create_bitmap_context(
                    Some(bytes.as_mut_ptr() as *mut _),
                    bitmap_size.width.0 as usize,
                    bitmap_size.height.0 as usize,
                    8,
                    bitmap_size.width.0 as usize * 4,
                    &CGColorSpace::create_device_rgb(),
                    kCGImageAlphaPremultipliedLast,
                );
            } else {
                bytes = vec![0; bitmap_size.width.0 as usize * bitmap_size.height.0 as usize];
                cx = CGContext::create_bitmap_context(
                    Some(bytes.as_mut_ptr() as *mut _),
                    bitmap_size.width.0 as usize,
                    bitmap_size.height.0 as usize,
                    8,
                    bitmap_size.width.0 as usize,
                    &CGColorSpace::create_device_gray(),
                    kCGImageAlphaOnly,
                );
            }

            // Move the origin to bottom left and account for scaling, this
            // makes drawing text consistent with the font-kit's raster_bounds.
            cx.translate(
                -glyph_bounds.origin.x.0 as CGFloat,
                (glyph_bounds.origin.y.0 + glyph_bounds.size.height.0) as CGFloat,
            );
            cx.scale(
                params.scale_factor as CGFloat,
                params.scale_factor as CGFloat,
            );

            let subpixel_shift = params
                .subpixel_variant
                .map(|v| v as f32 / SUBPIXEL_VARIANTS_X as f32);
            cx.set_text_drawing_mode(CGTextDrawingMode::CGTextFill);
            cx.set_allows_antialiasing(true);
            cx.set_should_antialias(true);
            cx.set_allows_font_subpixel_positioning(true);
            cx.set_should_subpixel_position_fonts(true);
            cx.set_allows_font_subpixel_quantization(false);
            cx.set_should_subpixel_quantize_fonts(false);

            if cfg!(target_os = "macos") && params.dilation > 0 {
                let luminance = params.dilation as f64 * 0.25;
                cx.set_should_smooth_fonts(true);
                cx.set_gray_fill_color(luminance, 1.0);
            } else {
                cx.set_gray_fill_color(0.0, 1.0);
            }
            let native = self.fonts[params.font_id.0].native_font();
            let native = if self.frozen_variation_ids.contains(&params.font_id) {
                sized_font(&native, f32::from(params.font_size) as CGFloat)
            } else {
                native.clone_with_font_size(f32::from(params.font_size) as CGFloat)
            };
            native.draw_glyphs(
                &[params.glyph_id.0 as CGGlyph],
                &[CGPoint::new(
                    (subpixel_shift.x / params.scale_factor) as CGFloat,
                    (subpixel_shift.y / params.scale_factor) as CGFloat,
                )],
                cx,
            );

            if params.is_emoji {
                // Convert from RGBA with premultiplied alpha to BGRA with straight alpha.
                for pixel in bytes.chunks_exact_mut(4) {
                    swap_rgba_pa_to_bgra(pixel);
                }
            }

            Ok((bitmap_size, bytes))
        }
    }

    fn layout_line(&mut self, text: &str, font_size: Pixels, font_runs: &[FontRun]) -> LineLayout {
        // Construct the attributed string, converting UTF8 ranges to UTF16 ranges.
        let mut string = CFMutableAttributedString::new();
        let mut max_ascent = 0.0f32;
        let mut max_descent = 0.0f32;
        let mut variation_ranges = Vec::new();

        {
            let mut text = text;
            let mut break_ligature = true;
            for run in font_runs {
                let text_run;
                (text_run, text) = text.split_at(run.len);

                let utf16_start = string.char_len(); // insert at end of string
                // note: replace_str may silently ignore codepoints it dislikes (e.g., BOM at start of string)
                string.replace_str(&CFString::new(text_run), CFRange::init(utf16_start, 0));
                let utf16_end = string.char_len();

                let length = utf16_end - utf16_start;
                let cf_range = CFRange::init(utf16_start, length);
                let font = &self.fonts[run.font_id.0];
                if let Some(settings) = self.requested_variations.get(&run.font_id) {
                    variation_ranges.push((cf_range, settings.clone()));
                }

                let font_metrics = font.metrics();
                let font_scale = f32::from(font_size) / font_metrics.units_per_em as f32;
                max_ascent = max_ascent.max(font_metrics.ascent * font_scale);
                max_descent = max_descent.max(-font_metrics.descent * font_scale);

                let font_size = if break_ligature {
                    px(f32::from(font_size).next_up())
                } else {
                    font_size
                };
                unsafe {
                    string.set_attribute(
                        cf_range,
                        kCTFontAttributeName,
                        &if self.frozen_variation_ids.contains(&run.font_id) {
                            sized_font(&font.native_font(), font_size.into())
                        } else {
                            font.native_font().clone_with_font_size(font_size.into())
                        },
                    );
                }
                break_ligature = !break_ligature;
            }
        }
        // Retrieve the glyphs from the shaped line, converting UTF16 offsets to UTF8 offsets.
        let mut line = CTLine::new_with_attributed_string(string.as_concrete_TypeRef());
        if !variation_ranges.is_empty() {
            let mut changed = false;
            for run in line.glyph_runs().into_iter() {
                unsafe extern "C" {
                    fn CTRunGetStringRange(run: core_text::run::CTRunRef) -> CFRange;
                }
                let range = unsafe { CTRunGetStringRange(run.as_concrete_TypeRef()) };
                let Some(attrs) = run.attributes() else {
                    continue;
                };
                let Some(native) =
                    (unsafe { attrs.get(kCTFontAttributeName).downcast::<CTFont>() })
                else {
                    continue;
                };
                for (request_range, (settings, weight)) in &variation_ranges {
                    let start = range.location.max(request_range.location);
                    let end = (range.location + range.length)
                        .min(request_range.location + request_range.length);
                    if start >= end {
                        continue;
                    }
                    let implicit = if settings
                        .settings()
                        .iter()
                        .any(|(tag, _)| tag.as_bytes() == b"wght")
                    {
                        Ok(native.clone())
                    } else {
                        with_weight(&native, *weight)
                    };
                    let adjusted = implicit.and_then(|font| with_variations(&font, settings));
                    match adjusted {
                        Ok(adjusted)
                            if NativeFontKey::new(&native).coordinates
                                != NativeFontKey::new(&adjusted).coordinates =>
                        {
                            unsafe {
                                string.set_attribute(
                                    CFRange::init(start, end - start),
                                    kCTFontAttributeName,
                                    &adjusted,
                                )
                            };
                            changed = true;
                        }
                        Ok(_) => {}
                        Err(error) => log::error!("fallback variations rejected: {error}"),
                    }
                }
            }
            if changed {
                line = CTLine::new_with_attributed_string(string.as_concrete_TypeRef());
            }
        }
        let glyph_runs = line.glyph_runs();
        let mut runs = <Vec<ShapedRun>>::with_capacity(glyph_runs.len() as usize);
        let mut ix_converter = StringIndexConverter::new(text);
        for run in glyph_runs.into_iter() {
            let attributes = run.attributes().unwrap();
            let font = unsafe {
                attributes
                    .get(kCTFontAttributeName)
                    .downcast::<CTFont>()
                    .unwrap()
            };
            unsafe extern "C" {
                fn CTRunGetStringRange(run: core_text::run::CTRunRef) -> CFRange;
            }
            let range = unsafe { CTRunGetStringRange(run.as_concrete_TypeRef()) };
            let freeze_variations = variation_ranges.iter().any(|(requested, _)| {
                range.location < requested.location + requested.length
                    && requested.location < range.location + range.length
            });
            if freeze_variations {
                max_ascent = max_ascent.max(font.ascent() as f32);
                max_descent = max_descent.max(font.descent() as f32);
            }
            let font_id = self.id_for_native_font(font, freeze_variations);

            let glyphs = match runs.last_mut() {
                Some(run) if run.font_id == font_id => &mut run.glyphs,
                _ => {
                    runs.push(ShapedRun {
                        font_id,
                        glyphs: Vec::with_capacity(run.glyph_count().try_into().unwrap_or(0)),
                    });
                    &mut runs.last_mut().unwrap().glyphs
                }
            };
            for ((&glyph_id, position), &glyph_utf16_ix) in run
                .glyphs()
                .iter()
                .zip(run.positions().iter())
                .zip(run.string_indices().iter())
            {
                let glyph_utf16_ix = usize::try_from(glyph_utf16_ix).unwrap();
                if ix_converter.utf16_ix > glyph_utf16_ix {
                    // We cannot reuse current index converter, as it can only seek forward. Restart the search.
                    ix_converter = StringIndexConverter::new(text);
                }
                ix_converter.advance_to_utf16_ix(glyph_utf16_ix);
                glyphs.push(ShapedGlyph {
                    id: GlyphId(glyph_id as u32),
                    position: point(position.x as f32, position.y as f32).map(px),
                    index: ix_converter.utf8_ix,
                    is_emoji: self.is_emoji(font_id),
                });
            }
        }
        let typographic_bounds = line.get_typographic_bounds();
        LineLayout {
            runs,
            font_size,
            width: typographic_bounds.width.into(),
            ascent: max_ascent.into(),
            descent: max_descent.into(),
            len: text.len(),
        }
    }
}

#[derive(Debug, Clone)]
struct StringIndexConverter<'a> {
    text: &'a str,
    /// Index in UTF-8 bytes
    utf8_ix: usize,
    /// Index in UTF-16 code units
    utf16_ix: usize,
}

impl<'a> StringIndexConverter<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            utf8_ix: 0,
            utf16_ix: 0,
        }
    }

    fn advance_to_utf16_ix(&mut self, utf16_target: usize) {
        for (ix, c) in self.text[self.utf8_ix..].char_indices() {
            if self.utf16_ix >= utf16_target {
                self.utf8_ix += ix;
                return;
            }
            self.utf16_ix += c.len_utf16();
        }
        self.utf8_ix = self.text.len();
    }
}

fn font_kit_metrics_to_metrics(metrics: Metrics) -> FontMetrics {
    FontMetrics {
        units_per_em: metrics.units_per_em,
        ascent: metrics.ascent,
        descent: metrics.descent,
        line_gap: metrics.line_gap,
        underline_position: metrics.underline_position,
        underline_thickness: metrics.underline_thickness,
        cap_height: metrics.cap_height,
        x_height: metrics.x_height,
        bounding_box: bounds_from_rect(metrics.bounding_box),
    }
}

fn bounds_from_rect(rect: RectF) -> Bounds<f32> {
    Bounds {
        origin: point(rect.origin_x(), rect.origin_y()),
        size: size(rect.width(), rect.height()),
    }
}

fn bounds_from_rect_i(rect: RectI) -> Bounds<DevicePixels> {
    Bounds {
        origin: point(DevicePixels(rect.origin_x()), DevicePixels(rect.origin_y())),
        size: size(DevicePixels(rect.width()), DevicePixels(rect.height())),
    }
}

// impl From<Vector2I> for Size<DevicePixels> {
//     fn from(value: Vector2I) -> Self {
//         size(value.x().into(), value.y().into())
//     }
// }

// impl From<RectI> for Bounds<i32> {
//     fn from(rect: RectI) -> Self {
//         Bounds {
//             origin: point(rect.origin_x(), rect.origin_y()),
//             size: size(rect.width(), rect.height()),
//         }
//     }
// }

// impl From<Point<u32>> for Vector2I {
//     fn from(size: Point<u32>) -> Self {
//         Vector2I::new(size.x as i32, size.y as i32)
//     }
// }

fn size_from_vector2f(vec: Vector2F) -> Size<f32> {
    size(vec.x(), vec.y())
}

fn fontkit_weight(value: FontWeight) -> FontkitWeight {
    FontkitWeight(value.0)
}

fn fontkit_style(style: FontStyle) -> FontkitStyle {
    match style {
        FontStyle::Normal => FontkitStyle::Normal,
        FontStyle::Italic => FontkitStyle::Italic,
        FontStyle::Oblique => FontkitStyle::Oblique,
    }
}

// Some fonts may have no attributes despite `core_text` requiring them (and panicking).
// This is the same version as `core_text` has without `expect` calls.
mod lenient_font_attributes {
    use core_foundation::{
        base::{CFRetain, CFType, TCFType},
        string::{CFString, CFStringRef},
    };
    use core_text::font_descriptor::{
        CTFontDescriptor, CTFontDescriptorCopyAttribute, kCTFontFamilyNameAttribute,
    };

    pub fn family_name(descriptor: &CTFontDescriptor) -> Option<String> {
        unsafe { get_string_attribute(descriptor, kCTFontFamilyNameAttribute) }
    }

    fn get_string_attribute(
        descriptor: &CTFontDescriptor,
        attribute: CFStringRef,
    ) -> Option<String> {
        unsafe {
            let value = CTFontDescriptorCopyAttribute(descriptor.as_concrete_TypeRef(), attribute);
            if value.is_null() {
                return None;
            }

            let value = CFType::wrap_under_create_rule(value);
            assert!(value.instance_of::<CFString>());
            let s = wrap_under_get_rule(value.as_CFTypeRef() as CFStringRef);
            Some(s.to_string())
        }
    }

    unsafe fn wrap_under_get_rule(reference: CFStringRef) -> CFString {
        unsafe {
            assert!(!reference.is_null(), "Attempted to create a NULL object.");
            let reference = CFRetain(reference as *const ::std::os::raw::c_void) as CFStringRef;
            TCFType::wrap_under_create_rule(reference)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::AppleTextSystem;
    use gpui::{FontFallbacks, FontFeatures, FontRun, GlyphId, PlatformTextSystem, font, px};
    use std::sync::Arc;

    fn static_axes(settings: &[(&str, f32)]) -> gpui::FontVariations {
        gpui::FontVariations::new(
            settings
                .iter()
                .map(|(tag, value)| (tag.parse().unwrap(), *value)),
        )
        .unwrap()
    }

    fn flex_backend() -> AppleTextSystem {
        let fonts = AppleTextSystem::new();
        fonts
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../../../../extensions/crates/xgpui-parley/tests/fonts/RobotoFlex.ttf"
            ))])
            .unwrap();
        fonts
    }

    #[test]
    fn static_axes_preserve_native_instances_and_layout_raster_identity() {
        let fonts = flex_backend();
        let mut descriptor = font("Roboto Flex");
        descriptor.weight = gpui::FontWeight::BOLD;
        descriptor.variations = static_axes(&[
            ("wght", 300.25),
            ("slnt", -10.0),
            ("wdth", 75.0),
            ("opsz", 14.0),
        ]);
        let narrow = fonts.font_id(&descriptor).unwrap();
        descriptor.variations = static_axes(&[
            ("opsz", 14.0),
            ("wdth", 75.0),
            ("slnt", -10.0),
            ("wght", 300.25),
        ]);
        assert_eq!(fonts.font_id(&descriptor).unwrap(), narrow);
        let native = fonts.0.read().fonts[narrow.0].native_font();
        let coords = super::variations(&native);
        for (tag, expected) in [("wght", 300.25), ("slnt", -10.0), ("wdth", 75.0)] {
            let key = core_foundation::number::CFNumber::from(u32::from_be_bytes(
                tag.as_bytes().try_into().unwrap(),
            ) as i64);
            assert_eq!(coords.find(&key).unwrap().to_f64(), Some(expected));
        }
        let sized = native.clone_with_font_size(32.0);
        assert_eq!(
            super::NativeFontKey::new(&native).coordinates,
            super::NativeFontKey::new(&sized).coordinates,
            "resizing must preserve axes"
        );
        let glyph = fonts.glyph_for_char(narrow, 'H').unwrap();
        let params = gpui::RenderGlyphParams {
            font_id: narrow,
            glyph_id: glyph,
            font_size: px(32.0),
            scale_factor: 1.0,
            subpixel_variant: gpui::point(0, 0),
            is_emoji: false,
            subpixel_rendering: false,
            dilation: 0,
        };
        let bounds = fonts.glyph_raster_bounds(&params).unwrap();
        let before = fonts.rasterize_glyph(&params, bounds).unwrap();
        let line = fonts.layout_line(
            "H",
            px(32.0),
            &[FontRun {
                len: 1,
                font_id: narrow,
            }],
        );
        let resolved = line.runs[0].font_id;
        let resolved_native = fonts.0.read().fonts[resolved.0].native_font();
        let requested_key = super::NativeFontKey::new(&native);
        let resolved_key = super::NativeFontKey::new(&resolved_native);
        assert_eq!(
            requested_key.coordinates, resolved_key.coordinates,
            "shaping must retain requested effective axes"
        );
        let metrics = fonts.font_metrics(resolved);
        let expected_width =
            fonts.advance(resolved, glyph).unwrap().width * 32.0 / metrics.units_per_em as f32;
        assert!((f32::from(line.width) - expected_width).abs() < 0.02);
        let line2 = fonts.layout_line(
            "H",
            px(64.0),
            &[FontRun {
                len: 1,
                font_id: narrow,
            }],
        );
        assert!((f32::from(line2.width) - 2.0 * f32::from(line.width)).abs() < 0.02);
        descriptor.variations = static_axes(&[("wght", 900.0), ("wdth", 150.0)]);
        let wide = fonts.font_id(&descriptor).unwrap();
        assert_ne!(wide, narrow);
        assert_ne!(
            fonts.advance(wide, glyph).unwrap().width,
            fonts.advance(narrow, glyph).unwrap().width
        );
        assert_eq!(before, fonts.rasterize_glyph(&params, bounds).unwrap());
        descriptor.variations = static_axes(&[("wght", 5000.0)]);
        let clamped = fonts.font_id(&descriptor).unwrap();
        let native = fonts.0.read().fonts[clamped.0].native_font();
        let weight = core_foundation::number::CFNumber::from(u32::from_be_bytes(*b"wght") as i64);
        assert_eq!(
            super::variations(&native).find(&weight).unwrap().to_f64(),
            Some(1000.0)
        );
        assert!(fonts.font_capabilities().static_variations);
        let gpui::FontQuery::Supported(axes) = fonts.font_axes(narrow).unwrap() else {
            panic!("axis query unsupported")
        };
        assert!(
            axes.iter()
                .any(|axis| axis.tag.to_string() == "slnt" && axis.min_value < 0.0)
        );
    }

    #[test]
    fn static_axes_do_not_change_legacy_sizing_policy() {
        let fonts = flex_backend();
        let mut descriptor = font("Roboto Flex");
        let legacy = fonts.font_id(&descriptor).unwrap();
        let legacy_line = fonts.layout_line(
            "H",
            px(28.0),
            &[FontRun {
                len: 1,
                font_id: legacy,
            }],
        );
        descriptor.variations = static_axes(&[("opsz", 16.0)]);
        let explicit = fonts.font_id(&descriptor).unwrap();
        let explicit_line = fonts.layout_line(
            "H",
            px(28.0),
            &[FontRun {
                len: 1,
                font_id: explicit,
            }],
        );
        let state = fonts.0.read();
        assert!(!state.frozen_variation_ids.contains(&legacy));
        assert!(
            !state
                .frozen_variation_ids
                .contains(&legacy_line.runs[0].font_id)
        );
        assert!(state.frozen_variation_ids.contains(&explicit));
        assert!(
            state
                .frozen_variation_ids
                .contains(&explicit_line.runs[0].font_id)
        );
        assert_ne!(legacy_line.runs[0].font_id, explicit_line.runs[0].font_id);
    }

    #[test]
    fn static_axes_without_weight_or_optical_overrides_keep_the_actual_face() {
        let fonts = flex_backend();
        let mut descriptor = font("Roboto Flex");
        descriptor.variations = static_axes(&[("wght", 100.0), ("wdth", 50.0), ("slnt", -10.0)]);
        let id = fonts.font_id(&descriptor).unwrap();
        let source = fonts.0.read().fonts[id.0].native_font();
        let resized = super::sized_font(&source, f32::from(px(28.0)).next_up() as f64);
        assert_eq!(
            source.family_name(),
            resized.family_name(),
            "resized family"
        );
        assert_eq!(
            super::NativeFontKey::new(&source).coordinates,
            super::NativeFontKey::new(&resized).coordinates,
            "resized coords"
        );
        assert!(fonts.glyph_for_char(id, 'H').is_some());
        let line = fonts.layout_line(
            "Hamburgefonts 012345",
            px(28.0),
            &[FontRun {
                len: 19,
                font_id: id,
            }],
        );
        for run in line.runs {
            let native = fonts.0.read().fonts[run.font_id.0].native_font();
            assert_eq!(source.family_name(), native.family_name());
            assert_eq!(
                super::NativeFontKey::new(&source).coordinates,
                super::NativeFontKey::new(&native).coordinates
            );
        }
    }

    #[test]
    fn static_axes_reach_fallback_faces_even_when_primary_is_static() {
        let fonts = flex_backend();
        fonts
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../../gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
            ))])
            .unwrap();
        let mut primary = font("IBM Plex Sans");
        primary.fallbacks = Some(FontFallbacks::from_fonts(vec!["Roboto Flex".into()]));
        primary.variations = static_axes(&[("slnt", -10.0), ("wght", 800.0)]);
        let primary_id = fonts.font_id(&primary).unwrap();
        let fallback_id = fonts.font_id(&font("Roboto Flex")).unwrap();
        let ch = (0x20..0x3000)
            .filter_map(char::from_u32)
            .find(|&ch| {
                fonts.glyph_for_char(primary_id, ch).is_none()
                    && fonts.glyph_for_char(fallback_id, ch).is_some()
            })
            .expect("fallback-only character");
        let text = ch.to_string();
        let line = fonts.layout_line(
            &text,
            px(24.0),
            &[FontRun {
                len: text.len(),
                font_id: primary_id,
            }],
        );
        let state = fonts.0.read();
        let resolved = state.fonts[line.runs[0].font_id.0].native_font();
        assert!(
            resolved.family_name().contains("Roboto"),
            "U+{:04X}: {}",
            ch as u32,
            resolved.family_name()
        );
        let slant = core_foundation::number::CFNumber::from(u32::from_be_bytes(*b"slnt") as i64);
        assert_eq!(
            super::variations(&resolved).find(&slant).unwrap().to_f64(),
            Some(-10.0)
        );
    }

    #[test]
    fn feature_introspection_reads_pinned_font() {
        let fonts = AppleTextSystem::new();
        fonts
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../../gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
            ))])
            .unwrap();
        let id = fonts.font_id(&font("IBM Plex Sans")).unwrap();
        let gpui::FontQuery::Supported(features) = fonts.opentype_features(id).unwrap() else {
            panic!("Apple feature introspection must be supported")
        };
        assert!(
            features
                .iter()
                .any(|f| f.tag.to_string() == "liga" && f.substitution)
        );
        assert!(
            features
                .iter()
                .any(|f| f.tag.to_string() == "kern" && f.positioning)
        );
        assert!(fonts.opentype_features(gpui::FontId(usize::MAX)).is_err());
    }

    #[test]
    fn test_reload_family_with_features_and_fallbacks() {
        let fonts = AppleTextSystem::new();
        let mut font = font("Helvetica");
        let original_id = fonts.font_id(&font).unwrap();
        font.features = FontFeatures::disable_ligatures();
        font.fallbacks = Some(FontFallbacks(Arc::new(vec!["Times".into()])));
        let configured_id = fonts.font_id(&font).unwrap();
        assert_ne!(original_id, configured_id);
        assert_eq!(fonts.font_id(&font).unwrap(), configured_id);
        assert!(fonts.glyph_for_char(configured_id, 'm').is_some());
        let layout = fonts.layout_line(
            "office",
            px(16.),
            &[FontRun {
                font_id: configured_id,
                len: 6,
            }],
        );
        assert_eq!(layout.len, 6);
        assert!(!layout.runs.is_empty());
        assert!(layout.runs.iter().all(|run| run.font_id == configured_id));
        let original = fonts.layout_line(
            "office",
            px(28.),
            &[FontRun {
                font_id: original_id,
                len: 6,
            }],
        );
        assert!(original.runs.iter().all(|run| run.font_id == original_id));
    }

    #[test]
    fn test_layout_line_bom_char() {
        let fonts = AppleTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();
        let line = "\u{feff}";
        let mut style = FontRun {
            font_id,
            len: line.len(),
        };

        let layout = fonts.layout_line(line, px(16.), &[style]);
        assert_eq!(layout.len, line.len());
        assert!(layout.runs.is_empty());

        let line = "a\u{feff}b";
        style.len = line.len();
        let layout = fonts.layout_line(line, px(16.), &[style]);
        assert_eq!(layout.len, line.len());
        assert_eq!(layout.runs.len(), 1);
        assert_eq!(layout.runs[0].glyphs.len(), 2);
        assert_eq!(layout.runs[0].glyphs[0].id, GlyphId(68u32)); // a
        // There's no glyph for \u{feff}
        assert_eq!(layout.runs[0].glyphs[1].id, GlyphId(69u32)); // b

        let line = "\u{feff}ab";
        let font_runs = &[
            FontRun {
                len: "\u{feff}".len(),
                font_id,
            },
            FontRun {
                len: "ab".len(),
                font_id,
            },
        ];
        let layout = fonts.layout_line(line, px(16.), font_runs);
        assert_eq!(layout.len, line.len());
        assert_eq!(layout.runs.len(), 1);
        assert_eq!(layout.runs[0].glyphs.len(), 2);
        // There's no glyph for \u{feff}
        assert_eq!(layout.runs[0].glyphs[0].id, GlyphId(68u32)); // a
        assert_eq!(layout.runs[0].glyphs[1].id, GlyphId(69u32)); // b
    }

    #[test]
    fn test_layout_line_zwnj_insertion() {
        let fonts = AppleTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();

        let text = "hello world";
        let font_runs = &[
            FontRun { font_id, len: 5 }, // "hello"
            FontRun { font_id, len: 6 }, // " world"
        ];

        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, text.len());

        for run in &layout.runs {
            for glyph in &run.glyphs {
                assert!(
                    glyph.index < text.len(),
                    "Glyph index {} is out of bounds for text length {}",
                    glyph.index,
                    text.len()
                );
            }
        }

        // Test with different font runs - should not insert ZWNJ
        let font_id2 = fonts.font_id(&font("Times")).unwrap_or(font_id);
        let font_runs_different = &[
            FontRun { font_id, len: 5 }, // "hello"
            // " world"
            FontRun {
                font_id: font_id2,
                len: 6,
            },
        ];

        let layout2 = fonts.layout_line(text, px(16.), font_runs_different);
        assert_eq!(layout2.len, text.len());

        for run in &layout2.runs {
            for glyph in &run.glyphs {
                assert!(
                    glyph.index < text.len(),
                    "Glyph index {} is out of bounds for text length {}",
                    glyph.index,
                    text.len()
                );
            }
        }
    }

    #[test]
    fn test_layout_line_zwnj_edge_cases() {
        let fonts = AppleTextSystem::new();
        let font_id = fonts.font_id(&font("Helvetica")).unwrap();

        let text = "hello";
        let font_runs = &[FontRun { font_id, len: 5 }];
        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, text.len());

        let text = "abc";
        let font_runs = &[
            FontRun { font_id, len: 1 }, // "a"
            FontRun { font_id, len: 1 }, // "b"
            FontRun { font_id, len: 1 }, // "c"
        ];
        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, text.len());

        for run in &layout.runs {
            for glyph in &run.glyphs {
                assert!(
                    glyph.index < text.len(),
                    "Glyph index {} is out of bounds for text length {}",
                    glyph.index,
                    text.len()
                );
            }
        }

        // Test with empty text
        let text = "";
        let font_runs = &[];
        let layout = fonts.layout_line(text, px(16.), font_runs);
        assert_eq!(layout.len, 0);
        assert!(layout.runs.is_empty());
    }
}
