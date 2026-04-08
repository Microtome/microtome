//! Egui paint callback integration for the isosurface 3D viewport.
//!
//! Renders the scene to an offscreen target in `prepare`, then blits
//! the result to egui's render pass in `paint`.

use std::sync::Arc;

use glam::Mat4;

use crate::renderer::{MeshBuffers, ViewportRenderer};

/// Paint callback submitted to egui for custom 3D viewport rendering.
pub struct ViewportPaintCallback {
    /// Combined view-projection matrix from the orbit camera.
    pub view_proj: Mat4,
    /// GPU mesh buffers, shared via `Arc`.
    pub mesh: Option<Arc<MeshBuffers>>,
    /// Viewport width in physical pixels.
    pub width: u32,
    /// Viewport height in physical pixels.
    pub height: u32,
    /// Whether to draw wireframe overlay.
    pub show_wireframe: bool,
}

impl egui_wgpu::CallbackTrait for ViewportPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = callback_resources.get_mut::<ViewportRenderer>() {
            renderer.render_offscreen(
                device,
                queue,
                egui_encoder,
                self.width,
                self.height,
                self.view_proj,
                self.mesh.as_deref(),
                self.show_wireframe,
            );
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &egui_wgpu::CallbackResources,
    ) {
        if let Some(renderer) = callback_resources.get::<ViewportRenderer>() {
            renderer.blit(render_pass);
        }
    }
}
