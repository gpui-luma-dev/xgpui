use anyhow::{Result, ensure};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
fn next_owner() -> u64 {
    let mut current = NEXT_OWNER.load(Ordering::Relaxed);
    loop {
        let next = current
            .checked_add(1)
            .expect("source request identity exhausted");
        match NEXT_OWNER.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return current,
            Err(actual) => current = actual,
        }
    }
}

/// Caller-owned asset identity. Increment generation whenever pixels, dimensions
/// or metadata change; do not reuse a pair for different source contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceId {
    pub asset: u64,
    pub generation: u64,
}
/// A request token obtained from its owning ProgressiveImage. A restart (including
/// a viewport change) changes revision even when the source is unchanged. Tokens
/// belong to one state instance; never route updates between unrelated instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceRequest {
    source: SourceId,
    owner: u64,
    revision: u64,
}
impl SourceRequest {
    pub fn source(self) -> SourceId {
        self.source
    }
    pub fn revision(self) -> u64 {
        self.revision
    }
}
/// Provider promises. The default supports a complete-image-only provider.
/// A preview is optional even when supported; no deadline is implied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceCapabilities {
    pub preview: bool,
    pub refinement: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressiveStatus {
    Loading,
    Preview,
    Refining,
    /// Provider is complete; does not mean GPU upload/presentation is complete.
    Ready,
    /// Last usable snapshot remains visible until restart/cancel/drop.
    Failed(String),
    Cancelled,
}
/// Full immutable drawable snapshots, not mutable scanline buffers. A future
/// tile provider may make T an immutable page-table snapshot. Publication never
/// authorizes modifying a resource still retained by a scene or GPU submission.
#[derive(Debug)]
pub enum SourceEvent<T> {
    Preview(Arc<T>),
    Refinement(Arc<T>),
    Complete(Arc<T>),
    Failed(String),
}
#[derive(Debug)]
pub struct SourceUpdate<T> {
    pub request: SourceRequest,
    /// Strictly increasing per request, including failures. Start at any value.
    pub sequence: u64,
    pub event: SourceEvent<T>,
}
/// CPU publication state only: no decoding, worker queue, upload or notification
/// mechanism. The host serializes updates and arranges redraws. Rejected updates
/// change nothing. Retained snapshots use Arc; retirement follows last ownership,
/// not a provider's claim that work was submitted.
#[derive(Debug)]
pub struct ProgressiveImage<T> {
    request: SourceRequest,
    capabilities: SourceCapabilities,
    sequence: Option<u64>,
    status: ProgressiveStatus,
    snapshot: Option<Arc<T>>,
}
impl<T> ProgressiveImage<T> {
    pub fn new(source: SourceId, capabilities: SourceCapabilities) -> Self {
        Self {
            request: SourceRequest {
                source,
                owner: next_owner(),
                revision: 0,
            },
            capabilities,
            sequence: None,
            status: ProgressiveStatus::Loading,
            snapshot: None,
        }
    }
    pub fn request(&self) -> SourceRequest {
        self.request
    }
    pub fn status(&self) -> &ProgressiveStatus {
        &self.status
    }
    pub fn snapshot(&self) -> Option<&Arc<T>> {
        self.snapshot.as_ref()
    }
    /// Replace the asset or invalidate work for a changed viewport. Returns the
    /// new token; previous tokens can no longer publish, including A→B→A changes.
    /// Old scene snapshots may remain alive through other Arc owners.
    pub fn restart(
        &mut self,
        source: SourceId,
        capabilities: SourceCapabilities,
    ) -> Result<SourceRequest> {
        let revision = self.request.revision.checked_add(1);
        ensure!(revision.is_some(), "source request revision exhausted");
        self.request = SourceRequest {
            source,
            owner: self.request.owner,
            revision: revision.unwrap(),
        };
        self.capabilities = capabilities;
        self.sequence = None;
        self.status = ProgressiveStatus::Loading;
        self.snapshot = None;
        Ok(self.request)
    }
    /// Suppress further publication and release this state's ownership. A codec
    /// worker need not be forcibly interruptible; its later results are rejected.
    pub fn cancel(&mut self) {
        self.status = ProgressiveStatus::Cancelled;
        self.snapshot = None;
    }
    /// Accept only current, ordered events allowed by the provider contract.
    /// Ready, Failed and Cancelled are terminal until restart. Preview cannot
    /// replace a refinement or another preview; refinement may arrive directly.
    pub fn publish(&mut self, update: SourceUpdate<T>) -> bool {
        if update.request != self.request
            || self
                .sequence
                .is_some_and(|sequence| update.sequence <= sequence)
            || matches!(
                self.status,
                ProgressiveStatus::Ready
                    | ProgressiveStatus::Failed(_)
                    | ProgressiveStatus::Cancelled
            )
        {
            return false;
        }
        match update.event {
            SourceEvent::Preview(snapshot) => {
                if !self.capabilities.preview || self.status != ProgressiveStatus::Loading {
                    return false;
                }
                self.snapshot = Some(snapshot);
                self.status = ProgressiveStatus::Preview;
            }
            SourceEvent::Refinement(snapshot) => {
                if !self.capabilities.refinement {
                    return false;
                }
                self.snapshot = Some(snapshot);
                self.status = ProgressiveStatus::Refining;
            }
            SourceEvent::Complete(snapshot) => {
                self.snapshot = Some(snapshot);
                self.status = ProgressiveStatus::Ready;
            }
            SourceEvent::Failed(message) => self.status = ProgressiveStatus::Failed(message),
        }
        self.sequence = Some(update.sequence);
        true
    }
}
