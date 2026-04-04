//! Isosurface viewer application state and eframe integration.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use glam::{IVec3, Vec3};
use wgpu::util::DeviceExt;

use microtome_core::isosurface::{
    Aabb, Cylinder, Difference, IsoMesh, KdTreeNode, OctreeNode, ScalarField,
};

use crate::camera::OrbitCamera;
use crate::renderer::{MeshBuffers, ViewportRenderer};
use crate::viewport::ViewportPaintCallback;

/// Isosurface construction structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structure {
    /// Octree-based dual contouring.
    Octree,
    /// K-d tree based dual contouring.
    KdTree,
}

impl std::fmt::Display for Structure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Octree => write!(f, "Octree"),
            Self::KdTree => write!(f, "KdTree"),
        }
    }
}

/// Result of an asynchronous isosurface build.
struct BuildResult {
    mesh: IsoMesh,
    build_time_ms: f64,
}

/// GPU-ready mesh data stored on the application side.
struct GpuMesh {
    /// Shared mesh buffers for the viewport callback.
    buffers: Arc<MeshBuffers>,
}

/// Isosurface viewer application.
pub struct IsosurfaceApp {
    camera: OrbitCamera,
    gpu_mesh: Option<GpuMesh>,
    triangle_count: usize,
    build_time_ms: f64,
    structure: Structure,
    error_threshold: f32,
    octree_depth: u32,
    show_wireframe: bool,
    needs_rebuild: bool,
    /// Whether settings have changed since the last build.
    stale: bool,
    /// Receiver for async build results.
    build_rx: Option<mpsc::Receiver<BuildResult>>,
    /// Whether a build is currently in progress.
    building: bool,
    /// When the current build started (for elapsed time display).
    build_start: Option<Instant>,
}

impl IsosurfaceApp {
    /// Creates a new isosurface viewer application.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        if let Some(ref wgpu_render_state) = cc.wgpu_render_state {
            let renderer =
                ViewportRenderer::new(&wgpu_render_state.device, wgpu_render_state.target_format);
            wgpu_render_state
                .renderer
                .write()
                .callback_resources
                .insert(renderer);
        }

        Self {
            camera: OrbitCamera::new(),
            gpu_mesh: None,
            triangle_count: 0,
            build_time_ms: 0.0,
            structure: Structure::Octree,
            error_threshold: 1e-2,
            octree_depth: 8,
            show_wireframe: false,
            needs_rebuild: true,
            stale: false,
            build_rx: None,
            building: false,
            build_start: None,
        }
    }

    /// Builds the default SDF scene: cube with a cylindrical hole through it.
    fn build_default_scene() -> Box<dyn ScalarField> {
        Box::new(Difference::new(
            Aabb::new(Vec3::splat(-4.0), Vec3::splat(4.0)),
            Cylinder::new(Vec3::new(0.0, 0.0, 3.0)),
        ))
    }

    /// Builds an isosurface mesh from the scalar field.
    fn build_mesh(
        field: &dyn ScalarField,
        depth: u32,
        structure: Structure,
        threshold: f32,
    ) -> Option<IsoMesh> {
        // Keep a constant world volume of [-16, 16]³ regardless of depth.
        // The C++ uses octSize=16, octDepth=8 giving unitSize=0.25, extent=32.
        let size_code = IVec3::splat(1 << (depth - 1));
        let unit_size = 32.0 / size_code.x as f32;
        let min_code = -size_code / 2;

        let mut octree = OctreeNode::build_with_scalar_field(
            min_code,
            depth,
            field,
            matches!(structure, Structure::KdTree),
            unit_size,
        )?;

        match structure {
            Structure::Octree => {
                OctreeNode::simplify(&mut octree, threshold);
                Some(OctreeNode::extract_mesh(&mut octree, field, unit_size))
            }
            Structure::KdTree => {
                let mut kdtree = KdTreeNode::build_from_octree(
                    &octree,
                    min_code,
                    size_code / 2,
                    field,
                    0,
                    unit_size,
                )?;
                Some(KdTreeNode::extract_mesh(
                    &mut kdtree,
                    field,
                    threshold,
                    unit_size,
                ))
            }
        }
    }

    /// Starts an asynchronous isosurface build on a background thread.
    fn start_rebuild(&mut self) {
        let depth = self.octree_depth;
        let structure = self.structure;
        let threshold = self.error_threshold;

        let (tx, rx) = mpsc::channel();
        self.build_rx = Some(rx);
        self.building = true;
        self.build_start = Some(Instant::now());
        self.needs_rebuild = false;

        let _handle = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let start = Instant::now();
                let field = Self::build_default_scene();
                let mesh_opt = Self::build_mesh(field.as_ref(), depth, structure, threshold);
                let build_time_ms = start.elapsed().as_secs_f64() * 1000.0;

                if let Some(mut mesh) = mesh_opt {
                    mesh.generate_flat_normals();
                    let _ = tx.send(BuildResult {
                        mesh,
                        build_time_ms,
                    });
                }
                // If mesh_opt is None, the channel drops and recv will fail gracefully
            });
    }

    /// Polls for build completion. If ready, uploads the mesh to the GPU.
    fn poll_build(&mut self, device: &wgpu::Device) {
        let rx = match &self.build_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(result) => {
                self.build_time_ms = result.build_time_ms;
                self.triangle_count = result.mesh.triangle_count();

                let mesh_data = result.mesh.to_mesh_data();

                if !mesh_data.vertices.is_empty() && !mesh_data.indices.is_empty() {
                    let vertex_buffer =
                        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("iso_vertex_buffer"),
                            contents: bytemuck::cast_slice(&mesh_data.vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });

                    let index_buffer =
                        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("iso_index_buffer"),
                            contents: bytemuck::cast_slice(&mesh_data.indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });

                    self.gpu_mesh = Some(GpuMesh {
                        buffers: Arc::new(MeshBuffers {
                            vertex_buffer,
                            index_buffer,
                            index_count: mesh_data.indices.len() as u32,
                        }),
                    });
                } else {
                    self.gpu_mesh = None;
                }

                self.building = false;
                self.build_rx = None;
                self.build_start = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                // Still building — nothing to do
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                // Build thread finished without sending (no sign changes found)
                log::info!("Build completed with no sign changes (empty mesh)");
                self.triangle_count = 0;
                self.gpu_mesh = None;
                self.building = false;
                self.build_rx = None;
                self.build_start = None;
            }
        }
    }
}

