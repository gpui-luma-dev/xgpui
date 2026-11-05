//! Internal font-table readers shared by GPUI backends. Not part of gpui's API.
#![doc(hidden)]

use anyhow::{Context, Result, ensure};
use gpui::{FontTag, OpenTypeFeatureInfo, VariationAxisInfo};
use std::collections::BTreeMap;

/// Reads feature metadata from optional raw GSUB/GPOS tables of one resolved
/// face. Missing tables are valid; malformed feature metadata is an error.
/// Tags are sorted and duplicates are merged, retaining both table sources.
/// Does not validate shaping lookups or determine script/language coverage.
pub fn opentype_features(
    gsub: Option<&[u8]>,
    gpos: Option<&[u8]>,
) -> Result<Vec<OpenTypeFeatureInfo>> {
    let mut features = BTreeMap::new();
    for (data, substitution) in [(gsub, true), (gpos, false)] {
        let Some(data) = data else { continue };
        let read = |at: usize| -> Result<usize> {
            let bytes = data
                .get(at..at + 2)
                .context("truncated layout feature metadata")?;
            Ok(u16::from_be_bytes(bytes.try_into().unwrap()) as usize)
        };
        ensure!(
            read(0)? == 1 && read(2)? <= 1,
            "unsupported layout table version"
        );
        data.get(..if read(2)? == 1 { 14 } else { 10 })
            .context("truncated layout header")?;
        let offset = read(6)?;
        ensure!(
            offset >= if read(2)? == 1 { 14 } else { 10 },
            "invalid feature list offset"
        );
        let count = read(offset)?;
        let records_end = offset + 2 + count * 6;
        data.get(..records_end)
            .context("truncated feature records")?;
        for i in 0..count {
            let at = offset + 2 + i * 6;
            let tag: FontTag = std::str::from_utf8(&data[at..at + 4])?.parse()?;
            let feature = offset + read(at + 4)?;
            ensure!(feature >= records_end, "invalid feature table offset");
            let lookup_count = read(feature + 2)?;
            data.get(feature..feature + 4 + lookup_count * 2)
                .context("truncated feature table")?;
            let info = features.entry(tag).or_insert(OpenTypeFeatureInfo {
                tag,
                substitution: false,
                positioning: false,
            });
            if substitution {
                info.substitution = true;
            } else {
                info.positioning = true;
            }
        }
    }
    Ok(features.into_values().collect())
}

/// Reads axis metadata. A missing table means the face has no variable axes.
/// Localized display names and named instances are not included.
pub fn variation_axes(table: Option<&[u8]>) -> Result<Vec<VariationAxisInfo>> {
    let Some(data) = table else {
        return Ok(Vec::new());
    };
    let u16_at = |at: usize| -> Result<usize> {
        Ok(u16::from_be_bytes(
            data.get(at..at + 2)
                .context("truncated fvar header")?
                .try_into()?,
        ) as usize)
    };
    ensure!(
        u16_at(0)? == 1 && u16_at(2)? == 0,
        "unsupported fvar version"
    );
    let offset = u16_at(4)?;
    let count = u16_at(8)?;
    let size = u16_at(10)?;
    ensure!(
        data.len() >= 16 && offset >= 16 && size >= 20,
        "invalid fvar header"
    );
    data.get(..offset + count * size)
        .context("truncated fvar axes")?;
    let mut axes = BTreeMap::new();
    for index in 0..count {
        let at = offset + index * size;
        let tag: FontTag = std::str::from_utf8(&data[at..at + 4])?.parse()?;
        let coordinate = |relative| {
            i32::from_be_bytes(data[at + relative..at + relative + 4].try_into().unwrap()) as f32
                / 65536.0
        };
        let min_value = coordinate(4);
        let default_value = coordinate(8);
        let max_value = coordinate(12);
        ensure!(
            min_value <= default_value && default_value <= max_value,
            "invalid fvar axis range"
        );
        ensure!(!axes.contains_key(&tag), "duplicate fvar axis");
        axes.insert(
            tag,
            VariationAxisInfo {
                tag,
                min_value,
                default_value,
                max_value,
                hidden: u16_at(at + 16)? & 1 != 0,
            },
        );
    }
    Ok(axes.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_metadata_preserves_negative_design_units_and_rejects_truncation() {
        let mut table = vec![0, 1, 0, 0, 0, 16, 0, 2, 0, 1, 0, 20, 0, 0, 0, 0];
        table.extend(b"slnt");
        for value in [-10.25f32, 0.0, 0.0] {
            table.extend(((value * 65536.0) as i32).to_be_bytes());
        }
        table.extend([0, 1, 1, 0]);
        let axis = variation_axes(Some(&table)).unwrap().remove(0);
        assert_eq!(axis.min_value, -10.25);
        assert!(axis.hidden);
        for length in 0..table.len() {
            assert!(variation_axes(Some(&table[..length])).is_err());
        }
        assert!(variation_axes(None).unwrap().is_empty());
    }

    // Minimal layout table containing one feature, no lookups.
    fn table(tag: &[u8; 4]) -> Vec<u8> {
        let mut bytes = vec![0, 1, 0, 0, 0, 0, 0, 10, 0, 0, 0, 1];
        bytes.extend(tag);
        bytes.extend([0, 8, 0, 0, 0, 0]);
        bytes
    }
    #[test]
    fn distinguishes_empty_and_merges_sources() {
        assert!(opentype_features(None, None).unwrap().is_empty());
        let table = table(b"kern");
        let infos = opentype_features(Some(&table), Some(&table)).unwrap();
        assert_eq!(infos.len(), 1);
        assert!(infos[0].substitution && infos[0].positioning);
        for len in 0..table.len() {
            assert!(opentype_features(Some(&table[..len]), None).is_err());
        }
    }
    #[test]
    fn rejects_malformed_feature_metadata() {
        let valid = table(b"liga");
        for (at, value) in [(0, 2), (7, 0), (17, 0), (20, 255)] {
            let mut invalid = valid.clone();
            invalid[at] = value;
            assert!(opentype_features(Some(&valid), Some(&invalid)).is_err());
        }
        let empty = [0, 1, 0, 0, 0, 0, 0, 10, 0, 0, 0, 0];
        assert!(opentype_features(Some(&empty), None).unwrap().is_empty());
    }

    #[test]
    fn pinned_plex_advertises_features() {
        let face = ttf_parser::Face::parse(
            include_bytes!(
                "../../../../gpui/crates/gpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
            ),
            0,
        )
        .unwrap();
        let infos = opentype_features(
            face.raw_face().table(ttf_parser::Tag::from_bytes(b"GSUB")),
            face.raw_face().table(ttf_parser::Tag::from_bytes(b"GPOS")),
        )
        .unwrap();
        assert!(
            infos
                .iter()
                .any(|f| f.tag.to_string() == "liga" && f.substitution)
        );
        assert!(
            infos
                .iter()
                .any(|f| f.tag.to_string() == "kern" && f.positioning)
        );
        assert!(infos.windows(2).all(|pair| pair[0].tag < pair[1].tag));
    }
}
