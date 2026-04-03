//! Simplified viewport renderer for the isosurface viewer.
//!
//! Provides phong-lit mesh rendering, wireframe line rendering, and a blit pass
//! to composite the offscreen target into egui's render pass.

use glam::Mat4;
use wgpu::util::DeviceExt;

/// Offscreen render target format.
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Depth buffer format.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Size of one uniform entry, 256-byte aligned for dynamic offsets.
const UNIFORM_ALIGN: u64 = 256;

/// Default mesh color: light gray.
const DEFAULT_COLOR: [f32; 4] = [0.812, 0.812, 0.812, 1.0];

/// A single vertex for line/wireframe rendering (position + color).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineVertex {
    /// Vertex position in world space.
    pub position: [f32; 3],
    /// Vertex color (RGBA).
    pub color: [f32; 4],
}

/// Uniform data for a single draw call, padded to 256 bytes.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ViewportUniforms {
    /// Combined view-projection matrix.
    pub view_proj: [[f32; 4]; 4],
    /// Model-to-world transform.
    pub model: [[f32; 4]; 4],
    /// Object base color.
    pub object_color: [f32; 4],
    /// Volume minimum bounds (unused in isosurface viewer, kept for shader compat).
    pub volume_min: [f32; 3],
    pub _pad0: f32,
    /// Volume maximum bounds (unused in isosurface viewer, kept for shader compat).
    pub volume_max: [f32; 3],
    pub _pad1: f32,
    pub _padding: [f32; 4],
}

/// GPU buffers for a single mesh.
pub struct MeshBuffers {
    /// Vertex buffer containing position+normal data.
    pub vertex_buffer: wgpu::Buffer,
    /// Index buffer (u32 indices).
    pub index_buffer: wgpu::Buffer,
    /// Number of indices to draw.
    pub index_count: u32,
}

/// GPU renderer for the isosurface viewport.
///
/// Renders meshes with phong lighting and optional wireframe overlay to an
/// offscreen target, then blits the result to egui's render pass.
pub struct ViewportRenderer {
    phong_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    max_draw_calls: u32,
    line_vertex_buffer: wgpu::Buffer,
    line_vertex_count: u32,

    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    offscreen_width: u32,
    offscreen_height: u32,

    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_group: wgpu::BindGroup,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
}

impl ViewportRenderer {
    /// Creates a new viewport renderer.
    ///
    /// `target_format` is egui's surface format (for the blit pipeline).
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let max_draw_calls: u32 = 4;
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

