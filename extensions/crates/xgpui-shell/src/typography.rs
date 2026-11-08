use gpui::{Font, FontStyle, FontWeight, font};

/// Client-frame typography in logical pixels, before the shell text scale.
/// Application content retains its own typography. Font availability and glyph
/// fallback are handled by GPUI's text system; a requested family is not proof
/// that the family is installed.
#[derive(Clone, Debug, PartialEq)]
pub struct ShellTypography {
    pub font: Font,
    /// Logical pixel size; normalized to 8–48 before text scaling.
    pub size: f32,
}

impl Default for ShellTypography {
    fn default() -> Self {
        Self {
            font: font(".SystemUIFont"),
            size: 14.0,
        }
    }
}

impl ShellTypography {
    pub(crate) fn normalized(mut self) -> Self {
        if self.font.family.trim().is_empty() {
            self.font.family = ".SystemUIFont".into();
        }
        self.size = if self.size.is_finite() {
            self.size.clamp(8.0, 48.0)
        } else {
            14.0
        };
        self.font.weight = if self.font.weight.0.is_finite() {
            FontWeight(self.font.weight.0.clamp(100.0, 900.0))
        } else {
            FontWeight::NORMAL
        };
        self
    }

    /// Parse the common desktop family/size/weight/style subset of GNOME Pango
    /// descriptions or KDE QFont serialization. Points use 96/72 logical pixels;
    /// explicit `px` is already logical pixels. This never applies device DPI.
    /// Advanced Pango variations/features/stretch are not supported. QFont's
    /// additional attributes (underline, stretch, features, etc.) are not applied.
    /// Invalid or unsupported descriptions return None, allowing a safe fallback.
    pub fn from_desktop_description(description: &str) -> Option<Self> {
        if description.chars().any(char::is_control) {
            return None;
        }
        let description = description.trim();
        if description.is_empty() {
            return None;
        }
        let fields: Vec<_> = description.split(',').map(str::trim).collect();
        let parsed = if fields.len() >= 10 && fields[1].parse::<f32>().is_ok() {
            qfont(&fields)
        } else {
            pango(description)
        }?;
        Some(parsed.normalized())
    }
}

fn positive(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite() && *v > 0.0)
}

fn qfont(fields: &[&str]) -> Option<ShellTypography> {
    let family = fields.first()?.trim();
    if family.is_empty() {
        return None;
    }
    let size = positive(fields.get(1)?)
        .map(|pt| pt * 96.0 / 72.0)
        .or_else(|| positive(fields.get(2)?))?;
    let weight = fields.get(4)?.parse::<u16>().ok()?;
    let weight = if weight < 100 {
        // Qt 5 legacy weights are nonlinear; common serialized Plasma fonts
        // use 50 (normal), 63 (demibold), or 75 (bold).
        match weight {
            0..=11 => 100.0,
            12..=24 => 200.0,
            25..=49 => 300.0,
            50..=56 => 400.0,
            57..=62 => 500.0,
            63..=74 => 600.0,
            75..=80 => 700.0,
            81..=86 => 800.0,
            _ => 900.0,
        }
    } else if weight <= 900 {
        weight as f32
    } else {
        return None;
    };
    let style = match *fields.get(5)? {
        "0" => FontStyle::Normal,
        "1" => FontStyle::Italic,
        "2" => FontStyle::Oblique,
        _ => return None,
    };
    let mut font = font(family.to_owned());
    font.weight = FontWeight(weight);
    font.style = style;
    Some(ShellTypography { font, size })
}

