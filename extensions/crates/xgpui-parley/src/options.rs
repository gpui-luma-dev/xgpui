//! Paragraph-wide shaping options owned by this extension; GPUI core is unchanged.
use anyhow::{Context, Result};

/// Paragraph-wide options that affect shaping and line breaking.
///
/// Construct with `..Default::default()`; the defaults reproduce
/// [`crate::ParleyTextSystem::prepare_paragraph`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParagraphOptions {
    /// Base direction; [`TextDirection::Auto`] infers it from the text.
    pub direction: TextDirection,
    /// Content language for shaping and line breaking; `None` leaves it unspecified.
    pub language: Option<TextLanguage>,
    /// Absolute line height; `None` uses the font's own line metrics.
    pub line_height: Option<gpui::Pixels>,
}

/// Base direction of a paragraph: the order of its lines' runs and its start edge.
///
/// An explicit direction sets the Unicode Bidirectional Algorithm's paragraph
/// level; mixed-direction runs inside the paragraph are still reordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextDirection {
    /// Use the first strong directional character, or left-to-right if there is none.
    #[default]
    Auto,
    /// Left-to-right base direction.
    LeftToRight,
    /// Right-to-left base direction.
    RightToLeft,
}

impl From<TextDirection> for parley::BaseDirection {
    fn from(direction: TextDirection) -> Self {
        match direction {
            TextDirection::Auto => Self::Auto,
            TextDirection::LeftToRight => Self::Ltr,
            TextDirection::RightToLeft => Self::Rtl,
        }
    }
}

/// A content language parsed from a BCP 47 tag.
///
/// Parley keeps the normalized `language[-Script][-REGION]` prefix
/// (`EN-latn-us` becomes `en-Latn-US`); later subtags are validated and discarded.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TextLanguage(parley::Language);

impl TextLanguage {
    /// Parse a BCP 47 tag; `-` and `_` are accepted as separators.
    pub fn parse(tag: &str) -> Result<Self> {
        parley::Language::parse(tag)
            .map(Self)
            .with_context(|| format!("invalid language tag {tag:?}"))
    }

    /// The normalized `language[-Script][-REGION]` tag.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub(crate) fn parley(self) -> parley::Language {
        self.0
    }
}

impl std::str::FromStr for TextLanguage {
    type Err = anyhow::Error;

    fn from_str(tag: &str) -> Result<Self> {
        Self::parse(tag)
    }
}

impl std::fmt::Display for TextLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Debug for TextLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLanguage({:?})", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_tags_normalize_through_parley() {
        for (input, expected) in [
            ("ar", "ar"),
            ("ur-pk", "ur-PK"),
            ("EN-latn-us", "en-Latn-US"),
            ("sr_cyrl", "sr-Cyrl"),
            ("en-US-u-ca-gregory", "en-US"),
        ] {
            let language = TextLanguage::parse(input).unwrap();
            assert_eq!(language.as_str(), expected, "{input}");
            assert_eq!(language.to_string(), expected);
        }
        assert_eq!(
            TextLanguage::parse("EN-us").unwrap(),
            "en_US".parse().unwrap()
        );
        // Validation follows Parley's parser, which rejects these.
        for input in ["", "english", "zh-yue"] {
            assert!(TextLanguage::parse(input).is_err(), "{input:?}");
        }
    }

    #[test]
    fn defaults_are_automatic_and_unspecified() {
        let options = ParagraphOptions::default();
        assert_eq!(options.direction, TextDirection::Auto);
        assert_eq!(options.language, None);
        assert_eq!(options.line_height, None);
    }
}
