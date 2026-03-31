//! GPU-accelerated mesh slicing pipeline.
//!
//! Implements the intersection-test / slice-extract two-pass algorithm
//! ported from the original TypeScript/GLSL slicer.

use std::sync::Arc;

use image::ImageEncoder;

use crate::error::{MicrotomeError, Result};
use crate::gpu::GpuContext;

/// Padding added to the far plane so geometry on the z=0 plane is not ambiguous.
const FAR_Z_PADDING: f32 = 1.0;

/// Number of bytes per pixel in Rgba8Unorm format.
const BYTES_PER_PIXEL: u32 = 4;

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
struct IntersectionUniforms {
    cutoff: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
    view_proj: [[f32; 4]; 4],
}

/// Slice extraction pass uniform data.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SliceUniforms {
    cutoff: f32,
    _pad0: f32,
    view_width: f32,
    view_height: f32,
    view_proj: [[f32; 4]; 4],
}

/// GPU-accelerated slicer implementing the intersection-test / slice-extract pipeline.
///
/// Produces black-and-white slice images suitable for DLP projection.
#[allow(dead_code)]
pub struct AdvancedSlicer {
    gpu: GpuContext,
    // Render pipelines
    intersection_pipeline: wgpu::RenderPipeline,
    slice_pipeline: wgpu::RenderPipeline,
    // Compute pipelines
    erode_dilate_pipeline: wgpu::ComputePipeline,
    // Bind group layouts (needed to create per-frame bind groups)
    intersection_bgl: wgpu::BindGroupLayout,
    slice_bgl0: wgpu::BindGroupLayout,
    slice_bgl1: wgpu::BindGroupLayout,
    erode_dilate_bgl0: wgpu::BindGroupLayout,
    erode_dilate_bgl1: wgpu::BindGroupLayout,
    // Textures
    mask_texture: wgpu::Texture,
    scratch_texture: wgpu::Texture,
    temp1_texture: wgpu::Texture,
    temp2_texture: wgpu::Texture,
    // Texture views (cached)
    mask_view: wgpu::TextureView,
    scratch_view: wgpu::TextureView,
    temp1_view: wgpu::TextureView,
    temp2_view: wgpu::TextureView,
    // Sampler for the slice pass
    nearest_sampler: wgpu::Sampler,
    // Staging buffer for CPU readback
    staging_buffer: wgpu::Buffer,
    // Resolution
    width: u32,
    height: u32,
}

/// Computes the number of bytes per row, aligned to wgpu's required 256-byte alignment.
fn aligned_bytes_per_row(width: u32) -> u32 {
    let unaligned = width * BYTES_PER_PIXEL;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    unaligned.div_ceil(align) * align
}

