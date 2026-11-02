//! Immutable external frame ownership, separate from native import and submission.
use super::*;
use std::{
    any::Any,
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
};

/// Acquisition readiness, not GPU submission or presentation completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalImageReadiness {
    /// The adapter is still observing producer completion. Sampling is forbidden.
    Pending,
    /// The adapter has established immutable, synchronized access to the backing.
    Ready,
    /// Acquisition failed or its completion endpoint was abandoned.
    Failed(ContentError),
}
type Wake = Arc<dyn Fn() + Send + Sync>;
struct State {
    readiness: ExternalImageReadiness,
    version: u64,
    subscribers: HashMap<u64, Wake>,
}
struct Inner {
    id: u64,
    descriptor: ImageDescriptor,
    backing: Arc<dyn Any + Send + Sync>,
    state: Mutex<State>,
    release: Option<Box<dyn FnOnce() + Send + Sync>>,
}
impl Drop for Inner {
    fn drop(&mut self) {
        // The backing remains alive throughout the callback. An adapter needing
        // thread-affine cleanup must dispatch it, retaining its own backing owner.
        if let Some(release) = self.release.take() {
            release();
        }
    }
}
/// Portable identity and ownership for one immutable native frame. Clones share
/// acquisition and retirement. Construct a new handle for every producer frame,
/// even when a decoder reuses the same allocation address.
///
/// This handle does not import or sample resources. Native adapters validate the
/// backing, descriptor and synchronization protocol before GPU use. Executor and
/// graph integration is a separate implementation step.
#[derive(Clone)]
pub struct ExternalImage(Arc<Inner>);
impl std::fmt::Debug for ExternalImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExternalImage")
            .field("id", &self.id())
            .field("descriptor", self.descriptor())
            .finish_non_exhaustive()
    }
}
impl ExternalImage {
    /// Retain an adapter-owned backing and its producer lease while acquisition
    /// is pending. Only the returned endpoint can publish a terminal result.
    ///
    /// `release` runs exactly once after the last snapshot, acquisition endpoint
    /// and acquired-use lease has dropped, while the backing is still alive. It
    /// must not panic. It also runs after failed/abandoned acquisition, so adapters
    /// must distinguish their acquired/unacquired native obligations internally.
    pub fn pending(
        descriptor: ImageDescriptor,
        backing: Arc<dyn Any + Send + Sync>,
        release: impl FnOnce() + Send + Sync + 'static,
    ) -> (Self, ExternalImageCompletion) {
        let inner = Arc::new(Inner {
            id: next_content_identity(),
            descriptor,
            backing,
            state: Mutex::new(State {
                readiness: ExternalImageReadiness::Pending,
                version: 0,
                subscribers: HashMap::new(),
            }),
            release: Some(Box::new(release)),
        });
        (Self(inner.clone()), ExternalImageCompletion(Some(inner)))
    }
    /// Retain a backing whose adapter has already established immutable access.
    /// This is the synchronous equivalent of resolving pending acquisition.
    pub fn ready(
        descriptor: ImageDescriptor,
        backing: Arc<dyn Any + Send + Sync>,
        release: impl FnOnce() + Send + Sync + 'static,
    ) -> Self {
        let (image, completion) = Self::pending(descriptor, backing, release);
        completion.resolve(Ok(()));
        image
    }
    /// Immutable frame identity, independent of native allocation addresses.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    /// Natural logical image descriptor, independent of its destination geometry.
    /// Native plane layouts and conversion details remain adapter-owned backing data.
    pub fn descriptor(&self) -> &ImageDescriptor {
        &self.0.descriptor
    }
    /// Atomically observe acquisition and its terminal-transition version.
    pub fn observe(&self) -> (u64, ExternalImageReadiness) {
        let state = self.0.state.lock().unwrap();
        (state.version, state.readiness.clone())
    }
    /// Register before observing so acquisition cannot race past a subscription.
    /// Callbacks run outside locks and should schedule paced redraw through weak
    /// consumer ownership. Terminal observations do not need a callback.
    pub fn subscribe(
        &self,
        wake: Wake,
    ) -> (ExternalImageSubscription, u64, ExternalImageReadiness) {
        let mut state = self.0.state.lock().unwrap();
        let id = next_content_identity();
        if state.readiness == ExternalImageReadiness::Pending {
            state.subscribers.insert(id, wake);
        }
        (
            ExternalImageSubscription {
                inner: Arc::downgrade(&self.0),
                id,
            },
            state.version,
            state.readiness.clone(),
        )
    }
    /// Obtain ownership for a native import/use only after acquisition succeeds.
    /// Pending returns immediately; no host thread waits on a producer fence.
    pub fn acquire(&self) -> Result<ExternalImageLease, ContentError> {
        match &self.0.state.lock().unwrap().readiness {
            ExternalImageReadiness::Pending => Err(ContentError::new(
                ContentErrorKind::Pending,
                "external image acquisition is pending",
            )),
            ExternalImageReadiness::Failed(error) => Err(error.clone()),
            ExternalImageReadiness::Ready => Ok(ExternalImageLease(self.0.clone())),
        }
    }
}
/// An acquired immutable backing owner. Import caches and every submitted GPU
/// use must retain a clone until native completion, including failed submissions
/// with already-submitted prefixes. Dropping after submission alone is too early.
/// Retained snapshots also keep the producer lease; no implicit result cache is
/// created here. The executor decides cache admission and retirement explicitly.
#[derive(Clone)]
pub struct ExternalImageLease(Arc<Inner>);
impl ExternalImageLease {
    /// Frame identity for native cache validation.
    pub fn id(&self) -> u64 {
        self.0.id
    }
    /// Declared descriptor; the native importer must validate actual storage.
    pub fn descriptor(&self) -> &ImageDescriptor {
        &self.0.descriptor
    }
    /// Integration-boundary access to adapter data. Incompatible adapters must
    /// return UnsupportedCapability; this is not a backend switch for SDK users.
    pub fn backing<T: Any>(&self) -> Option<&T> {
        self.0.backing.downcast_ref()
    }
}
/// Removing a subscription cancels its wake but does not cancel shared acquisition.
/// A callback detached by completion can still run; consumers must reject stale epochs.
pub struct ExternalImageSubscription {
    inner: Weak<Inner>,
    id: u64,
}
impl Drop for ExternalImageSubscription {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            inner.state.lock().unwrap().subscribers.remove(&self.id);
        }
    }
}
/// Single terminal acquisition endpoint. It retains the backing independently
/// of consumer snapshots, so abandoning consumers cannot retire an active adapter
/// operation. Drop this endpoint only after that operation has stopped accessing
/// its backing; native asynchronous work must retain its own owner as needed.
pub struct ExternalImageCompletion(Option<Arc<Inner>>);
impl ExternalImageCompletion {
    /// Publish globally synchronized immutable access or a structured failure.
    /// Initial adapters use already-complete or asynchronously CPU-observed
    /// acquisition. Merely enqueueing a wait on one native queue is insufficient:
    /// queue-local GPU dependencies require future executor-specific integration.
    pub fn resolve(mut self, result: Result<(), ContentError>) {
        self.finish(result);
    }
    fn finish(&mut self, result: Result<(), ContentError>) {
        let Some(inner) = self.0.take() else { return };
        let wakes = {
            let mut state = inner.state.lock().unwrap();
            state.readiness = match result {
                Ok(()) => ExternalImageReadiness::Ready,
                Err(error) => ExternalImageReadiness::Failed(error),
            };
            state.version += 1;
            std::mem::take(&mut state.subscribers)
        };
        for wake in wakes.into_values() {
            wake();
        }
    }
}
impl Drop for ExternalImageCompletion {
    fn drop(&mut self) {
        self.finish(Err(ContentError::new(
            ContentErrorKind::Cancelled,
            "external image acquisition abandoned",
        )));
    }
}
