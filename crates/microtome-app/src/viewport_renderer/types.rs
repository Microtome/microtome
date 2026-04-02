//! Data types and constants for the viewport renderer.

use glam::Mat4;

/// Default object color: gray #cfcfcf.
pub(super) const DEFAULT_COLOR: [f32; 4] = [0.812, 0.812, 0.812, 1.0];

/// Selected object color: cyan #00cfcf.
pub(super) const SELECTED_COLOR: [f32; 4] = [0.0, 0.812, 0.812, 1.0];

/// Offscreen render target format.
pub(super) const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Depth buffer format.
pub(super) const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Size of one uniform entry, guaranteed to be 256 bytes.
pub(super) const UNIFORM_ALIGN: u64 = 256;

/// A single vertex for line rendering (position + color).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct LineVertex {
    pub(super) position: [f32; 3],
    pub(super) color: [f32; 4],
}

/// A vertex for the slice overlay quad (position + UV).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct OverlayVertex {
    pub(super) position: [f32; 3],
    pub(super) uv: [f32; 2],
}

/// Uniforms for the slice overlay (just the view-projection matrix).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct OverlayUniforms {
    pub(super) view_proj: [[f32; 4]; 4],
}

/// Uniform data for a single draw call.
///
/// Padded to 256-byte alignment for dynamic uniform buffer offsets.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct ViewportUniforms {
    pub(super) view_proj: [[f32; 4]; 4],
    pub(super) model: [[f32; 4]; 4],
    pub(super) object_color: [f32; 4],
    pub(super) volume_min: [f32; 3],
    pub(super) _pad0: f32,
    pub(super) volume_max: [f32; 3],
    pub(super) _pad1: f32,
    pub(super) _padding: [f32; 4],
}

/// GPU buffers for a single mesh, ready for rendering.
pub struct MeshBuffers {
    /// Vertex buffer containing [`MeshVertex`](microtome_core::MeshVertex) data.
    pub vertex_buffer: wgpu::Buffer,
    /// Index buffer (u32 indices).
    pub index_buffer: wgpu::Buffer,
    /// Number of indices to draw.
    pub index_count: u32,
    /// Model-to-world transform matrix.
    pub model_matrix: Mat4,
}