impl AdvancedSlicer {
    /// Creates a new slicer with all GPU pipelines and textures at the given resolution.
    pub fn new(gpu: &GpuContext, width: u32, height: u32) -> Result<Self> {
        let device = &gpu.device;

        // Load shaders
        let intersection_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("intersection-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/intersection.wgsl").into()),
        });

        let slice_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("slice-extract-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/slice_extract.wgsl").into()),
        });

        let erode_dilate_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("erode-dilate-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/erode_dilate.wgsl").into()),
        });

        // Vertex buffer layout: position(vec3) + normal(vec3)
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<crate::mesh::MeshVertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 12,
                    shader_location: 1,
                },
            ],
        };

        // --- Intersection pipeline ---
        let intersection_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("intersection-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let intersection_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("intersection-pipeline-layout"),
                bind_group_layouts: &[Some(&intersection_bgl)],
                ..Default::default()
            });

        let intersection_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("intersection-pipeline"),
                layout: Some(&intersection_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &intersection_shader,
                    entry_point: Some("vs_main"),
                    buffers: std::slice::from_ref(&vertex_layout),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &intersection_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: Some(wgpu::BlendState {
                            color: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                            alpha: wgpu::BlendComponent {
                                src_factor: wgpu::BlendFactor::One,
                                dst_factor: wgpu::BlendFactor::One,
                                operation: wgpu::BlendOperation::Add,
                            },
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            });

        // --- Slice extraction pipeline ---
        let slice_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("slice-bgl0"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let slice_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("slice-bgl1"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let slice_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("slice-pipeline-layout"),
                bind_group_layouts: &[Some(&slice_bgl0), Some(&slice_bgl1)],
                ..Default::default()
            });

        let slice_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("slice-pipeline"),
            layout: Some(&slice_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &slice_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &slice_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        // --- Erode/Dilate compute pipeline ---
        let erode_dilate_bgl0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("erode-dilate-bgl0"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let erode_dilate_bgl1 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("erode-dilate-bgl1"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });

        let erode_dilate_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("erode-dilate-pipeline-layout"),
                bind_group_layouts: &[Some(&erode_dilate_bgl0), Some(&erode_dilate_bgl1)],
                ..Default::default()
            });

        let erode_dilate_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("erode-dilate-pipeline"),
                layout: Some(&erode_dilate_pipeline_layout),
                module: &erode_dilate_shader,
                entry_point: Some("cs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            });

        // --- Textures ---
        let tex_usage = wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC;

        let storage_tex_usage = tex_usage | wgpu::TextureUsages::STORAGE_BINDING;

        let create_texture = |label: &str, usage: wgpu::TextureUsages| -> wgpu::Texture {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage,
                view_formats: &[],
            })
        };

        let mask_texture = create_texture("mask-texture", tex_usage);
        let scratch_texture = create_texture("scratch-texture", storage_tex_usage);
        let temp1_texture = create_texture("temp1-texture", storage_tex_usage);
        let temp2_texture = create_texture("temp2-texture", tex_usage);

        let mask_view = mask_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let scratch_view = scratch_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let temp1_view = temp1_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let temp2_view = temp2_texture.create_view(&wgpu::TextureViewDescriptor::default());

        let nearest_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });

        // Staging buffer for readback
        let row_bytes = aligned_bytes_per_row(width);
        let staging_size = u64::from(row_bytes) * u64::from(height);
        let staging_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging-buffer"),
            size: staging_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        Ok(Self {
            gpu: GpuContext::from_existing(Arc::clone(&gpu.device), Arc::clone(&gpu.queue)),
            intersection_pipeline,
            slice_pipeline,
            erode_dilate_pipeline,
            intersection_bgl,
            slice_bgl0,
            slice_bgl1,
            erode_dilate_bgl0,
            erode_dilate_bgl1,
            mask_texture,
            scratch_texture,
            temp1_texture,
            temp2_texture,
            mask_view,
            scratch_view,
            temp1_view,
            temp2_view,
            nearest_sampler,
            staging_buffer,
            width,
            height,
        })
    }

    /// Runs the GPU slicing pipeline for one layer at the given z height.
    ///
    /// The orthographic projection is computed to fit the build volume
    /// (`volume_width` x `volume_depth`) proportionally into the render target.
    /// The result is stored in the mask texture and can be read back with
    /// [`read_slice_to_png`](Self::read_slice_to_png).
    pub fn slice_at(
        &self,
        z: f32,
        volume_width: f32,
        volume_depth: f32,
        volume_height: f32,
        meshes: &[SliceMeshBuffers],
    ) -> Result<()> {
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;

        let slice_z = (FAR_Z_PADDING + z) / (FAR_Z_PADDING + volume_height);

        // Build orthographic projection looking down the Z axis.
        // Scale the build volume to fit proportionally into the render target,
        // matching the TypeScript original's aspect-correct frustum calculation.
        let width_ratio = volume_width / self.width as f32;
        let height_ratio = volume_depth / self.height as f32;
        let scale = width_ratio.max(height_ratio);
        let half_w = (scale * self.width as f32) / 2.0;
        let half_h = (scale * self.height as f32) / 2.0;
        let camera_near = 1.0_f32;
        let camera_far = FAR_Z_PADDING + volume_height + camera_near;

        let view = glam::Mat4::look_at_rh(
            glam::Vec3::new(0.0, 0.0, volume_height + camera_near),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        );
        let proj =
            glam::Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, camera_near, camera_far);
        let view_proj = proj * view;

        // --- Pass 1: Intersection test ---
        let intersection_uniforms = IntersectionUniforms {
            cutoff: slice_z,
            _pad0: 0.0,
            _pad1: 0.0,
            _pad2: 0.0,
            view_proj: view_proj.to_cols_array_2d(),
        };

        let intersection_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("intersection-uniform-buf"),
            size: std::mem::size_of::<IntersectionUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &intersection_uniform_buf,
            0,
            bytemuck::bytes_of(&intersection_uniforms),
        );

        let intersection_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("intersection-bg"),
            layout: &self.intersection_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: intersection_uniform_buf.as_entire_binding(),
            }],
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("slice-encoder"),
        });

        // Intersection pass -> temp2
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("intersection-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.temp2_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            rpass.set_pipeline(&self.intersection_pipeline);
            rpass.set_bind_group(0, &intersection_bg, &[]);

            for mesh in meshes {
                rpass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rpass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                rpass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        // --- Pass 2: Slice extraction ---
        let slice_uniforms = SliceUniforms {
            cutoff: slice_z,
            _pad0: 0.0,
            view_width: self.width as f32,
            view_height: self.height as f32,
            view_proj: view_proj.to_cols_array_2d(),
        };

        let slice_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("slice-uniform-buf"),
            size: std::mem::size_of::<SliceUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&slice_uniform_buf, 0, bytemuck::bytes_of(&slice_uniforms));

        let slice_bg0 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("slice-bg0"),
            layout: &self.slice_bgl0,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: slice_uniform_buf.as_entire_binding(),
            }],
        });

        let slice_bg1 = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("slice-bg1"),
            layout: &self.slice_bgl1,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.temp2_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.nearest_sampler),
                },
            ],
        });

        // Slice pass -> mask
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("slice-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.mask_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            rpass.set_pipeline(&self.slice_pipeline);
            rpass.set_bind_group(0, &slice_bg0, &[]);
            rpass.set_bind_group(1, &slice_bg1, &[]);

            for mesh in meshes {
                rpass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                rpass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                rpass.draw_indexed(0..mesh.index_count, 0, 0..1);
            }
        }

        queue.submit(std::iter::once(encoder.finish()));

        Ok(())
    }

    /// Reads the current mask texture back to the CPU and encodes it as PNG.
    ///
    /// The PNG data is appended to the provided `output` buffer.
    pub fn read_slice_to_png(&self, output: &mut Vec<u8>) -> Result<()> {
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;

        let row_bytes = aligned_bytes_per_row(self.width);

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("readback-encoder"),
        });

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.mask_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.staging_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_bytes),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );

        queue.submit(std::iter::once(encoder.finish()));

        // Map the staging buffer and read the data
        let buffer_slice = self.staging_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| MicrotomeError::Slicing(format!("Device poll failed: {e}")))?;

        rx.recv()
            .map_err(|e| MicrotomeError::Slicing(format!("Buffer map channel error: {e}")))?
            .map_err(|e| MicrotomeError::Slicing(format!("Buffer map failed: {e}")))?;

        let mapped = buffer_slice.get_mapped_range();

        // Strip row padding and collect pixel data
        let actual_row_bytes = (self.width * BYTES_PER_PIXEL) as usize;
        let mut pixels = Vec::with_capacity(actual_row_bytes * self.height as usize);
        for row in 0..self.height as usize {
            let start = row * row_bytes as usize;
            let end = start + actual_row_bytes;
            pixels.extend_from_slice(&mapped[start..end]);
        }

        drop(mapped);
        self.staging_buffer.unmap();

        // Encode as PNG
        let mut png_data = std::io::Cursor::new(output);
        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
        encoder
            .write_image(
                &pixels,
                self.width,
                self.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| MicrotomeError::Image(e.to_string()))?;

        Ok(())
    }

    /// Returns the width of the slice output in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Returns the height of the slice output in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

