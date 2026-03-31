//! Main application state and UI layout for Microtome.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use glam::{Mat4, Quat, Vec3};
use microtome_core::{
    GpuContext, MeshData, PrintJobConfig, PrintMesh, PrintVolume, PrinterConfig, PrinterScene,
    Projector, SliceProgress, ZStage, run_slicing_job,
};
use transform_gizmo_egui::math::Transform;
use transform_gizmo_egui::prelude::*;
use wgpu::util::DeviceExt;

use crate::camera::OrbitCamera;
use crate::slice_preview::SlicePreview;
use crate::ui::panels::{self, AppState};
use crate::viewport::ViewportPaintCallback;
use crate::viewport_renderer::{MeshBuffers, ViewportRenderer};

/// GPU-side buffers corresponding to a scene mesh.
struct GpuMesh {
    /// Vertex buffer on the GPU.
    vertex_buffer: wgpu::Buffer,
    /// Index buffer on the GPU.
    index_buffer: wgpu::Buffer,
    /// Number of indices.
    index_count: u32,
}

/// Main application state holding the scene, configuration, and UI state.
pub struct MicrotomeApp {
    /// The 3D print scene with volume and meshes.
    scene: PrinterScene,
    /// Hardware printer configuration.
    printer_config: PrinterConfig,
    /// Print job settings.
    job_config: PrintJobConfig,
    /// Orbit camera for viewport navigation.
    camera: OrbitCamera,
    /// Current slice plane Z height in mm.
    slice_z: f32,
    /// Index of the currently selected mesh, if any.
    selected_mesh: Option<usize>,
    /// GPU buffers for each mesh in the scene.
    gpu_meshes: Vec<GpuMesh>,
    /// Whether the render state was successfully initialized.
    has_render_state: bool,
    /// 2D slice preview panel.
    slice_preview: SlicePreview,
    /// Overhang angle in degrees for visualization.
    overhang_angle_degrees: f32,
    /// Active slicing job progress (0.0 to 1.0).
    slicing_progress: Option<f32>,
    /// Receiver for slicing progress updates from the background job.
    progress_rx: Option<mpsc::Receiver<SliceProgress>>,
    /// Path to save the ZIP output when slicing completes.
    export_path: Option<PathBuf>,
    /// Cancellation flag for active slicing job.
    cancel_flag: Option<Arc<AtomicBool>>,
    /// 3D transform gizmo for interactive manipulation.
    gizmo: Gizmo,
    /// Current gizmo modes (translate, rotate, or scale group).
    gizmo_modes: EnumSet<GizmoMode>,
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
        }
    }

    /// Uploads a loaded mesh's vertex and index data to the GPU.
    fn upload_mesh(&mut self, render_state: &egui_wgpu::RenderState, mesh_data: &MeshData) {
        let vertex_buffer =
            render_state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_vertices"),
                    contents: bytemuck::cast_slice(&mesh_data.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

        let index_buffer =
            render_state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("mesh_indices"),
                    contents: bytemuck::cast_slice(&mesh_data.indices),
                    usage: wgpu::BufferUsages::INDEX,
                });

        self.gpu_meshes.push(GpuMesh {
            vertex_buffer,
            index_buffer,
            index_count: mesh_data.indices.len() as u32,
        });
    }

    /// Builds the model matrix for a [`PrintMesh`] from its position, rotation, and scale.
    fn model_matrix(mesh: &PrintMesh) -> Mat4 {
        let translation = Mat4::from_translation(mesh.position);
        let rotation = Mat4::from_quat(Quat::from_euler(
            glam::EulerRot::XYZ,
            mesh.rotation.x,
            mesh.rotation.y,
            mesh.rotation.z,
        ));
        let scale = Mat4::from_scale(mesh.scale);
        translation * rotation * scale
    }

    /// Builds the list of [`MeshBuffers`] for the current frame.
    fn collect_mesh_buffers(&self) -> Vec<MeshBuffers> {
        self.gpu_meshes
            .iter()
            .zip(self.scene.meshes.iter())
            .map(|(gpu, scene_mesh)| MeshBuffers {
                vertex_buffer: gpu.vertex_buffer.clone(),
                index_buffer: gpu.index_buffer.clone(),
                index_count: gpu.index_count,
                model_matrix: Self::model_matrix(scene_mesh),
            })
            .collect()
    }

    /// Polls the progress receiver for slicing job updates.
    ///
    /// Handles progress fraction updates, job completion (writing ZIP to disk),
    /// and job failure/cancellation.
    fn poll_slicing_progress(&mut self) {
        let should_clear = if let Some(rx) = &self.progress_rx {
            let mut should_clear = false;
            // Drain all available messages
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    SliceProgress::Progress(p) => {
                        self.slicing_progress = Some(p);
                    }
                    SliceProgress::Complete(zip_bytes) => {
                        if let Some(path) = &self.export_path {
                            if let Err(e) = std::fs::write(path, &zip_bytes) {
                                log::error!("Failed to write ZIP: {e}");
                            } else {
                                log::info!("Exported ZIP to: {}", path.display());
                            }
                        }
                        should_clear = true;
                        break;
                    }
                    SliceProgress::Failed(msg) => {
                        log::error!("Slicing job failed: {msg}");
                        should_clear = true;
                        break;
                    }
                }
            }
            should_clear
        } else {
            false
        };

        if should_clear {
            self.slicing_progress = None;
            self.progress_rx = None;
            self.export_path = None;
            self.cancel_flag = None;
        }
    }

    /// Starts a slicing job on a background thread.
    ///
    /// Constructs a fresh [`PrinterScene`] from the current config and meshes,
    /// then spawns the job in a background thread.
    fn start_slicing_job(&mut self) {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));

        // Build a fresh scene for the background thread (PrinterScene is not Clone)
        let mut scene = PrinterScene::from_config(&self.printer_config);
        for mesh in &self.scene.meshes {
            scene.add_mesh(mesh.clone());
        }
        scene.set_overhang_angle_degrees(self.overhang_angle_degrees as f64);

        let printer_config = self.printer_config.clone();
        let job_config = self.job_config.clone();
        let cancel_clone = Arc::clone(&cancel);
        let tx_clone = tx.clone();

        std::thread::spawn(move || {
            if let Err(e) =
                run_slicing_job(&scene, &printer_config, &job_config, tx_clone, cancel_clone)
            {
                let _ = tx.send(SliceProgress::Failed(e.to_string()));
            }
        });

        self.slicing_progress = Some(0.0);
        self.progress_rx = Some(rx);
        self.cancel_flag = Some(cancel);
    }
}

