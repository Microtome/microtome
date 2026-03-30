//! Main application state and UI layout for Microtome.

use std::sync::Arc;

use glam::{Mat4, Quat};
use microtome_core::{
    MeshData, PrintJobConfig, PrintMesh, PrintVolume, PrinterConfig, PrinterScene, Projector,
    ZStage,
};
use wgpu::util::DeviceExt;

use crate::camera::OrbitCamera;
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

        Self {
            scene,
            printer_config,
            job_config: default_job_config(),
            camera: OrbitCamera::new(),
            slice_z: 0.0,
            selected_mesh: None,
            gpu_meshes: Vec::new(),
            has_render_state,
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
}

impl eframe::App for MicrotomeApp {
    /// Main UI rendering called each frame.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("controls_panel")
            .default_size(260.0)
            .show_inside(ui, |ui| {
                ui.heading("Microtome");
                ui.separator();

                if ui.button("Load STL...").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("STL files", &["stl", "STL"])
                        .pick_file()
                {
                    match std::fs::read(&path) {
                        Ok(data) => match MeshData::from_stl_bytes(&data) {
                            Ok(mesh_data) => {
                                if let Some(render_state) = _frame.wgpu_render_state() {
                                    self.upload_mesh(render_state, &mesh_data);
                                }
                                self.scene.add_mesh(PrintMesh::new(mesh_data));
                                log::info!("Loaded STL: {}", path.display());
                            }
                            Err(e) => log::error!("Failed to parse STL: {e}"),
                        },
                        Err(e) => log::error!("Failed to read file: {e}"),
                    }
                }

                ui.separator();
                ui.label("Printer Configuration");
                ui.label(format!("  Name: {}", self.printer_config.name));
                ui.label(format!(
                    "  Volume: {:.0} x {:.0} x {:.0} mm",
                    self.printer_config.volume.width_mm,
                    self.printer_config.volume.depth_mm,
                    self.printer_config.volume.height_mm,
                ));
                ui.label(format!(
                    "  Projector: {}x{}",
                    self.printer_config.projector.x_res_px, self.printer_config.projector.y_res_px,
                ));

                ui.separator();
                ui.label("Print Job");
                ui.label(format!(
                    "  Layer height: {:.3} mm",
                    self.job_config.layer_height_mm(),
                ));
                ui.label(format!(
                    "  Exposure: {} ms",
                    self.job_config.layer_exposure_time_ms,
                ));

                ui.separator();
                ui.label(format!("  Meshes: {}", self.scene.meshes.len()));
                if let Some(idx) = self.selected_mesh {
                    ui.label(format!("  Selected: mesh {idx}"));
                }
            });

        egui::Panel::bottom("slice_panel")
            .min_size(40.0)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Slice Z:");
                    let max_z = self.printer_config.volume.height_mm as f32;
                    ui.add(egui::Slider::new(&mut self.slice_z, 0.0..=max_z).suffix(" mm"));
                });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let (response, painter) =
                ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            self.camera.handle_input(&response);

            if self.has_render_state {
                let rect = response.rect;
                let aspect = rect.width() / rect.height().max(1.0);
                let view = self.camera.view_matrix();
                let proj = self.camera.projection_matrix(aspect);
                let view_proj = proj * view;

                let mesh_buffers = Arc::new(self.collect_mesh_buffers());

                let callback = egui_wgpu::Callback::new_paint_callback(
                    rect,
                    ViewportPaintCallback {
                        view_proj,
                        meshes: mesh_buffers,
                        selected_index: self.selected_mesh,
                    },
                );
                painter.add(callback);
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
