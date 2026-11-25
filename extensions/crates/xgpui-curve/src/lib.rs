#![doc = include_str!("../README.md")]
#![deny(missing_docs)]

//! Module map: [`geom`] is always available. Every other module has a Cargo
//! feature of the same name (`svg-path` for [`svg_path`]), all on by default.

mod geometry;

pub mod error;
pub mod geom;

#[cfg(feature = "fit")]
pub mod fit;
#[cfg(feature = "func")]
pub mod func;
#[cfg(feature = "gpui")]
pub mod gpui_paint;
#[cfg(feature = "measure")]
pub mod measure;
#[cfg(feature = "mesh")]
pub mod mesh;
#[cfg(feature = "node")]
pub mod node;
#[cfg(feature = "series")]
pub mod series;
#[cfg(feature = "shape")]
pub mod shape;
#[cfg(feature = "spine")]
pub mod spine;
#[cfg(feature = "stroke")]
pub mod stroke;
#[cfg(feature = "svg-path")]
pub mod svg_path;

pub use error::CurveError;