impl eframe::App for MicrotomeApp {
    /// Main UI rendering called each frame.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Poll for slicing job progress
        self.poll_slicing_progress();

        // Track whether the panel requests an export start
        let mut stl_loaded: Option<MeshData> = None;

        egui::Panel::left("controls_panel")
            .default_size(260.0)
            .show_inside(ui, |ui| {
                let mut state = AppState {
                    scene: &mut self.scene,
                    printer_config: &mut self.printer_config,
                    job_config: &mut self.job_config,
                    slice_z: &mut self.slice_z,
                    selected_mesh: &mut self.selected_mesh,
                    overhang_angle_degrees: &mut self.overhang_angle_degrees,
                    slicing_progress: &mut self.slicing_progress,
                    progress_rx: &mut self.progress_rx,
                    export_path: &mut self.export_path,
                    cancel_flag: &mut self.cancel_flag,
                    stl_loaded: &mut stl_loaded,
                };
                panels::controls_panel(ui, &mut state);
            });

        // Handle deferred STL loading (needs render_state which is not available inside panel)
        if let Some(mesh_data) = stl_loaded {
            if let Some(render_state) = _frame.wgpu_render_state() {
                self.upload_mesh(render_state, &mesh_data);
            }
            let mut print_mesh = PrintMesh::new(mesh_data);
            let bbox = &print_mesh.mesh_data.bbox;
            let center = bbox.center();
            print_mesh.position = Vec3::new(
                -center.x,
                -center.y,
                -bbox.min.z + self.job_config.z_offset_mm as f32,
            );
            self.scene.add_mesh(print_mesh);
            self.slice_preview.mark_buffers_dirty();
        }

