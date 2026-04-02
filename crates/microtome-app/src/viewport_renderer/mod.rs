//! GPU viewport renderer for 3D mesh and wireframe rendering.
//!
//! Renders to an offscreen color+depth target during `prepare`, then blits
//! the result onto egui's render pass during `paint`. This allows proper
//! depth testing for solid mesh rendering.

mod helpers;
mod pipeline;
mod render;
mod types;

#[cfg(test)]
mod tests;

pub use pipeline::ViewportRenderer;
pub use types::MeshBuffers;