impl eframe::App for IsosurfaceApp {
    /// Main UI rendering called each frame.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Start async rebuild only when explicitly requested via button.
        if self.needs_rebuild && !self.stale && !self.building {
            self.start_rebuild();
        }

        // Poll for build completion and upload to GPU.
        if self.building {
            if let Some(render_state) = frame.wgpu_render_state() {
                self.poll_build(&render_state.device);
            }
            // Keep repainting while building so we see progress.
            ui.ctx().request_repaint();
        }

        egui::Panel::left("controls")
            .default_size(260.0)
            .show_inside(ui, |ui| {
                ui.heading("Isosurface Viewer");
                ui.separator();

                // Controls are disabled while building.
                let mut changed = false;
                ui.add_enabled_ui(!self.building, |ui| {
                    // Structure selection.
                    egui::ComboBox::from_label("Structure")
                        .selected_text(self.structure.to_string())
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut self.structure, Structure::Octree, "Octree")
                                .changed()
                            {
                                changed = true;
                            }
                            if ui
                                .selectable_value(&mut self.structure, Structure::KdTree, "KdTree")
                                .changed()
                            {
                                changed = true;
                            }
                        });

                    // Error threshold (logarithmic).
                    ui.horizontal(|ui| {
                        ui.label("Error threshold:");
                        let mut log_val = self.error_threshold.log10();
                        if ui
                            .add(egui::Slider::new(&mut log_val, -7.0..=2.0).text("10^"))
                            .changed()
                        {
                            self.error_threshold = 10.0_f32.powf(log_val);
                            changed = true;
                        }
                    });

                    // Octree depth.
                    ui.horizontal(|ui| {
                        ui.label("Octree depth:");
                        if ui
                            .add(egui::DragValue::new(&mut self.octree_depth).range(1..=10))
                            .changed()
                        {
                            changed = true;
                        }
                    });

                    if changed {
                        self.stale = true;
                    }

                    let rebuild_btn = if self.stale && !self.building {
                        egui::Button::new("Rebuild").fill(egui::Color32::from_rgb(180, 80, 30))
                    } else {
                        egui::Button::new("Rebuild")
                    };
                    if ui.add_enabled(!self.building, rebuild_btn).clicked() {
                        self.stale = false;
                        self.needs_rebuild = true;
                    }
                });

                ui.separator();

                // Build status / results.
                if self.building {
                    let elapsed = self
                        .build_start
                        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(format!("Building... {elapsed:.0} ms"));
                    });
                } else {
                    ui.label(format!("Triangles: {}", self.triangle_count));
                    ui.label(format!("Build time: {:.1} ms", self.build_time_ms));
                }

                ui.separator();

                ui.checkbox(&mut self.show_wireframe, "Wireframe");

                ui.separator();
                ui.label("View presets:");

                ui.horizontal(|ui| {
                    if ui.button("Front").clicked() {
                        self.camera.set_view_front();
                    }
                    if ui.button("Right").clicked() {
                        self.camera.set_view_right();
                    }
                    if ui.button("Top").clicked() {
                        self.camera.set_view_top();
                    }
                    if ui.button("Iso").clicked() {
                        self.camera.set_view_isometric();
                    }
                });

                let persp_label = if self.camera.use_perspective {
                    "Perspective"
                } else {
                    "Orthographic"
                };
                if ui.button(persp_label).clicked() {
                    self.camera.use_perspective = !self.camera.use_perspective;
                }
            });

        // Central viewport area — use allocate_painter for correct layer scoping.
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        self.camera.handle_input(&response);

        let rect = response.rect;
        let aspect = rect.width() / rect.height().max(1.0);
        let view = self.camera.view_matrix();
        let proj = self.camera.projection_matrix(aspect);
        let view_proj = proj * view;

        let ppp = ui.ctx().pixels_per_point();
        let width = (rect.width() * ppp) as u32;
        let height = (rect.height() * ppp) as u32;

        let mesh_arc = self.gpu_mesh.as_ref().map(|gm| Arc::clone(&gm.buffers));

        let callback = egui_wgpu::Callback::new_paint_callback(
            rect,
            ViewportPaintCallback {
                view_proj,
                mesh: mesh_arc,
                width,
                height,
                show_wireframe: self.show_wireframe,
            },
        );
        painter.add(callback);

        // Request repaint while dragging for smooth interaction.
        if response.dragged() || response.drag_stopped() {
            ui.ctx().request_repaint();
        }
    }
}