fn pango(description: &str) -> Option<ShellTypography> {
    if description.contains(['@', '#', '=']) {
        return None;
    }
    let mut words: Vec<_> = description.split_whitespace().collect();
    let last = words.last().copied()?;
    let (size, has_size) = if let Some(value) = last.strip_suffix("px") {
        (positive(value)?, true)
    } else if let Some(points) = positive(last) {
        (points * 96.0 / 72.0, true)
    } else {
        // Numeric-looking invalid sizes must not become part of a family.
        if last.parse::<f32>().is_ok() {
            return None;
        }
        (14.0, false)
    };
    if has_size {
        words.pop();
    }
    let mut weight = FontWeight::NORMAL;
    let mut style = FontStyle::Normal;
    while let Some(word) = words.last() {
        match word.to_ascii_lowercase().as_str() {
            "normal" | "regular" => (),
            "thin" => weight = FontWeight::THIN,
            "ultralight" | "extra-light" => weight = FontWeight::EXTRA_LIGHT,
            "light" => weight = FontWeight::LIGHT,
            "medium" => weight = FontWeight::MEDIUM,
            "semibold" | "demibold" | "semi-bold" => weight = FontWeight::SEMIBOLD,
            "bold" => weight = FontWeight::BOLD,
            "ultrabold" | "extra-bold" => weight = FontWeight::EXTRA_BOLD,
            "heavy" | "black" => weight = FontWeight::BLACK,
            "italic" => style = FontStyle::Italic,
            "oblique" => style = FontStyle::Oblique,
            "condensed" | "expanded" | "small-caps" | "ultracondensed" | "semicondensed"
            | "ultraexpanded" | "semiexpanded" => return None,
            _ => break,
        }
        words.pop();
    }
    let families = words.join(" ");
    let family = families.trim_end_matches(',').trim();
    // GPUI supports a family plus explicit fallback list, not a Pango family list.
    // Avoid silently treating multiple families as one installed family.
    if family.is_empty() || family.contains(',') {
        return None;
    }
    let mut font = font(family.to_owned());
    font.weight = weight;
    font.style = style;
    Some(ShellTypography { font, size })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gnome_fonts_preserve_families_units_and_common_styles() {
        let f = ShellTypography::from_desktop_description("Adwaita Sans 11").unwrap();
        assert_eq!(f.font.family.as_ref(), "Adwaita Sans");
        assert!((f.size - 14.666667).abs() < 0.001);
        let f = ShellTypography::from_desktop_description("Noto Sans Bold Italic 16px").unwrap();
        assert_eq!(f.font.family.as_ref(), "Noto Sans");
        assert_eq!(f.size, 16.0);
        assert_eq!(f.font.weight, FontWeight::BOLD);
        assert_eq!(f.font.style, FontStyle::Italic);
        let f = ShellTypography::from_desktop_description("Bookman Light, 12").unwrap();
        assert_eq!(f.font.family.as_ref(), "Bookman Light");
    }
    #[test]
    fn kde_fonts_support_legacy_and_css_weights_and_pixel_sizes() {
        let f =
            ShellTypography::from_desktop_description("Noto Sans,10,-1,5,50,0,0,0,0,0").unwrap();
        assert_eq!(f.font.weight, FontWeight::NORMAL);
        assert!((f.size - 13.333333).abs() < 0.001);
        let f =
            ShellTypography::from_desktop_description("Noto Sans,-1,18,5,700,1,0,0,0,0").unwrap();
        assert_eq!(f.size, 18.0);
        assert_eq!(f.font.weight, FontWeight::BOLD);
        assert_eq!(f.font.style, FontStyle::Italic);
    }
    #[test]
    fn malformed_and_unsupported_descriptions_are_not_used_as_families() {
        for value in [
            "",
            "Sans NaN",
            "Sans -1",
            "Sans 0",
            "Sans Condensed 11",
            "Sans 11 @wght=500",
            "Sans,Serif 11",
            "\nSans 11",
            "Noto,NaN,-1,5,50,0,0,0,0,0",
        ] {
            assert!(
                ShellTypography::from_desktop_description(value).is_none(),
                "{value}"
            );
        }
        let normalized = ShellTypography {
            font: font(""),
            size: f32::NAN,
        }
        .normalized();
        assert_eq!(normalized.size, 14.0);
        assert_eq!(normalized.font.family.as_ref(), ".SystemUIFont");
    }
}