#[cfg(test)]
mod tests {
    /// Parses a WGSL shader and validates it using naga, returning any error.
    fn validate_wgsl(source: &str, label: &str) {
        let module = naga::front::wgsl::parse_str(source)
            .unwrap_or_else(|e| panic!("{label}: WGSL parse error: {e}"));

        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|e| panic!("{label}: WGSL validation error: {e}"));
    }

    #[test]
    fn intersection_shader_is_valid_wgsl() {
        validate_wgsl(
            include_str!("shaders/intersection.wgsl"),
            "intersection.wgsl",
        );
    }

    #[test]
    fn slice_extract_shader_is_valid_wgsl() {
        validate_wgsl(
            include_str!("shaders/slice_extract.wgsl"),
            "slice_extract.wgsl",
        );
    }

    #[test]
    fn erode_dilate_shader_is_valid_wgsl() {
        validate_wgsl(
            include_str!("shaders/erode_dilate.wgsl"),
            "erode_dilate.wgsl",
        );
    }

    #[test]
    fn boolean_ops_shader_is_valid_wgsl() {
        validate_wgsl(include_str!("shaders/boolean_ops.wgsl"), "boolean_ops.wgsl");
    }

    #[test]
    fn overhang_shader_is_valid_wgsl() {
        validate_wgsl(include_str!("shaders/overhang.wgsl"), "overhang.wgsl");
    }
}
