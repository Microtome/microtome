//! Application state structs and initialization.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use microtome_core::{
    PrintJobConfig, PrintVolume, PrinterConfig, PrinterScene, Projector, SliceProgress,
};
use transform_gizmo_egui::prelude::*;

use crate::camera::OrbitCamera;
use crate::slice_preview::SlicePreview;
use crate::viewport_renderer::ViewportRenderer;

use super::defaults::{default_job_config, default_printer_config};

/// GPU-side buffers corresponding to a scene mesh.
pub(super) struct GpuMesh {
    /// Vertex buffer on the GPU.
    pub(super) vertex_buffer: wgpu::Buffer,
    /// Index buffer on the GPU.
    pub(super) index_buffer: wgpu::Buffer,
    /// Number of indices.
    pub(super) index_count: u32,
}

/// Main application state holding the scene, configuration, and UI state.
pub struct MicrotomeApp {
    /// The 3D print scene with volume and meshes.
    pub(super) scene: PrinterScene,
    /// Hardware printer configuration.
    pub(super) printer_config: PrinterConfig,
    /// Print job settings.
    pub(super) job_config: PrintJobConfig,
    /// Orbit camera for viewport navigation.
    pub(super) camera: OrbitCamera,
    /// Current slice plane Z height in mm.
    pub(super) slice_z: f32,
    /// Index of the currently selected mesh, if any.
    pub(super) selected_mesh: Option<usize>,
    /// GPU buffers for each mesh in the scene.
    pub(super) gpu_meshes: Vec<GpuMesh>,
    /// Whether the render state was successfully initialized.
    pub(super) has_render_state: bool,
    /// 2D slice preview panel.
    pub(super) slice_preview: SlicePreview,
    /// Overhang angle in degrees for visualization.
    pub(super) overhang_angle_degrees: f32,
    /// Active slicing job progress (0.0 to 1.0).
    pub(super) slicing_progress: Option<f32>,
    /// Receiver for slicing progress updates from the background job.
    pub(super) progress_rx: Option<mpsc::Receiver<SliceProgress>>,
    /// Path to save the ZIP output when slicing completes.
    pub(super) export_path: Option<PathBuf>,
    /// Cancellation flag for active slicing job.
    pub(super) cancel_flag: Option<Arc<AtomicBool>>,
    /// 3D transform gizmo for interactive manipulation.
    pub(super) gizmo: Gizmo,
    /// Current gizmo modes (translate, rotate, or scale group).
    pub(super) gizmo_modes: EnumSet<GizmoMode>,
    /// Whether to show the slice overlay in the 3D viewport.
    pub(super) show_slice_overlay: bool,
    /// Snapshot of the volume config from last frame (for change detection).
    pub(super) prev_volume: PrintVolume,
    /// Snapshot of the projector config from last frame (for change detection).
    pub(super) prev_projector: Projector,
}

impl MicrotomeApp {
    /// Creates a new application with default configurations.
    ///
    /// Initializes the [`ViewportRenderer`] and stores it in egui-wgpu's
    /// callback resources when a wgpu render state is available.
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let printer_config = default_printer_config();
        let scene = PrinterScene::from_config(&printer_config);
        let mut has_render_state = false;

        if let Some(render_state) = &cc.wgpu_render_state {
            let mut renderer =
                ViewportRenderer::new(&render_state.device, render_state.target_format);
            renderer.update_volume_lines(&render_state.device, &scene.volume);
            render_state
                .renderer
                .write()
                .callback_resources
                .insert(renderer);
            has_render_state = true;
        }

        // Use half the projector resolution for responsive preview
        let preview_width = printer_config.projector.x_res_px / 2;
        let preview_height = printer_config.projector.y_res_px / 2;

        let prev_volume = printer_config.volume.clone();
        let prev_projector = printer_config.projector.clone();

        Self {
            scene,
            printer_config,
            job_config: default_job_config(),
            camera: OrbitCamera::new(),
            slice_z: 0.0,
            selected_mesh: None,
            gpu_meshes: Vec::new(),
            has_render_state,
            slice_preview: SlicePreview::new(preview_width, preview_height),
            overhang_angle_degrees: 0.0,
            slicing_progress: None,
            progress_rx: None,
            export_path: None,
            cancel_flag: None,
            gizmo: Gizmo::default(),
            gizmo_modes: GizmoMode::all(),
            show_slice_overlay: true,
            prev_volume,
            prev_projector,
        }
    }
}
