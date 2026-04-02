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
    /// Whether the slice must be re-rendered (e.g., volume config changed).
    slice_dirty: bool,
    /// Preview width in pixels.
    preview_width: u32,
    /// Preview height in pixels.
    preview_height: u32,
    /// Raw wgpu texture for the viewport overlay (updated alongside the egui texture).
    wgpu_texture: Option<wgpu::Texture>,
    /// Texture view for the viewport overlay.
    wgpu_texture_view: Option<wgpu::TextureView>,
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
            slice_dirty: false,
            preview_width,
            preview_height,
            wgpu_texture: None,
            wgpu_texture_view: None,
        }
    }

    /// Marks the mesh buffers as needing to be rebuilt (e.g., when meshes are loaded or removed).
    pub fn mark_buffers_dirty(&mut self) {
        self.buffers_dirty = true;
        self.slice_dirty = true;
    }

    /// Marks the slice as needing re-render (e.g., volume dimensions changed).
    pub fn mark_slice_dirty(&mut self) {
        self.slice_dirty = true;
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
    ///
    /// The mesh's position, rotation, and scale transforms are baked into the
    /// vertex data since the slicer shaders don't use a model matrix.
    pub fn update_mesh_buffers(&mut self, gpu: &GpuContext, meshes: &[PrintMesh]) {
        self.mesh_buffers.clear();
        for mesh in meshes {
            // Bake the mesh transform into vertices for the slicer
            let model = glam::Mat4::from_scale_rotation_translation(
                mesh.scale,
                glam::Quat::from_euler(
                    glam::EulerRot::XYZ,
                    mesh.rotation.x,
                    mesh.rotation.y,
                    mesh.rotation.z,
                ),
                mesh.position,
            );

            let transformed_vertices: Vec<microtome_core::MeshVertex> = mesh
                .mesh_data
                .vertices
                .iter()
                .map(|v| {
                    let pos = glam::Vec3::from(v.position);
                    let norm = glam::Vec3::from(v.normal);
                    let new_pos = model.transform_point3(pos);
                    let new_norm = model.transform_vector3(norm).normalize_or_zero();
                    microtome_core::MeshVertex {
                        position: new_pos.into(),
                        normal: new_norm.into(),
                    }
                })
                .collect();

            let vertex_buffer = gpu
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("slice-preview-vertices"),
                    contents: bytemuck::cast_slice(&transformed_vertices),
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
        // Only re-slice when z actually changed or a re-render was requested
        if (self.current_z - z).abs() < f32::EPSILON && !self.slice_dirty {
            return Ok(());
        }

        self.ensure_slicer(gpu)?;
        self.slice_dirty = false;

        let slicer = match &self.slicer {
            Some(s) => s,
            None => return Ok(()),
        };

        // Run the slicer even with no meshes — produces an all-black image
        // from the clear color, ensuring the preview texture always exists.
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

        // Create wgpu texture for viewport overlay from the same RGBA data
        let rgba_data = color_image
            .pixels
            .iter()
            .flat_map(|c| [c.r(), c.g(), c.b(), c.a()])
            .collect::<Vec<u8>>();
        let tex_width = color_image.size[0] as u32;
        let tex_height = color_image.size[1] as u32;

        let wgpu_tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("slice_overlay_texture"),
            size: wgpu::Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &wgpu_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba_data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * tex_width),
                rows_per_image: Some(tex_height),
            },
            wgpu::Extent3d {
                width: tex_width,
                height: tex_height,
                depth_or_array_layers: 1,
            },
        );

        let wgpu_view = wgpu_tex.create_view(&Default::default());
        self.wgpu_texture = Some(wgpu_tex);
        self.wgpu_texture_view = Some(wgpu_view);

        self.texture =
            Some(ctx.load_texture("slice_preview", color_image, egui::TextureOptions::NEAREST));
        self.current_z = z;

        Ok(())
    }

    /// Returns the raw wgpu texture view of the latest slice (for viewport overlay).
    pub fn wgpu_texture_view(&self) -> Option<&wgpu::TextureView> {
        self.wgpu_texture_view.as_ref()
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