        // Start slicing job if an export path was just set and no job is running
        if self.export_path.is_some() && self.progress_rx.is_none() {
            self.start_slicing_job();
        }

        egui::Panel::bottom("slice_panel")
            .min_size(40.0)
            .show_inside(ui, |ui| {
                let mut state = AppState {
                    scene: &mut self.scene,
                    printer_config: &mut self.printer_config,
                    job_config: &mut self.job_config,
                    slice_z: &mut self.slice_z,
                    selected_mesh: &mut self.selected_mesh,
                    overhang_angle_degrees: &mut self.overhang_angle_degrees,
                    slicing_progress: &mut self.slicing_progress,
                    progress_rx: &mut self.progress_rx,
                    export_path: &mut self.export_path,
                    cancel_flag: &mut self.cancel_flag,
                    stl_loaded: &mut None,
                };
                panels::bottom_bar(ui, &mut state);
            });

        // Update slice preview if we have a render state
        if let Some(render_state) = _frame.wgpu_render_state() {
            let gpu = GpuContext::from_existing(
                Arc::new(render_state.device.clone()),
                Arc::new(render_state.queue.clone()),
            );
            if self.slice_preview.buffers_dirty() {
                self.slice_preview
                    .update_mesh_buffers(&gpu, &self.scene.meshes);
            }
            if let Err(e) = self.slice_preview.update_slice(
                ui.ctx(),
                &gpu,
                self.slice_z,
                self.printer_config.volume.width_mm as f32,
                self.printer_config.volume.depth_mm as f32,
                self.printer_config.volume.height_mm as f32,
            ) {
                log::error!("Slice preview error: {e}");
            }
        }

        egui::Panel::right("slice_preview_panel")
            .default_size(300.0)
            .show_inside(ui, |ui| {
                ui.heading("Slice Preview");
                ui.separator();
                self.slice_preview.show(ui);
            });

