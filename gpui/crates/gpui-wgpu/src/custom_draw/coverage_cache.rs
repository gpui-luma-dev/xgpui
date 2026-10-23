//! Private exact coverage cache. Fingerprints select buckets, never geometry equality.
use anyhow::{Result, ensure};
use gpui::{CustomCoverage, CustomDraw, PathVertex, ScaledPixels};
use std::{collections::HashMap, hash::Hasher};

const MAX_PATH_VERTICES: usize = 1_000_000;

/// Borrowed lookup: no owned vertex vector and no allocation on cache hits.
#[derive(Clone, Copy)]
pub(crate) struct CoverageKey<'a> {
    draw: &'a CustomDraw,
    fingerprint: u64,
    word_count: usize,
}
impl<'a> CoverageKey<'a> {
    pub(crate) fn new(draw: &'a CustomDraw) -> Result<Self> {
        Self::with_hasher(draw, collections::FxHasher::default())
    }
    fn with_hasher(draw: &'a CustomDraw, mut hasher: impl Hasher) -> Result<Self> {
        // Check before visiting or hashing even a single vertex.
        let word_count = match draw.coverage.as_ref().expect("coverage") {
            CustomCoverage::Path(path) => {
                ensure!(
                    path.vertices.len() <= MAX_PATH_VERTICES,
                    "coverage path too large"
                );
                8 + path.vertices.len() * 4
            }
            CustomCoverage::Border { .. } => 12,
        };
        for word in words(draw) {
            hasher.write_u32(word);
        }
        Ok(Self {
            draw,
            fingerprint: hasher.finish(),
            word_count,
        })
    }
    /// Retained normalized geometry bytes; included in the mask cache budget.
    pub(crate) fn byte_len(&self) -> usize {
        self.word_count * std::mem::size_of::<u32>()
    }
}

fn words(draw: &CustomDraw) -> impl Iterator<Item = u32> + '_ {
    let mut header = [0u32; 12];
    header[..6].copy_from_slice(&[
        draw.bounds.size.width.0.to_bits(),
        draw.bounds.size.height.0.to_bits(),
        draw.corner_radii.top_left.0.to_bits(),
        draw.corner_radii.top_right.0.to_bits(),
        draw.corner_radii.bottom_right.0.to_bits(),
        draw.corner_radii.bottom_left.0.to_bits(),
    ]);
    let vertices: &[PathVertex<ScaledPixels>];
    let header_len;
    match draw.coverage.as_ref().expect("coverage") {
        CustomCoverage::Border { widths, style } => {
            header[6..].copy_from_slice(&[
                0,
                *style as u32,
                widths.top.0.to_bits(),
                widths.right.0.to_bits(),
                widths.bottom.0.to_bits(),
                widths.left.0.to_bits(),
            ]);
            header_len = 12;
            vertices = &[];
        }
        CustomCoverage::Path(path) => {
            header[6] = 1;
            header[7] = path.vertices.len() as u32;
            header_len = 8;
            vertices = &path.vertices;
        }
    }
    let origin = draw.bounds.origin;
    header
        .into_iter()
        .take(header_len)
        .chain(vertices.iter().flat_map(move |v| {
            [
                (v.xy_position.x.0 - origin.x.0).to_bits(),
                (v.xy_position.y.0 - origin.y.0).to_bits(),
                v.st_position.x.to_bits(),
                v.st_position.y.to_bits(),
            ]
        }))
}
struct Entry<T> {
    words: Vec<u32>,
    value: T,
}
pub(crate) struct CoverageCache<T> {
    buckets: HashMap<u64, Vec<Entry<T>>>,
}
impl<T> Default for CoverageCache<T> {
    fn default() -> Self {
        Self {
            buckets: HashMap::new(),
        }
    }
}
impl<T> CoverageCache<T> {
    pub(crate) fn get(&self, key: &CoverageKey<'_>) -> Option<&T> {
        self.buckets
            .get(&key.fingerprint)?
            .iter()
            .find(|entry| {
                entry.words.len() == key.word_count
                    && words(key.draw).eq(entry.words.iter().copied())
            })
            .map(|entry| &entry.value)
    }
    /// Allocate exact geometry only after a cache miss and successful rasterization.
    pub(crate) fn insert(&mut self, key: &CoverageKey<'_>, value: T) {
        let bucket = self.buckets.entry(key.fingerprint).or_default();
        if let Some(entry) = bucket.iter_mut().find(|entry| {
            entry.words.len() == key.word_count && words(key.draw).eq(entry.words.iter().copied())
        }) {
            entry.value = value;
            return;
        }
        let mut geometry = Vec::with_capacity(key.word_count);
        geometry.extend(words(key.draw));
        bucket.push(Entry {
            words: geometry,
            value,
        });
    }
    pub(crate) fn clear(&mut self) {
        self.buckets.clear();
    }
}

#[cfg(test)]
mod tests;