        let depth_stencil = Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        // --- Phong pipeline ---
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
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: depth_stencil.clone(),
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &phong_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OFFSCREEN_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Line pipeline ---
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
            depth_stencil,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &line_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OFFSCREEN_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        // --- Blit pipeline ---
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blit_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blit.wgsl").into()),
        });

        let blit_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blit_bind_group_layout"),
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

        let blit_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blit_pipeline_layout"),
            bind_group_layouts: &[Some(&blit_bind_group_layout)],
            immediate_size: 0,
        });

        let blit_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit_pipeline"),
            layout: Some(&blit_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &blit_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &blit_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview_mask: None,
            cache: None,
        });

        let blit_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("blit_sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Initial 1x1 offscreen textures (resized on first frame).
        let (color_texture, color_view, depth_texture, depth_view) =
            create_offscreen_targets(device, 1, 1);

        let blit_bind_group =
            create_blit_bind_group(device, &blit_bind_group_layout, &color_view, &blit_sampler);

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
            color_texture,
            color_view,
            depth_texture,
            depth_view,
            offscreen_width: 1,
            offscreen_height: 1,
            blit_pipeline,
            blit_bind_group,
            blit_bind_group_layout,
            blit_sampler,
        }
    }

    /// Updates the wireframe line vertex buffer from mesh triangle edges.
    pub fn update_wireframe_lines(
        &mut self,
        device: &wgpu::Device,
        mesh: &microtome_core::isosurface::IsoMesh,
    ) {
        let color: [f32; 4] = [0.0, 0.0, 0.0, 0.6];
        let tri_count = mesh.indices.len() / 3;
        let mut vertices = Vec::with_capacity(tri_count * 6);

        for i in 0..tri_count {
            let i0 = mesh.indices[i * 3] as usize;
            let i1 = mesh.indices[i * 3 + 1] as usize;
            let i2 = mesh.indices[i * 3 + 2] as usize;

            let p0 = mesh.positions[i0];
            let p1 = mesh.positions[i1];
            let p2 = mesh.positions[i2];

            vertices.push(LineVertex {
                position: [p0.x, p0.y, p0.z],
                color,
            });
            vertices.push(LineVertex {
                position: [p1.x, p1.y, p1.z],
                color,
            });
            vertices.push(LineVertex {
                position: [p1.x, p1.y, p1.z],
                color,
            });
            vertices.push(LineVertex {
                position: [p2.x, p2.y, p2.z],
                color,
            });
            vertices.push(LineVertex {
                position: [p2.x, p2.y, p2.z],
                color,
            });
            vertices.push(LineVertex {
                position: [p0.x, p0.y, p0.z],
                color,
            });
        }

        self.line_vertex_count = vertices.len() as u32;
        if vertices.is_empty() {
            return;
        }
        self.line_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("line_vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    }

    /// Renders the scene to the offscreen target with depth testing.
    ///
    /// Resizes offscreen targets if viewport dimensions changed.
    #[allow(clippy::too_many_arguments)]
    pub fn render_offscreen(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        width: u32,
        height: u32,
        view_proj: Mat4,
        mesh: Option<&MeshBuffers>,
        show_wireframe: bool,
    ) {
        let w = width.max(1);
        let h = height.max(1);

        // Resize offscreen targets if needed.
        if w != self.offscreen_width || h != self.offscreen_height {
            let (ct, cv, dt, dv) = create_offscreen_targets(device, w, h);
            self.color_texture = ct;
            self.color_view = cv;
            self.depth_texture = dt;
            self.depth_view = dv;
            self.offscreen_width = w;
            self.offscreen_height = h;
            self.blit_bind_group = create_blit_bind_group(
                device,
                &self.blit_bind_group_layout,
                &self.color_view,
                &self.blit_sampler,
            );
        }

        // Write uniforms.
        let needed = 2_u32; // line + mesh
        if needed > self.max_draw_calls {
            self.grow_uniform_buffer(device, needed);
        }

        let large = 1e6_f32;
        let line_uniforms = ViewportUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            object_color: [1.0, 1.0, 1.0, 1.0],
            volume_min: [-large, -large, -large],
            _pad0: 0.0,
            volume_max: [large, large, large],
            _pad1: 0.0,
            _padding: [0.0; 4],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&line_uniforms));

        if mesh.is_some() {
            let mesh_uniforms = ViewportUniforms {
                view_proj: view_proj.to_cols_array_2d(),
                model: Mat4::IDENTITY.to_cols_array_2d(),
                object_color: DEFAULT_COLOR,
                volume_min: [-large, -large, -large],
                _pad0: 0.0,
                volume_max: [large, large, large],
                _pad1: 0.0,
                _padding: [0.0; 4],
            };
            queue.write_buffer(
                &self.uniform_buffer,
                UNIFORM_ALIGN,
                bytemuck::bytes_of(&mesh_uniforms),
            );
        }

        // Offscreen render pass.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport_offscreen_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.15,
                            g: 0.15,
                            b: 0.15,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            });

            // Draw mesh.
            if let Some(mesh_buf) = mesh {
                let offset = UNIFORM_ALIGN as u32;
                pass.set_pipeline(&self.phong_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[offset]);
                pass.set_vertex_buffer(0, mesh_buf.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh_buf.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh_buf.index_count, 0, 0..1);
            }

            // Draw wireframe lines.
            if show_wireframe && self.line_vertex_count > 0 {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                pass.set_vertex_buffer(0, self.line_vertex_buffer.slice(..));
                pass.draw(0..self.line_vertex_count, 0..1);
            }
        }
    }

    /// Blits the offscreen color texture onto egui's render pass.
    pub fn blit(&self, render_pass: &mut wgpu::RenderPass<'static>) {
        render_pass.set_pipeline(&self.blit_pipeline);
        render_pass.set_bind_group(0, &self.blit_bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }

    /// Grows the uniform buffer to hold at least `needed` entries.
    fn grow_uniform_buffer(&mut self, device: &wgpu::Device, needed: u32) {
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

/// Creates offscreen color and depth textures of the given size.
fn create_offscreen_targets(
    device: &wgpu::Device,
    width: u32,
    height: u32,
) -> (
    wgpu::Texture,
    wgpu::TextureView,
    wgpu::Texture,
    wgpu::TextureView,
) {
    let color_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewport_color"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: OFFSCREEN_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let color_view = color_texture.create_view(&Default::default());

    let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewport_depth"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth_texture.create_view(&Default::default());

    (color_texture, color_view, depth_texture, depth_view)
}

/// Creates the bind group for the blit pass.
fn create_blit_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    color_view: &wgpu::TextureView,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blit_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(color_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}
