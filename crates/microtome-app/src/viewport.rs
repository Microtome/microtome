//! Egui paint callback integration for the 3D viewport.
//!
//! Implements [`egui_wgpu::CallbackTrait`] to bridge egui's rendering
//! pipeline with the custom [`ViewportRenderer`](super::viewport_renderer::ViewportRenderer).

use std::sync::Arc;

use glam::Mat4;

use crate::viewport_renderer::{MeshBuffers, ViewportRenderer};

/// Paint callback submitted to egui for custom 3D viewport rendering.
///
/// Carries all per-frame data needed to render the scene: camera matrices,
/// mesh GPU buffers, and selection state.
pub struct ViewportPaintCallback {
    /// Combined view-projection matrix from the orbit camera.
    pub view_proj: Mat4,
    /// GPU buffers for each mesh in the scene, shared via `Arc`.
    pub meshes: Arc<Vec<MeshBuffers>>,
    /// Index of the currently selected mesh, if any.
    pub selected_index: Option<usize>,
}

impl egui_wgpu::CallbackTrait for ViewportPaintCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen_descriptor: &egui_wgpu::ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        callback_resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = callback_resources.get_mut::<ViewportRenderer>() {
            renderer.prepare_uniforms(
                device,
                queue,
                self.view_proj,
                &self.meshes,
                self.selected_index,
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
            renderer.paint(render_pass, &self.meshes);
        }
    }
}
