//! Experimental SVG documents, bounded raster targets, progressive scheduling
//! and retained contour trim/dash effects before native stroke tessellation.
//! SvgImage owns GPUI raster tasks and timers; low-level queue APIs remain available.
mod document;
mod element;
pub mod path_effects;
mod policy;
mod queue;
mod scheduler;
pub use document::{RasterImage, SvgDocument};
pub use element::{SvgImage, SvgPhase, SvgStatus, svg_image};
pub use policy::{MIB, RasterPolicy, Resolution, image_bytes};
pub use queue::{Job, RasterQueue, Stage};
pub use scheduler::{SchedulerLimits, SchedulerStats, SvgScheduler};
