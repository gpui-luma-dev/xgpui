//! Conservative reuse of framebuffer captures, shared by all SDR backends.
use crate::*;
use std::sync::Arc;

/// Exact scene dependencies for a retained backdrop result.
#[derive(Clone, Debug, PartialEq)]
pub struct BackdropCacheKey {
    /// Viewport changes invalidate even when the crop is unchanged.
    pub viewport: Size<DevicePixels>,
    /// Full-resolution crop origin and extent.
    pub origin: [u32; 2],
    /// Full-resolution crop extent.
    pub size: [u32; 2],
    /// Filter parameters; composition-only properties are deliberately excluded.
    pub filter: [[[f32; 4]; 6]; 2],
    /// Exact immutable identities and draw orders of all preceding paint commands.
    pub revision: Arc<[(u64, DrawOrder)]>,
    /// External content and oversized prefixes must refresh every frame.
    pub reusable: bool,
}
/// Cumulative statistics exposed by headless renderers.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct BackdropCacheStats {
    /// Source captures and two-pass filters executed.
    pub captures: u64,
    /// Source capture and filter passes skipped.
    pub hits: u64,
}
/// Bounded cache bookkeeping; GPU textures remain owned by each backend.
#[derive(Default)]
pub struct BackdropCache {
    current: Vec<BackdropCacheKey>,
    retained: Vec<Option<BackdropCacheKey>>,
    hits: Vec<bool>,
    context: Vec<u64>,
    /// Cumulative successfully encoded frame statistics.
    pub stats: BackdropCacheStats,
}
impl BackdropCache {
    /// Prepare exact keys and a backend fingerprint of clear color/render settings.
    pub fn prepare(&mut self, keys: Vec<BackdropCacheKey>, context: &[u64]) {
        if self.context != context {
            self.retained.clear();
            self.context = context.to_vec();
        }
        self.retained.resize_with(keys.len(), || None);
        self.hits = keys
            .iter()
            .zip(&self.retained)
            .map(|(key, old)| key.reusable && old.as_ref() == Some(key))
            .collect();
        self.current = keys;
    }
    /// Invalidate a newly allocated/replaced texture slot.
    pub fn invalidate(&mut self, index: usize) {
        if let Some(old) = self.retained.get_mut(index) {
            *old = None;
        }
        if let Some(hit) = self.hits.get_mut(index) {
            *hit = false;
        }
    }
    /// Whether the source capture and filter passes can be skipped.
    pub fn hit(&self, index: usize) -> bool {
        self.hits[index]
    }
    /// Clear the old key before overwriting its pixels, including on failed frames.
    pub fn start(&mut self, index: usize) {
        if !self.hits[index] {
            self.retained[index] = None;
        }
    }
    /// Commit only boundaries encoded by a successfully encoded frame.
    pub fn commit(&mut self, executed: &[usize]) {
        for &index in executed {
            if self.hits[index] {
                self.stats.hits += 1;
            } else {
                self.stats.captures += 1;
                self.retained[index] = self.current[index]
                    .reusable
                    .then(|| self.current[index].clone());
            }
        }
    }
}
