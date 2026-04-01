//! GPU viewport renderer for 3D mesh and wireframe rendering.
//!
//! Renders to an offscreen color+depth target during `prepare`, then blits
//! the result onto egui's render pass during `paint`. This allows proper
//! depth testing for solid mesh rendering.

use glam::Mat4;
use microtome_core::PrintVolumeBox;
use wgpu::util::DeviceExt;

/// Default object color: gray #cfcfcf.
const DEFAULT_COLOR: [f32; 4] = [0.812, 0.812, 0.812, 1.0];

/// Selected object color: cyan #00cfcf.
const SELECTED_COLOR: [f32; 4] = [0.0, 0.812, 0.812, 1.0];

/// Offscreen render target format.
const OFFSCREEN_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Depth buffer format.
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// A single vertex for line rendering (position + color).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
    position: [f32; 3],
    color: [f32; 4],
}

/// Uniform data for a single draw call.
///
/// Padded to 256-byte alignment for dynamic uniform buffer offsets.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ViewportUniforms {
    view_proj: [[f32; 4]; 4],
    model: [[f32; 4]; 4],
    object_color: [f32; 4],
    volume_min: [f32; 3],
    _pad0: f32,
    volume_max: [f32; 3],
    _pad1: f32,
    _padding: [f32; 4],
}

/// Size of one uniform entry, guaranteed to be 256 bytes.
const UNIFORM_ALIGN: u64 = 256;

/// GPU renderer for the 3D viewport.
///
/// Renders meshes and wireframe to an offscreen target with depth testing,
/// then blits the result to egui's render pass.
pub struct ViewportRenderer {
    // --- Scene rendering pipelines (target offscreen) ---
    phong_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    max_draw_calls: u32,
    line_vertex_buffer: wgpu::Buffer,
    line_vertex_count: u32,
    slice_plane_pipeline: wgpu::RenderPipeline,
    slice_plane_buffer: wgpu::Buffer,
    slice_plane_count: u32,

    // --- Offscreen render targets ---
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_texture: wgpu::Texture,
    depth_view: wgpu::TextureView,
    offscreen_width: u32,
    offscreen_height: u32,

    // --- Blit pipeline (draws offscreen texture to egui's pass) ---
    blit_pipeline: wgpu::RenderPipeline,
    blit_bind_group: wgpu::BindGroup,
    blit_bind_group_layout: wgpu::BindGroupLayout,
    blit_sampler: wgpu::Sampler,
}

impl ViewportRenderer {
    /// Creates a new viewport renderer.
    ///
    /// `target_format` is egui's surface format (for the blit pipeline).
    /// Scene pipelines target `OFFSCREEN_FORMAT` with depth testing.
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
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

        let depth_stencil = Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        });

        // --- Phong pipeline (offscreen with depth) ---
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

        // --- Line pipeline (offscreen with depth) ---
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
            depth_stencil: depth_stencil.clone(),
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

        // --- Slice plane pipeline (translucent filled quad, same shader as lines) ---
        let slice_plane_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("slice_plane_pipeline"),
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
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: depth_stencil.clone(),
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

        // --- Blit pipeline (draws offscreen texture to egui's pass) ---
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

        let slice_plane_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("slice_plane_vertices"),
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
            slice_plane_pipeline,
            slice_plane_buffer,
            slice_plane_count: 0,
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

    /// Rebuilds the wireframe vertex buffer for the print volume box.
    ///
    /// Axis-aligned edges are colored by axis: X=red, Y=green, Z=blue.
    pub fn update_volume_lines(&mut self, device: &wgpu::Device, volume: &PrintVolumeBox) {
        let hw = volume.width as f32 / 2.0;
        let hd = volume.depth as f32 / 2.0;
        let h = volume.height as f32;

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

    /// Rebuilds the slice plane quad at the given Z height.
    pub fn update_slice_plane(
        &mut self,
        device: &wgpu::Device,
        volume: &PrintVolumeBox,
        slice_z: f32,
    ) {
        let hw = volume.width as f32 / 2.0;
        let hd = volume.depth as f32 / 2.0;
        let z = slice_z;

        // Translucent blue-ish color
        let color: [f32; 4] = [0.2, 0.5, 1.0, 0.25];

        // Two triangles forming a quad at z height
        let vertices = vec![
            LineVertex {
                position: [-hw, -hd, z],
                color,
            },
            LineVertex {
                position: [hw, -hd, z],
                color,
            },
            LineVertex {
                position: [hw, hd, z],
                color,
            },
            LineVertex {
                position: [-hw, -hd, z],
                color,
            },
            LineVertex {
                position: [hw, hd, z],
                color,
            },
            LineVertex {
                position: [-hw, hd, z],
                color,
            },
        ];

        self.slice_plane_count = vertices.len() as u32;
        self.slice_plane_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("slice_plane_vertices"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    }

    /// Renders the scene to the offscreen target with depth testing.
    ///
    /// Call this from the paint callback's `prepare` method. Resizes the
    /// offscreen targets if the viewport dimensions changed.
    #[allow(clippy::too_many_arguments)]
    pub fn render_offscreen(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        width: u32,
        height: u32,
        view_proj: Mat4,
        meshes: &[MeshBuffers],
        selected_index: Option<usize>,
        volume_min: [f32; 3],
        volume_max: [f32; 3],
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
        let needed = 1 + meshes.len() as u32;
        if needed > self.max_draw_calls {
            self.grow_uniform_buffer(device, needed);
        }

        let line_uniforms = ViewportUniforms {
            view_proj: view_proj.to_cols_array_2d(),
            model: Mat4::IDENTITY.to_cols_array_2d(),
            object_color: [1.0, 1.0, 1.0, 1.0],
            volume_min,
            _pad0: 0.0,
            volume_max,
            _pad1: 0.0,
            _padding: [0.0; 4],
        };
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&line_uniforms));

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
                volume_min,
                _pad0: 0.0,
                volume_max,
                _pad1: 0.0,
                _padding: [0.0; 4],
            };
            let offset = UNIFORM_ALIGN * (i as u64 + 1);
            queue.write_buffer(&self.uniform_buffer, offset, bytemuck::bytes_of(&uniforms));
        }

        // Run the offscreen render pass.
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

            // Draw wireframe lines.
            if self.line_vertex_count > 0 {
                pass.set_pipeline(&self.line_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                pass.set_vertex_buffer(0, self.line_vertex_buffer.slice(..));
                pass.draw(0..self.line_vertex_count, 0..1);
            }

            // Draw meshes.
            for (i, mesh_buf) in meshes.iter().enumerate() {
                let offset = (UNIFORM_ALIGN * (i as u64 + 1)) as u32;
                pass.set_pipeline(&self.phong_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[offset]);
                pass.set_vertex_buffer(0, mesh_buf.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh_buf.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh_buf.index_count, 0, 0..1);
            }

            // Draw translucent slice plane (after meshes so it blends on top).
            if self.slice_plane_count > 0 {
                pass.set_pipeline(&self.slice_plane_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                pass.set_vertex_buffer(0, self.slice_plane_buffer.slice(..));
                pass.draw(0..self.slice_plane_count, 0..1);
            }
        }
    }

    /// Blits the offscreen color texture onto egui's render pass.
    ///
    /// Call this from the paint callback's `paint` method.
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

    #[test]
    fn blit_shader_is_valid_wgsl() {
        validate_wgsl(include_str!("shaders/blit.wgsl"), "blit.wgsl");
    }
}
