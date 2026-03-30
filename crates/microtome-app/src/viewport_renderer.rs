//! GPU viewport renderer for 3D mesh and wireframe rendering.
//!
//! Provides Phong-lit mesh rendering and colored wireframe lines for the
//! print volume box. Designed to be stored in egui-wgpu's `CallbackResources`
//! and driven by [`ViewportPaintCallback`](super::viewport::ViewportPaintCallback).

use glam::Mat4;
use microtome_core::PrintVolumeBox;
use wgpu::util::DeviceExt;

/// Default object color: gray #cfcfcf.
const DEFAULT_COLOR: [f32; 4] = [0.812, 0.812, 0.812, 1.0];

/// Selected object color: cyan #00cfcf.
const SELECTED_COLOR: [f32; 4] = [0.0, 0.812, 0.812, 1.0];

/// A single vertex for line rendering (position + color).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    position: [f32; 3],
    color: [f32; 4],
}

/// Uniform data for a single draw call.
///
/// The line pipeline only reads `view_proj`; the Phong pipeline uses all fields.
/// Padded to 256-byte alignment for dynamic uniform buffer offsets.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    object_color: [f32; 4],
    /// Padding to reach 256-byte alignment (required by wgpu for dynamic offsets).
    _padding: [f32; 12],
}

/// Size of one uniform entry, guaranteed to be 256 bytes.
const UNIFORM_ALIGN: u64 = 256;

/// GPU renderer for the 3D viewport.
///
/// Holds wgpu pipelines, uniform buffers, and the print-volume wireframe
/// geometry. Created once and stored in egui-wgpu `CallbackResources`.
pub struct ViewportRenderer {
    phong_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    /// Maximum number of draw calls the uniform buffer can hold.
    max_draw_calls: u32,
    line_vertex_buffer: wgpu::Buffer,
    line_vertex_count: u32,
}

