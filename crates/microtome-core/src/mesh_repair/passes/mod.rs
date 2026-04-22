//! Concrete mesh repair passes.

pub mod fill_holes;
pub mod weld_vertices;

pub use fill_holes::{FillSmallHoles, HoleFillMethod};
pub use weld_vertices::WeldVertices;
