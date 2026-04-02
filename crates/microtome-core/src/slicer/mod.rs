//! GPU-accelerated mesh slicing pipeline.
//!
//! Implements the intersection-test / slice-extract two-pass algorithm
//! ported from the original TypeScript/GLSL slicer.

mod pipeline;
mod types;

#[cfg(test)]
mod tests;

pub use pipeline::AdvancedSlicer;
pub use types::SliceMeshBuffers;
