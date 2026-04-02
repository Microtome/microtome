//! Rendering methods for the viewport renderer.

use glam::Mat4;
use microtome_core::PrintVolumeBox;
use wgpu::util::DeviceExt;

use super::helpers::{create_blit_bind_group, create_offscreen_targets};
use super::pipeline::ViewportRenderer;
use super::types::*;

impl ViewportRenderer {
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

        // Translucent blue-ish color (used as fallback when no overlay texture)
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

        // Also build overlay quad with UVs mapped [0,1]x[0,1] to build volume XY
        let overlay_vertices = vec![
            OverlayVertex {
                position: [-hw, -hd, z],
                uv: [0.0, 1.0],
            },
            OverlayVertex {
                position: [hw, -hd, z],
                uv: [1.0, 1.0],
            },
            OverlayVertex {
                position: [hw, hd, z],
                uv: [1.0, 0.0],
            },
            OverlayVertex {
                position: [-hw, -hd, z],
                uv: [0.0, 1.0],
            },
            OverlayVertex {
                position: [hw, hd, z],
                uv: [1.0, 0.0],
            },
            OverlayVertex {
                position: [-hw, hd, z],
                uv: [0.0, 0.0],
            },
        ];

        self.overlay_vertex_count = overlay_vertices.len() as u32;
        self.overlay_vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay_vertices"),
            contents: bytemuck::cast_slice(&overlay_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
    }

    /// Updates the overlay bind group with a new slice texture view.
    pub fn update_overlay_bind_group(
        &mut self,
        device: &wgpu::Device,
        texture_view: &wgpu::TextureView,
    ) {
        self.overlay_texture_bind_group =
            Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("overlay_texture_bind_group"),
                layout: &self.overlay_texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(texture_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.overlay_sampler),
                    },
                ],
            }));
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
        show_overlay: bool,
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
            // Only draw when we don't have the textured overlay active.
            if self.slice_plane_count > 0 && !show_overlay {
                pass.set_pipeline(&self.slice_plane_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[0]);
                pass.set_vertex_buffer(0, self.slice_plane_buffer.slice(..));
                pass.draw(0..self.slice_plane_count, 0..1);
            }
        }

        // Draw the textured slice overlay in a separate pass without depth testing.
        if show_overlay
            && self.overlay_vertex_count > 0
            && let Some(ref tex_bg) = self.overlay_texture_bind_group
        {
            // Write overlay uniforms
            let overlay_uniforms = OverlayUniforms {
                view_proj: view_proj.to_cols_array_2d(),
            };
            queue.write_buffer(
                &self.overlay_uniform_buffer,
                0,
                bytemuck::bytes_of(&overlay_uniforms),
            );

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport_overlay_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // preserve the scene
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None, // no depth test
                ..Default::default()
            });

            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_bind_group(0, &self.overlay_uniform_bind_group, &[]);
            pass.set_bind_group(1, tex_bg, &[]);
            pass.set_vertex_buffer(0, self.overlay_vertex_buffer.slice(..));
            pass.draw(0..self.overlay_vertex_count, 0..1);
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