impl ViewportRenderer {
    /// Creates a new viewport renderer with the given target surface format.
    ///
    /// Pipelines render without depth testing since they draw into egui's
    /// render pass which does not include a depth attachment.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        // Start with space for 16 draw calls (1 lines + 15 meshes).
        let max_draw_calls: u32 = 16;
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport_uniforms"),
            size: UNIFORM_ALIGN * u64::from(max_draw_calls),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewport_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<ViewportUniforms>() as u64,
                    ),
                },
                count: None,
            }],
        });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniform_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<ViewportUniforms>() as u64),
                }),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        // --- Phong pipeline (no depth stencil — renders into egui's pass) ---
        let phong_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("phong_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/phong.wgsl").into()),
        });

        let phong_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("phong_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &phong_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<microtome_core::MeshVertex>() as u64,
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
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &phong_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Line pipeline (no depth stencil) ---
        let line_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("line_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/line.wgsl").into()),
        });

        let line_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("line_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &line_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<LineVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x4,
                            offset: 12,
                            shader_location: 1,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // Empty line vertex buffer placeholder.
        let line_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("line_vertices"),
            size: 4,
            usage: wgpu::BufferUsages::VERTEX,
            mapped_at_creation: false,
        });

        Self {
            phong_pipeline,
            line_pipeline,
            uniform_buffer,
            uniform_bind_group,
            max_draw_calls,
            line_vertex_buffer,
            line_vertex_count: 0,
        }
    }

    /// Rebuilds the wireframe vertex buffer for the print volume box.
    ///
    /// Axis-aligned edges are colored by axis: X=red, Y=green, Z=blue.
    /// The box is centered on XY at the origin, Z goes from 0 to height.
    pub fn update_volume_lines(&mut self, device: &wgpu::Device, volume: &PrintVolumeBox) {
        let hw = volume.width as f32 / 2.0;
        let hd = volume.depth as f32 / 2.0;
        let h = volume.height as f32;

        // 8 corners: bottom (z=0) and top (z=h)
        let c = [
            [-hw, -hd, 0.0],
            [hw, -hd, 0.0],
            [hw, hd, 0.0],
            [-hw, hd, 0.0],
            [-hw, -hd, h],
            [hw, -hd, h],
            [hw, hd, h],
            [-hw, hd, h],
        ];

        let red: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        let green: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
        let blue: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

        // 12 edges grouped by axis direction
        let edges: [(usize, usize, [f32; 4]); 12] = [
            (0, 1, red),
            (3, 2, red),
            (4, 5, red),
            (7, 6, red),
            (0, 3, green),
            (1, 2, green),
            (4, 7, green),
            (5, 6, green),
            (0, 4, blue),
            (1, 5, blue),
            (2, 6, blue),
            (3, 7, blue),
        ];

        let mut vertices = Vec::with_capacity(24);
        for (a, b, color) in edges {
            vertices.push(LineVertex {
                position: c[a],
                color,
            });
            vertices.push(LineVertex {
                position: c[b],
                color,
            });
        }

        self.line_vertex_count = vertices.len() as u32;
        self.line_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("line_vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    }

    /// Writes all uniform data for the current frame into the uniform buffer.
    ///
    /// Slot 0 is used for line rendering, slots 1..N+1 for meshes.
    /// If the buffer is too small, it is reallocated.
    pub fn prepare_uniforms(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        view_proj: Mat4,
        meshes: &[MeshBuffers],
        selected_index: Option<usize>,
    ) {
        let needed = 1 + meshes.len() as u32; // 1 for lines + N meshes
        if needed > self.max_draw_calls {
            self.grow_uniform_buffer(device, needed);
        }

        // Slot 0: line uniforms
        let line_uniforms = ViewportUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            object_color: [1.0, 1.0, 1.0, 1.0],
            _padding: [0.0; 12],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&line_uniforms));

        // Slots 1..N+1: mesh uniforms
        for (i, mesh_buf) in meshes.iter().enumerate() {
            let color = if selected_index == Some(i) {
                SELECTED_COLOR
            } else {
                DEFAULT_COLOR
            };
            let uniforms = ViewportUniforms {
                view_proj: view_proj.to_cols_array_2d(),
                model: mesh_buf.model_matrix.to_cols_array_2d(),
                object_color: color,
                _padding: [0.0; 12],
            };
            let offset = UNIFORM_ALIGN * (i as u64 + 1);
            queue.write_buffer(&self.uniform_buffer, offset, bytemuck::bytes_of(&uniforms));
        }
    }

    /// Issues draw calls for the wireframe and all meshes.
    ///
    /// Must be called after [`prepare_uniforms`](Self::prepare_uniforms) has
    /// written uniform data for the current frame.
    pub fn paint(&self, render_pass: &mut wgpu::RenderPass<'static>, meshes: &[MeshBuffers]) {
        // Draw wireframe lines (uniform slot 0)
        if self.line_vertex_count > 0 {
            render_pass.set_pipeline(&self.line_pipeline);
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
            render_pass.set_vertex_buffer(0, self.line_vertex_buffer.slice(..));
            render_pass.draw(0..self.line_vertex_count, 0..1);
        }

        // Draw meshes (uniform slots 1..N+1)
        for (i, mesh_buf) in meshes.iter().enumerate() {
            let offset = (UNIFORM_ALIGN * (i as u64 + 1)) as u32;
            render_pass.set_pipeline(&self.phong_pipeline);
            render_pass.set_bind_group(0, &self.uniform_bind_group, &[offset]);
            render_pass.set_vertex_buffer(0, mesh_buf.vertex_buffer.slice(..));
            render_pass
                .set_index_buffer(mesh_buf.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..mesh_buf.index_count, 0, 0..1);
        }
    }

    /// Grows the uniform buffer to hold at least `needed` entries.
    fn grow_uniform_buffer(&mut self, device: &wgpu::Device, needed: u32) {
        // Round up to next power of two for growth.
        let new_max = needed.next_power_of_two();
        self.uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewport_uniforms"),
            size: UNIFORM_ALIGN * u64::from(new_max),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group_layout = self.phong_pipeline.get_bind_group_layout(0);
        self.uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewport_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &self.uniform_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<ViewportUniforms>() as u64),
                }),
            }],
        });

        self.max_draw_calls = new_max;
    }
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
    fn phong_shader_is_valid_wgsl() {
        validate_wgsl(include_str!("shaders/phong.wgsl"), "phong.wgsl");
    }

    #[test]
    fn line_shader_is_valid_wgsl() {
        validate_wgsl(include_str!("shaders/line.wgsl"), "line.wgsl");
    }
}