        // Handle keyboard shortcuts: Delete/Backspace removes selected mesh
        let delete_pressed = ui
            .ctx()
            .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace));
        if let Some(idx) = self.selected_mesh
            && delete_pressed
            && idx < self.scene.meshes.len()
        {
            self.scene.remove_mesh(idx);
            self.gpu_meshes.remove(idx);
            self.selected_mesh = None;
            self.slice_preview.mark_buffers_dirty();
        }

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let (response, painter) =
                ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            self.camera.handle_input(&response);

            // Click to select/deselect mesh (cycle through meshes on click)
            if response.clicked() && !self.scene.meshes.is_empty() {
                self.selected_mesh = match self.selected_mesh {
                    Some(idx) if idx + 1 < self.scene.meshes.len() => Some(idx + 1),
                    Some(_) => None,
                    None => Some(0),
                };
            }

            if self.has_render_state {
                let rect = response.rect;
                let aspect = rect.width() / rect.height().max(1.0);
                let view = self.camera.view_matrix();
                let proj = self.camera.projection_matrix(aspect);
                let view_proj = proj * view;

                let mesh_buffers = Arc::new(self.collect_mesh_buffers());

                let ppp = ui.ctx().pixels_per_point();
                let callback = egui_wgpu::Callback::new_paint_callback(
                    rect,
                    ViewportPaintCallback {
                        view_proj,
                        meshes: mesh_buffers,
                        selected_index: self.selected_mesh,
                        width: (rect.width() * ppp) as u32,
                        height: (rect.height() * ppp) as u32,
                    },
                );
                painter.add(callback);

                // 3D transform gizmo for the selected mesh
                if let Some(idx) = self.selected_mesh
                    && idx < self.scene.meshes.len()
                {
                    let mesh = &self.scene.meshes[idx];

                    // Gizmo should be centered on the mesh's world-space bbox center.
                    // The mesh's world center = position + bbox_center * scale.
                    let bbox_center = mesh.mesh_data.bbox.center();
                    let world_center = mesh.position + bbox_center * mesh.scale;

                    let rot_quat = Quat::from_euler(
                        glam::EulerRot::XYZ,
                        mesh.rotation.x,
                        mesh.rotation.y,
                        mesh.rotation.z,
                    );
                    let mint_rot: mint::Quaternion<f64> = mint::Quaternion {
                        v: mint::Vector3 {
                            x: f64::from(rot_quat.x),
                            y: f64::from(rot_quat.y),
                            z: f64::from(rot_quat.z),
                        },
                        s: f64::from(rot_quat.w),
                    };
                    let gizmo_transform = Transform::from_scale_rotation_translation(
                        mint::Vector3 {
                            x: f64::from(mesh.scale.x),
                            y: f64::from(mesh.scale.y),
                            z: f64::from(mesh.scale.z),
                        },
                        mint_rot,
                        mint::Vector3 {
                            x: f64::from(world_center.x),
                            y: f64::from(world_center.y),
                            z: f64::from(world_center.z),
                        },
                    );

                    let view_mint: mint::RowMatrix4<f64> = view.as_dmat4().into();
                    let proj_mint: mint::RowMatrix4<f64> = proj.as_dmat4().into();
                    self.gizmo.update_config(GizmoConfig {
                        view_matrix: view_mint,
                        projection_matrix: proj_mint,
                        viewport: rect,
                        modes: self.gizmo_modes,
                        orientation: GizmoOrientation::Global,
                        ..Default::default()
                    });

                    if let Some((_result, new_transforms)) =
                        self.gizmo.interact(ui, &[gizmo_transform])
                    {
                        let t = &new_transforms[0];
                        let mesh = &mut self.scene.meshes[idx];

                        // Convert gizmo world center back to mesh position offset
                        let new_world_center = Vec3::new(
                            t.translation.x as f32,
                            t.translation.y as f32,
                            t.translation.z as f32,
                        );
                        let new_scale =
                            Vec3::new(t.scale.x as f32, t.scale.y as f32, t.scale.z as f32);
                        mesh.position = new_world_center - bbox_center * new_scale;

                        let q = Quat::from_xyzw(
                            t.rotation.v.x as f32,
                            t.rotation.v.y as f32,
                            t.rotation.v.z as f32,
                            t.rotation.s as f32,
                        );
                        let (rx, ry, rz) = q.to_euler(glam::EulerRot::XYZ);
                        mesh.rotation = Vec3::new(rx, ry, rz);
                        mesh.scale = new_scale;
                        self.slice_preview.mark_buffers_dirty();
                    }
                }
            } else {
                let center = response.rect.center();
                painter.text(
                    center,
                    egui::Align2::CENTER_CENTER,
                    "3D Viewport (no GPU)",
                    egui::FontId::proportional(24.0),
                    ui.visuals().text_color(),
                );
            }
        });
    }
}

/// Returns the default printer configuration.
fn default_printer_config() -> PrinterConfig {
    PrinterConfig {
        name: "Default Printer".into(),
        description: String::new(),
        last_modified: 0,
        volume: PrintVolume {
            width_mm: 120.0,
            depth_mm: 68.0,
            height_mm: 150.0,
        },
        z_stage: ZStage {
            lead_mm: 8.0,
            steps_per_rev: 200,
            microsteps: 16,
        },
        projector: Projector {
            x_res_px: 2560,
            y_res_px: 1440,
        },
    }
}

/// Returns the default print job configuration.
fn default_job_config() -> PrintJobConfig {
    PrintJobConfig {
        name: "Default Job".into(),
        description: String::new(),
        step_distance_microns: 2.5,
        steps_per_layer: 20,
        settle_time_ms: 3000,
        layer_exposure_time_ms: 8000,
        blank_time_ms: 500,
        retract_distance_mm: 6.0,
        z_offset_mm: 0.3,
        raft_thickness_mm: 0.2,
        raft_outset_mm: 0.5,
    }
}
