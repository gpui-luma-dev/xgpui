//! Experimental static SVG documents, bounded raster targets and progressive scheduling.
//! SvgImage owns GPUI raster tasks and timers; low-level queue APIs remain available.
mod document;
mod element;
mod policy;
mod queue;
mod scheduler;
pub use document::{RasterImage, SvgDocument};
pub use element::{SvgImage, SvgPhase, SvgStatus, svg_image};
pub use policy::{MIB, RasterPolicy, Resolution, image_bytes};
pub use queue::{Job, RasterQueue, Stage};
pub use scheduler::{SchedulerLimits, SchedulerStats, SvgScheduler};
