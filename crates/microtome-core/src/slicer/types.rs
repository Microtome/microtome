//! Data types and constants for the slicer pipeline.

/// Padding added to the far plane so geometry on the z=0 plane is not ambiguous.
pub(super) const FAR_Z_PADDING: f32 = 1.0;

/// Number of bytes per pixel in Rgba8Unorm format.
pub(super) const BYTES_PER_PIXEL: u32 = 4;

/// Pre-uploaded GPU buffers for a single mesh ready for slicing.
pub struct SliceMeshBuffers {
    /// Vertex buffer containing interleaved position + normal data.
    pub vertex_buffer: wgpu::Buffer,
    /// Index buffer containing triangle indices.
    pub index_buffer: wgpu::Buffer,
    /// Number of indices (triangles * 3).
    pub index_count: u32,
}

/// Intersection pass uniform data.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct IntersectionUniforms {
    pub(super) cutoff: f32,
    pub(super) _pad0: f32,
    pub(super) _pad1: f32,
    pub(super) _pad2: f32,
    pub(super) view_proj: [[f32; 4]; 4],
}

/// Slice extraction pass uniform data.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct SliceUniforms {
    pub(super) cutoff: f32,
    pub(super) _pad0: f32,
    pub(super) view_width: f32,
    pub(super) view_height: f32,
    pub(super) view_proj: [[f32; 4]; 4],
}

/// Computes the number of bytes per row, aligned to wgpu's required 256-byte alignment.
pub(super) fn aligned_bytes_per_row(width: u32) -> u32 {
    let unaligned = width * BYTES_PER_PIXEL;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unaligned.div_ceil(align) * align
}
