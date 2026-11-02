//! CPU image readiness, independent of native allocation and submission.
use super::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};
/// Validated immutable packed mip payload. Color/alpha values remain a producer contract.
#[derive(Clone, Debug)]
pub struct ImagePixels {
    descriptor: ImageDescriptor,
    bytes: Arc<[u8]>,
}
impl ImagePixels {
    /// Validate storage size and an explicit source-memory admission limit.
    pub fn new(
        descriptor: ImageDescriptor,
        bytes: Arc<[u8]>,
        max_bytes: u64,
    ) -> Result<Self, ContentError> {
        if bytes.len() as u64 != descriptor.byte_len() {
            return Err(ContentError::new(
                ContentErrorKind::InvalidDescriptor,
                "image payload does not match packed mip size",
            ));
        }
        if bytes.len() as u64 > max_bytes {
            return Err(ContentError::new(
                ContentErrorKind::ResourceLimit,
                "image payload exceeds source-memory limit",
            ));
        }
        Ok(Self { descriptor, bytes })
    }
    /// Natural source descriptor, unaffected by destination size.
    pub fn descriptor(&self) -> &ImageDescriptor {
        &self.descriptor
    }
    /// Immutable tightly packed mips, concatenated in level order.
    pub fn bytes(&self) -> &Arc<[u8]> {
        &self.bytes
    }
}
/// Terminal readiness result or a pending CPU decode/request.
#[derive(Clone, Debug)]
pub enum ImageReadiness {
    /// CPU decoding or loading has not completed.
    Pending,
    /// Validated immutable pixels are ready for GPU upload.
    Available(ImagePixels),
    /// Terminal loading, decoding, or cancellation error.
    Failed(ContentError),
}
type Wake = Arc<dyn Fn() + Send + Sync>;
struct State {
    readiness: ImageReadiness,
    version: u64,
    subscribers: HashMap<u64, Wake>,
}
struct Inner {
    id: u64,
    state: Mutex<State>,
}
/// Stable request identity. Clones share terminal pixels and readiness.
#[derive(Clone)]
pub struct ImageAsset(Arc<Inner>);
impl std::fmt::Debug for ImageAsset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ImageAsset").field(&self.id()).finish()
    }
}
impl ImageAsset {
    /// Create a pending asset and its sole terminal completion endpoint.
    pub fn pending() -> (Self, ImageCompletion) {
        let inner = Arc::new(Inner {
            id: next_content_identity(),
            state: Mutex::new(State {
                readiness: ImageReadiness::Pending,
                version: 0,
                subscribers: HashMap::new(),
            }),
        });
        (Self(inner.clone()), ImageCompletion(Some(inner)))
    }
    /// Create an already available immutable source.
    pub fn ready(pixels: ImagePixels) -> Self {
        Self(Arc::new(Inner {
            id: next_content_identity(),
            state: Mutex::new(State {
                readiness: ImageReadiness::Available(pixels),
                version: 1,
                subscribers: HashMap::new(),
            }),
        }))
    }
    /// Stable request identity; replacement requests use a new asset.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    /// Atomically observe readiness and its terminal-transition version.
    pub fn observe(&self) -> (u64, ImageReadiness) {
        let state = self.0.state.lock().unwrap();
        (state.version, state.readiness.clone())
    }
    /// Atomically subscribe before observing. Pending completion cannot be missed.
    /// A terminal observation needs no wake. Callbacks run outside the state lock
    /// and should only schedule paced redraws through weak consumer ownership.
    pub fn subscribe(&self, wake: Wake) -> (ImageSubscription, u64, ImageReadiness) {
        let mut state = self.0.state.lock().unwrap();
        let id = next_content_identity();
        if matches!(state.readiness, ImageReadiness::Pending) {
            state.subscribers.insert(id, wake);
        }
        (
            ImageSubscription {
                inner: Arc::downgrade(&self.0),
                id,
            },
            state.version,
            state.readiness.clone(),
        )
    }
}
/// Consumer-owned cancellation of a readiness subscription. A callback already
/// detached by completion may still run; consumers must reject stale epochs.
pub struct ImageSubscription {
    inner: Weak<Inner>,
    id: u64,
}
impl Drop for ImageSubscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.state.lock().unwrap().subscribers.remove(&self.id);
        }
    }
}
/// Single-use completion ownership. Abandonment resolves a pending request as
/// Cancelled; dropping an asset clone does not cancel other consumers.
pub struct ImageCompletion(Option<Arc<Inner>>);
impl ImageCompletion {
    /// Publish validated pixels or a terminal structured failure.
    pub fn resolve(mut self, result: Result<ImagePixels, ContentError>) {
        self.finish(result);
    }
    fn finish(&mut self, result: Result<ImagePixels, ContentError>) {
        let Some(inner) = self.0.take() else {
            return;
        };
        let wakes = {
            let mut state = inner.state.lock().unwrap();
            state.readiness = match result {
                Ok(p) => ImageReadiness::Available(p),
                Err(e) => ImageReadiness::Failed(e),
            };
            state.version += 1;
            std::mem::take(&mut state.subscribers)
        };
        for wake in wakes.into_values() {
            wake();
        }
    }
}
impl Drop for ImageCompletion {
    fn drop(&mut self) {
        self.finish(Err(ContentError::new(
            ContentErrorKind::Cancelled,
            "image request completion abandoned",
        )));
    }
}
