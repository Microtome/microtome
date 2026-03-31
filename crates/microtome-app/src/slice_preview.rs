//! 2D slice preview for visualizing slicer output at a given Z height.

use anyhow::Result;
use microtome_core::{AdvancedSlicer, GpuContext, PrintMesh, SliceMeshBuffers};
use wgpu::util::DeviceExt;

/// Manages the 2D slice preview, running the slicer and displaying results as an egui texture.
pub struct SlicePreview {
    /// The GPU slicer instance (created lazily when render state is available).
    slicer: Option<AdvancedSlicer>,
    /// Pre-uploaded mesh buffers for the slicer.
    mesh_buffers: Vec<SliceMeshBuffers>,
    /// Current preview texture handle.
    texture: Option<egui::TextureHandle>,
    /// The Z height of the currently displayed slice.
    current_z: f32,
    /// Whether the mesh buffers need to be rebuilt.
    buffers_dirty: bool,
    /// Preview width in pixels.
    preview_width: u32,
    /// Preview height in pixels.
    preview_height: u32,
}

impl SlicePreview {
    /// Creates a new slice preview with the given resolution.
    pub fn new(preview_width: u32, preview_height: u32) -> Self {
        Self {
            slicer: None,
            mesh_buffers: Vec::new(),
            texture: None,
            current_z: -1.0, // sentinel so first update always runs
            buffers_dirty: false,
            preview_width,
            preview_height,
        }
    }

    /// Marks the mesh buffers as needing to be rebuilt (e.g., when meshes are loaded or removed).
    pub fn mark_buffers_dirty(&mut self) {
        self.buffers_dirty = true;
    }

    /// Returns whether the mesh buffers need to be rebuilt.
    pub fn buffers_dirty(&self) -> bool {
        self.buffers_dirty
    }

    /// Lazily creates the slicer at preview resolution if not already created.
    pub fn ensure_slicer(&mut self, gpu: &GpuContext) -> Result<()> {
        if self.slicer.is_none() {
            let slicer = AdvancedSlicer::new(gpu, self.preview_width, self.preview_height)?;
            self.slicer = Some(slicer);
        }
        Ok(())
    }

    /// Uploads mesh geometry to GPU buffers for the slicer.
    pub fn update_mesh_buffers(&mut self, gpu: &GpuContext, meshes: &[PrintMesh]) {
        self.mesh_buffers.clear();
        for mesh in meshes {
            let vertex_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("slice-preview-vertices"),
                    contents: bytemuck::cast_slice(&mesh.mesh_data.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

            let index_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("slice-preview-indices"),
                    contents: bytemuck::cast_slice(&mesh.mesh_data.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

            self.mesh_buffers.push(SliceMeshBuffers {
                vertex_buffer,
                index_buffer,
                index_count: mesh.mesh_data.indices.len() as u32,
            });
        }
        self.buffers_dirty = false;
    }

    /// Runs the slicer if the Z height changed and uploads the result as an egui texture.
    ///
    /// The build volume dimensions (`volume_width`, `volume_depth`, `volume_height`)
    /// are forwarded to the slicer for correct orthographic projection.
    pub fn update_slice(
        &mut self,
        ctx: &egui::Context,
        gpu: &GpuContext,
        z: f32,
        volume_width: f32,
        volume_depth: f32,
        volume_height: f32,
    ) -> Result<()> {
        // Only re-slice when z actually changed
        if (self.current_z - z).abs() < f32::EPSILON && !self.buffers_dirty {
            return Ok(());
        }

        self.ensure_slicer(gpu)?;

        if self.buffers_dirty {
            // Buffers were already updated externally; just clear the flag
            self.buffers_dirty = false;
        }

        let slicer = match &self.slicer {
            Some(s) => s,
            None => return Ok(()),
        };

        if self.mesh_buffers.is_empty() {
            self.current_z = z;
            return Ok(());
        }

        slicer.slice_at(
            z,
            volume_width,
            volume_depth,
            volume_height,
            &self.mesh_buffers,
        )?;

        let mut png_bytes = Vec::new();
        slicer.read_slice_to_png(&mut png_bytes)?;

        let color_image = decode_png_to_color_image(&png_bytes)?;

        self.texture =
            Some(ctx.load_texture("slice_preview", color_image, egui::TextureOptions::NEAREST));
        self.current_z = z;

        Ok(())
    }

    /// Displays the preview texture in the UI, scaled to fit the available space.
    pub fn show(&self, ui: &mut egui::Ui) {
        if let Some(tex) = &self.texture {
            let available = ui.available_size();
            let tex_size = tex.size_vec2();
            let scale = (available.x / tex_size.x).min(available.y / tex_size.y);
            let display_size = tex_size * scale;
            ui.image(egui::load::SizedTexture::new(tex.id(), display_size));
        } else {
            ui.label("No slice preview");
        }
    }
}

/// Decodes PNG bytes into an `egui::ColorImage`.
fn decode_png_to_color_image(png_bytes: &[u8]) -> Result<egui::ColorImage> {
    let img = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| anyhow::anyhow!("Failed to decode slice PNG: {e}"))?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(
        size,
        rgba.as_raw(),
    ))
}
