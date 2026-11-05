use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    str::FromStr,
    sync::Arc,
};

/// A validated four-byte OpenType tag (printable ASCII, including spaces).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FontTag([u8; 4]);

impl FontTag {
    /// Returns the tag's bytes in OpenType order.
    pub fn as_bytes(&self) -> &[u8; 4] {
        &self.0
    }
}

impl FromStr for FontTag {
    type Err = anyhow::Error;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        anyhow::ensure!(
            value.len() == 4 && value.bytes().all(|b| (0x20..=0x7e).contains(&b)),
            "OpenType tags must contain four printable ASCII bytes"
        );
        Ok(Self(value.as_bytes().try_into().unwrap()))
    }
}

impl std::fmt::Display for FontTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(std::str::from_utf8(&self.0).unwrap())
    }
}

/// Precise author coordinates, independent of any interactive preview policy.
///
/// Settings are sorted by tag; the last duplicate wins. Values are finite signed
/// f32 coordinates with negative zero normalized. No quantization is performed.
/// Explicit settings override implicit weight/style coordinates after face selection.
/// Unsupported axes are ignored per face; supported values are clamped per face.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[serde(into = "BTreeMap<String, f32>")]
#[schemars(with = "BTreeMap<String, f32>")]
pub struct FontVariations(Arc<Vec<(FontTag, f32)>>);

impl Eq for FontVariations {}
impl Hash for FontVariations {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.len().hash(state);
        for (tag, value) in self.0.iter() {
            tag.hash(state);
            value.to_bits().hash(state);
        }
    }
}

impl FontVariations {
    /// Validates and canonicalizes settings. Invalid values, even in overwritten
    /// entries, are rejected. Empty settings preserve backend defaults.
    pub fn new(settings: impl IntoIterator<Item = (FontTag, f32)>) -> anyhow::Result<Self> {
        let mut axes = BTreeMap::new();
        for (tag, value) in settings {
            anyhow::ensure!(value.is_finite(), "axis {tag} must be finite");
            axes.insert(tag, if value == 0.0 { 0.0 } else { value });
        }
        Ok(Self(Arc::new(axes.into_iter().collect())))
    }

    /// Returns canonical settings in tag order, in the font's design units.
    pub fn settings(&self) -> &[(FontTag, f32)] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    fn hash(value: &FontVariations) -> u64 {
        let mut h = DefaultHasher::new();
        value.hash(&mut h);
        h.finish()
    }
    #[test]
    fn canonical_coordinates_preserve_signed_precision() {
        let slnt = "slnt".parse().unwrap();
        let wght = "wght".parse().unwrap();
        let a = FontVariations::new([(wght, 300.25), (slnt, -12.5), (wght, 400.125)]).unwrap();
        let b = FontVariations::new([(slnt, -12.5), (wght, 400.125)]).unwrap();
        assert_eq!(a, b);
        assert_eq!(hash(&a), hash(&b));
        assert_eq!(a.settings()[0], (slnt, -12.5));
        assert_ne!(
            a,
            FontVariations::new([(slnt, -12.5), (wght, 400.126)]).unwrap()
        );
        let zero = FontVariations::new([(slnt, 0.0)]).unwrap();
        let negative_zero = FontVariations::new([(slnt, -0.0)]).unwrap();
        assert_eq!(zero, negative_zero);
        assert_eq!(hash(&zero), hash(&negative_zero));
    }
    #[test]
    fn serialization_preserves_signed_settings_and_rejects_invalid_duplicates() {
        let values = FontVariations::new([("slnt".parse().unwrap(), -10.125)]).unwrap();
        assert_eq!(
            serde_json::to_string(&values).unwrap(),
            r#"{"slnt":-10.125}"#
        );
        assert_eq!(
            serde_json::from_str::<FontVariations>(r#"{"wght":400,"slnt":-10.125,"wght":500}"#)
                .unwrap()
                .settings(),
            &[
                ("slnt".parse().unwrap(), -10.125),
                ("wght".parse().unwrap(), 500.0)
            ]
        );
        assert!(serde_json::from_str::<FontVariations>(r#"{"wght":1e100,"wght":400}"#).is_err());
        assert!(serde_json::from_str::<FontVariations>(r#"{"bad":400}"#).is_err());
    }

    #[test]
    fn rejects_invalid_coordinates_and_tags() {
        for tag in ["", "abc", "abcde", "éab", "a\nbc"] {
            assert!(tag.parse::<FontTag>().is_err());
        }
        assert!("CFF ".parse::<FontTag>().is_ok());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(FontVariations::new([("slnt".parse().unwrap(), value)]).is_err());
        }
        assert!(FontVariations::default().settings().is_empty());
    }
}

impl TryFrom<BTreeMap<String, f32>> for FontVariations {
    type Error = anyhow::Error;
    fn try_from(settings: BTreeMap<String, f32>) -> Result<Self, Self::Error> {
        Self::new(
            settings
                .into_iter()
                .map(|(tag, value)| Ok((tag.parse()?, value)))
                .collect::<anyhow::Result<Vec<_>>>()?,
        )
    }
}

impl From<FontVariations> for BTreeMap<String, f32> {
    fn from(settings: FontVariations) -> Self {
        settings
            .settings()
            .iter()
            .map(|(tag, value)| (tag.to_string(), *value))
            .collect()
    }
}

impl<'de> serde::Deserialize<'de> for FontVariations {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = FontVariations;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a map of four-byte axis tags to finite coordinates")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut settings = Vec::new();
                while let Some((tag, value)) = map.next_entry::<String, f32>()? {
                    let tag = tag.parse().map_err(serde::de::Error::custom)?;
                    if !value.is_finite() {
                        return Err(serde::de::Error::custom("axis coordinate must be finite"));
                    }
                    settings.push((tag, value));
                }
                FontVariations::new(settings).map_err(serde::de::Error::custom)
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}
