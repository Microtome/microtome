//! Concrete mesh repair passes.

pub mod fill_holes;
pub mod remove_slivers;
pub mod taubin_smooth;
pub mod weld_vertices;

pub use fill_holes::{FillSmallHoles, HoleFillMethod};
pub use remove_slivers::RemoveSlivers;
pub use taubin_smooth::TaubinSmooth;
pub use weld_vertices::WeldVertices;
