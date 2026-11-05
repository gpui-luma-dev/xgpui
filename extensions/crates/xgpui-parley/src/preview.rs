//! Explicit preview admission and quantization; author values remain separate.
use anyhow::{Result, ensure};
use gpui::{Font, FontTag, FontVariations, VariationAxisInfo};
use std::collections::{BTreeMap, HashSet};

/// A preview policy for one fixed font descriptor and one bounded set of
/// requested instances. Quantization happens before font resolution, so shaping
/// and rendering see the same coordinates. No font IDs or atlas tiles are evicted.
///
/// This caps admitted descriptors in this controller only. It is not a global
/// CPU/GPU memory budget: fallback, glyphs, sizes, caches, and multiple controllers
/// need separate accounting. Keep a controller for the inspector's lifetime.
pub struct VariationPreview {
    base: Font,
    axes: BTreeMap<FontTag, (VariationAxisInfo, f32)>,
    admitted: HashSet<FontVariations>,
    capacity: usize,
}

impl VariationPreview {
    /// Creates a policy using face-specific ranges and positive design-unit
    /// intervals. Intervals are caller-selected; no universal step is implied.
    /// Every explicit axis must have a policy, including any fallback-only axis.
    pub fn new(
        base: Font,
        policies: impl IntoIterator<Item = (VariationAxisInfo, f32)>,
        capacity: usize,
    ) -> Result<Self> {
        ensure!(capacity > 0, "preview capacity must be positive");
        let mut axes = BTreeMap::new();
        for (axis, interval) in policies {
            ensure!(
                interval.is_finite() && interval > 0.0,
                "preview interval must be finite and positive"
            );
            ensure!(
                axis.min_value.is_finite()
                    && axis.default_value.is_finite()
                    && axis.max_value.is_finite()
                    && axis.min_value <= axis.default_value
                    && axis.default_value <= axis.max_value,
                "invalid preview axis range"
            );
            ensure!(
                !axes.contains_key(&axis.tag),
                "duplicate preview axis policy"
            );
            axes.insert(axis.tag, (axis, interval));
        }
        Ok(Self {
            base,
            axes,
            admitted: HashSet::new(),
            capacity,
        })
    }

    /// Returns a complete preview descriptor. After capacity is reached, already
    /// admitted settings remain usable; new settings return an error without
    /// resolving a font or evicting anything. The caller can keep its last preview.
    /// Commit the precise author coordinates separately when editing finishes.
    pub fn font(&mut self, author: &FontVariations) -> Result<Font> {
        let mut settings = Vec::with_capacity(author.settings().len());
        for &(tag, value) in author.settings() {
            let (axis, interval) = self
                .axes
                .get(&tag)
                .ok_or_else(|| anyhow::anyhow!("missing preview policy for {tag}"))?;
            // f64 arithmetic avoids overflow for otherwise valid finite f32 inputs.
            let min = axis.min_value as f64;
            let max = axis.max_value as f64;
            let value = (value as f64).clamp(min, max);
            let snapped = (min + ((value - min) / *interval as f64).round() * *interval as f64)
                .clamp(min, max) as f32;
            settings.push((tag, snapped));
        }
        let settings = FontVariations::new(settings)?;
        ensure!(
            self.admitted.contains(&settings) || self.admitted.len() < self.capacity,
            "preview instance budget exhausted"
        );
        self.admitted.insert(settings.clone());
        let mut font = self.base.clone();
        font.variations = settings;
        Ok(font)
    }

    /// Number of distinct preview descriptors admitted so far.
    pub fn admitted_instances(&self) -> usize {
        self.admitted.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn admission_limits_combinations_without_mutating_precise_author_values() -> Result<()> {
        let tag = "slnt".parse()?;
        let axis = VariationAxisInfo {
            tag,
            min_value: -10.0,
            default_value: 0.0,
            max_value: 0.0,
            hidden: false,
        };
        let mut preview = VariationPreview::new(gpui::font("Roboto Flex"), [(axis, 2.0)], 2)?;
        let author = FontVariations::new([(tag, -9.125)])?;
        let first = preview.font(&author)?;
        assert_eq!(first.variations.settings()[0].1, -10.0);
        assert_eq!(author.settings()[0].1, -9.125);
        preview.font(&FontVariations::new([(tag, -7.5)])?)?;
        assert!(preview.font(&FontVariations::new([(tag, -2.0)])?).is_err());
        for _ in 0..100 {
            assert_eq!(preview.font(&author)?, first);
        }
        assert_eq!(preview.admitted_instances(), 2);
        Ok(())
    }
}
