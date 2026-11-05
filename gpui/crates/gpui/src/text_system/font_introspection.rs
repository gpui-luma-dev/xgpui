use super::FontTag;

/// A query can be unsupported, fail (`Err`), or succeed with an empty value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FontQuery<T> {
    /// The backend cannot perform this query; this says nothing about the font.
    Unsupported,
    /// The query succeeded, including when no features were found.
    Supported(T),
}

/// A feature advertised by the resolved face's GSUB and/or GPOS FeatureList.
/// Presence does not imply applicability to a script, language, or input text.
/// This is metadata, not a list of boolean toggles or allowed numeric values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenTypeFeatureInfo {
    /// The OpenType feature tag, usable with existing numeric `FontFeatures`.
    pub tag: FontTag,
    /// Advertised by the glyph substitution table.
    pub substitution: bool,
    /// Advertised by the glyph positioning table.
    pub positioning: bool,
}

/// Backend support for font inspection and explicit static axes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FontCapabilities {
    /// Can read GSUB/GPOS feature metadata.
    pub feature_introspection: bool,
    /// Can read variable-axis ranges on resolved faces.
    pub axis_introspection: bool,
    /// Applies arbitrary explicit coordinates consistently to layout and rasterization.
    pub static_variations: bool,
}

/// An axis advertised by a resolved face's `fvar` table, in design units.
#[derive(Clone, Debug, PartialEq)]
pub struct VariationAxisInfo {
    /// Four-byte OpenType tag.
    pub tag: FontTag,
    /// Minimum supported coordinate.
    pub min_value: f32,
    /// Default coordinate when no explicit or implicit setting applies.
    pub default_value: f32,
    /// Maximum supported coordinate.
    pub max_value: f32,
    /// Whether the font recommends hiding this axis from ordinary inspectors.
    pub hidden: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_backend_is_not_an_empty_success() {
        let text = crate::TextSystem::new(std::sync::Arc::new(crate::NoopTextSystem::new()));
        assert_eq!(
            text.opentype_features(crate::FontId(0)).unwrap(),
            FontQuery::Unsupported
        );
    }
}
