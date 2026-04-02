//! Egui paint callback integration for the 3D viewport.
//!
//! Renders the scene to an offscreen target with depth testing in `prepare`,
//! then blits the result to egui's render pass in `paint`.

use std::sync::Arc;

use glam::Mat4;

use crate::viewport_renderer::{MeshBuffers, ViewportRenderer};

/// Paint callback submitted to egui for custom 3D viewport rendering.
pub struct ViewportPaintCallback {
    /// Combined view-projection matrix from the orbit camera.
    pub view_proj: Mat4,
    /// GPU buffers for each mesh in the scene, shared via `Arc`.
    pub meshes: Arc<Vec<MeshBuffers>>,
    /// Index of the currently selected mesh, if any.
    pub selected_index: Option<usize>,
    /// Viewport width in physical pixels.
    pub width: u32,
    /// Viewport height in physical pixels.
    pub height: u32,
    /// Print volume minimum bounds (world space).
    pub volume_min: [f32; 3],
    /// Print volume maximum bounds (world space).
    pub volume_max: [f32; 3],
    /// Current slice Z height for the slice plane indicator.
    pub slice_z: f32,
    /// Print volume dimensions for the slice plane quad.
    pub volume_width: f32,
    /// Print volume depth for the slice plane quad.
    pub volume_depth: f32,
    /// Slice preview texture view for the overlay (if available and enabled).
    pub slice_texture_view: Option<Arc<wgpu::TextureView>>,
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
            let volume = microtome_core::PrintVolumeBox::new(
                f64::from(self.volume_width),
                f64::from(self.volume_depth),
                f64::from(self.volume_max[2]),
            );
            renderer.update_slice_plane(device, &volume, self.slice_z);
            if let Some(ref tex_view) = self.slice_texture_view {
                renderer.update_overlay_bind_group(device, tex_view);
            }
            renderer.render_offscreen(
                device,
                queue,
                egui_encoder,
                self.width,
                self.height,
                self.view_proj,
                &self.meshes,
                self.selected_index,
                self.volume_min,
                self.volume_max,
                self.slice_texture_view.is_some(),
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
