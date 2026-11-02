//! Portable, CPU-only content descriptions and execution accounting.
//!
//! This is the foundation of the portable content API. Graphs currently support
//! solids, immutable source images, copies, and typed render/compute pass graphs.
//! Portable programs are parsed, validated and reflected without native GPU access.
//! `Window::paint_content` records portable presentation requests. Native Metal
//! execution and window compositing live in the Apple backend. Assets and external
//! imports remain separate; legacy `CustomContent` producers keep their existing path.

mod asset;
mod descriptors;
mod graph;
mod group;
mod group_scene;
mod lifecycle;
mod mapping;
mod passes;
mod presentation;
mod program;
mod uniforms;

pub use asset::*;
pub use descriptors::*;
pub use graph::*;
pub use group::*;
pub use group_scene::*;
pub use lifecycle::*;
pub use mapping::*;
pub use passes::*;
pub use presentation::*;
pub use program::*;
pub use uniforms::*;

/// A structured failure while describing, planning, or tracking content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentError {
    /// Portable classification; native causes can be attached by executors later.
    pub kind: ContentErrorKind,
    /// Human-readable explanation, without resource handles.
    pub message: String,
}

/// Failures that callers can distinguish without backend-specific branching.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContentErrorKind {
    /// Invalid sizes, parameters, formats, ranges, or color contracts.
    InvalidDescriptor,
    /// A reference belongs to another graph or is not the expected resource kind.
    InvalidReference,
    /// Recursive content descriptions form a cycle.
    Cycle,
    /// Description exceeds its configured resource policy.
    ResourceLimit,
    /// A requested operation exceeds executor functionality.
    UnsupportedCapability,
    /// A lifecycle transition violates dependency or ownership ordering.
    InvalidTransition,
    /// Shader syntax, semantics, or selected stage interfaces are invalid.
    ShaderValidation,
    /// Native allocation failed before publishing an output.
    AllocationFailure,
    /// Backend compilation, submission, or GPU execution failed.
    ExecutionFailure,
    /// A request was canceled; submitted work still retires safely.
    Cancelled,
    /// A strict snapshot is not ready; retry asynchronously after native completion.
    Pending,
    /// A caller deadline expired; this does not terminate driver execution.
    Timeout,
}

impl ContentError {
    /// Construct a portable failure from a producer or executor.
    pub fn new(kind: ContentErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
impl std::fmt::Display for ContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}
impl std::error::Error for ContentError {}

pub(super) fn next_content_identity() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let mut current = NEXT.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1).expect("content identity exhausted");
        match NEXT.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return current,
            Err(actual) => current = actual,
        }
    }
}
