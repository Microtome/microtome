//! Isosurface viewer application state and eframe integration.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Instant;

use glam::{IVec3, Vec3};
use wgpu::util::DeviceExt;

use microtome_core::isosurface::{
    Aabb, Cylinder, Difference, IsoMesh, KdTreeNode, KdTreeV2Node, OctreeNode, PositionCode,
    ScalarField, ScannedMeshField, SignMode,
};
use microtome_core::{MeshData, MicrotomeError};

use crate::camera::OrbitCamera;
use crate::cli::{SignModeArg, StructureArg, default_output_path};
use crate::renderer::{MeshBuffers, ViewportRenderer};
use crate::viewport::ViewportPaintCallback;

/// Isosurface construction structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Structure {
    /// Octree-based dual contouring.
    Octree,
    /// K-d tree based dual contouring (C++ port with bug-compatible abs).
    KdTree,
    /// K-d tree v2 with corrected binary search (paper algorithm).
    KdTreeV2,
}

impl std::fmt::Display for Structure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Octree => write!(f, "Octree"),
            Self::KdTree => write!(f, "KdTree"),
            Self::KdTreeV2 => write!(f, "KdTree v2"),
        }
    }
}

/// Result of an asynchronous isosurface build.
struct BuildResult {
    mesh: IsoMesh,
    build_time_ms: f64,
    /// `true` if the rebuild was triggered for a user-loaded mesh (so the
    /// UI can distinguish original vs. remeshed state).
    source_was_loaded: bool,
}

/// GPU-ready mesh data stored on the application side.
struct GpuMesh {
    /// Shared mesh buffers for the viewport callback.
    buffers: Arc<MeshBuffers>,
}

/// Scalar-field source that drives rebuilds.
enum FieldSource {
    /// The hardcoded "cube with cylindrical hole" default SDF scene.
    DefaultScene,
    /// A user-loaded OBJ/STL file. Kept behind an `Arc` so rebuilds can
    /// snapshot it cheaply into the background thread.
    LoadedMesh {
        /// Display name (file basename).
        name: String,
        /// Original on-disk path. Embedded into the "copy CLI" command so
        /// the headless renderer can re-open the same file.
        path: PathBuf,
        /// Original mesh geometry, also displayed before the first remesh.
        mesh: Arc<MeshData>,
    },
}

/// Source snapshot moved into the rebuild background thread.
enum FieldSourceSnapshot {
    DefaultScene,
    LoadedMesh(Arc<MeshData>),
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
    /// How `ScannedMeshField::from_mesh` computes corner signs. Switch
    /// to `FloodFill` to A/B against `Gwn` and isolate whether artefacts
    /// trace back to the winding-number path.
    sign_mode: SignMode,
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
    /// Scalar-field source used by `Rebuild`.
    field_source: FieldSource,
    /// `false` = showing the loaded file's original geometry (or the default
    /// scene); `true` = showing the result of remeshing the loaded file.
    displaying_remesh: bool,
    /// Last mesh-loading error, shown in the side panel.
    load_error: Option<String>,
    /// Deferred flag: open the file dialog on the next frame (handled outside
    /// the side-panel closure so it can borrow the wgpu device).
    want_load_dialog: bool,
    /// Last "copy CLI for view" output, shown in the side panel so I (and
    /// the headless render automation) can read off the exact command that
    /// reproduces the current view.
    last_cli_view: Option<String>,
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
            sign_mode: SignMode::Gwn,
            show_wireframe: false,
            needs_rebuild: true,
            stale: false,
            build_rx: None,
            building: false,
            build_start: None,
            field_source: FieldSource::DefaultScene,
            displaying_remesh: false,
            load_error: None,
            want_load_dialog: false,
            last_cli_view: None,
        }
    }

    /// Opens a file picker and loads an OBJ or STL mesh synchronously.
    fn load_mesh_from_dialog(&mut self, device: &wgpu::Device) {
        let picked = rfd::FileDialog::new()
            .add_filter("Mesh files", &["obj", "stl"])
            .add_filter("Wavefront OBJ", &["obj"])
            .add_filter("STL", &["stl"])
            .pick_file();
        if let Some(path) = picked {
            self.load_mesh_from_path(device, &path);
        }
    }

    /// Loads a mesh file from the given path, uploads it to the GPU, and
    /// frames the camera around its bounding box.
    fn load_mesh_from_path(&mut self, device: &wgpu::Device, path: &Path) {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();

        let result: Result<MeshData, MicrotomeError> = match ext.as_str() {
            "obj" => MeshData::from_obj(path),
            "stl" => match std::fs::File::open(path) {
                Ok(file) => {
                    let mut reader = std::io::BufReader::new(file);
                    MeshData::from_stl(&mut reader)
                }
                Err(e) => Err(MicrotomeError::Io(e)),
            },
            other => {
                self.load_error = Some(format!("Unsupported file extension: .{other}"));
                return;
            }
        };

        let mesh_data = match result {
            Ok(m) => m,
            Err(e) => {
                log::error!("Mesh load failed for {}: {e}", path.display());
                self.load_error = Some(format!("Failed to load {}: {e}", display_name(path)));
                return;
            }
        };

        if mesh_data.indices.is_empty() || mesh_data.vertices.is_empty() {
            self.load_error = Some("Loaded mesh has no geometry".to_string());
            return;
        }

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("loaded_vertex_buffer"),
            contents: bytemuck::cast_slice(&mesh_data.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("loaded_index_buffer"),
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
        self.triangle_count = mesh_data.indices.len() / 3;
        self.build_time_ms = 0.0;
        self.load_error = None;
        self.camera
            .frame_bbox(mesh_data.bbox.min, mesh_data.bbox.max);

        let name = display_name(path);
        self.field_source = FieldSource::LoadedMesh {
            name,
            path: path.to_path_buf(),
            mesh: Arc::new(mesh_data),
        };
        self.displaying_remesh = false;
        // User will see the original mesh; they click Rebuild to remesh.
        // Flag `stale` so the Rebuild button is highlighted as actionable.
        self.stale = true;
    }

    /// Builds the default SDF scene: cube with a cylindrical hole through it.
    pub(crate) fn build_default_scene() -> Box<dyn ScalarField> {
        Box::new(Difference::new(
            Aabb::new(Vec3::splat(-4.0), Vec3::splat(4.0)),
            Cylinder::new(Vec3::new(0.0, 0.0, 3.0)),
        ))
    }

    /// Bounds for the default scene (`[-16, 16]³` world-space).
    pub(crate) fn default_scene_bounds(depth: u32) -> (PositionCode, f32) {
        let size_code = 1_i32 << (depth - 1);
        let unit_size = 32.0 / size_code as f32;
        let min_code = PositionCode::splat(-size_code / 2);
        (min_code, unit_size)
    }

    /// Bounds for a loaded mesh: fits the bbox with 10% padding on all sides.
    pub(crate) fn loaded_mesh_bounds(
        bbox_min: Vec3,
        bbox_max: Vec3,
        depth: u32,
    ) -> (PositionCode, f32) {
        let size_code = 1_i32 << (depth - 1);
        let raw_extent = (bbox_max - bbox_min).max_element().max(1e-6);
        let extent = raw_extent * 1.1;
        let unit_size = extent / size_code as f32;
        let center = (bbox_min + bbox_max) * 0.5;
        let world_min = center - Vec3::splat(extent * 0.5);
        let min_code = PositionCode::new(
            (world_min.x / unit_size).floor() as i32,
            (world_min.y / unit_size).floor() as i32,
            (world_min.z / unit_size).floor() as i32,
        );
        (min_code, unit_size)
    }

    /// Builds an isosurface mesh from the scalar field over the given grid.
    pub(crate) fn build_mesh(
        field: &dyn ScalarField,
        min_code: PositionCode,
        depth: u32,
        structure: Structure,
        threshold: f32,
        unit_size: f32,
    ) -> Option<IsoMesh> {
        let size_code = IVec3::splat(1 << (depth - 1));

        let as_mipmap = matches!(structure, Structure::KdTree | Structure::KdTreeV2);
        let mut octree =
            OctreeNode::build_with_scalar_field(min_code, depth, field, as_mipmap, unit_size)?;

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
            Structure::KdTreeV2 => {
                let mut kdtree = KdTreeV2Node::build_from_octree(
                    &octree,
                    min_code,
                    size_code / 2,
                    field,
                    0,
                    unit_size,
                )?;
                Some(KdTreeV2Node::extract_mesh(
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
        let sign_mode = self.sign_mode;

        let snapshot = match &self.field_source {
            FieldSource::DefaultScene => FieldSourceSnapshot::DefaultScene,
            FieldSource::LoadedMesh { mesh, .. } => {
                FieldSourceSnapshot::LoadedMesh(Arc::clone(mesh))
            }
        };
        let source_was_loaded = matches!(snapshot, FieldSourceSnapshot::LoadedMesh(_));

        let (tx, rx) = mpsc::channel();
        self.build_rx = Some(rx);
        self.building = true;
        self.build_start = Some(Instant::now());
        self.needs_rebuild = false;

        let _handle = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let start = Instant::now();

                let mesh_opt = match snapshot {
                    FieldSourceSnapshot::DefaultScene => {
                        let (min_code, unit_size) = Self::default_scene_bounds(depth);
                        let field = Self::build_default_scene();
                        Self::build_mesh(
                            field.as_ref(),
                            min_code,
                            depth,
                            structure,
                            threshold,
                            unit_size,
                        )
                    }
                    FieldSourceSnapshot::LoadedMesh(mesh) => {
                        let (min_code, unit_size) =
                            Self::loaded_mesh_bounds(mesh.bbox.min, mesh.bbox.max, depth);
                        let size_code = 1_i32 << (depth - 1);
                        let field = ScannedMeshField::from_mesh(
                            mesh.as_ref(),
                            min_code,
                            size_code,
                            unit_size,
                            sign_mode,
                        );
                        Self::build_mesh(&field, min_code, depth, structure, threshold, unit_size)
                    }
                };

                let build_time_ms = start.elapsed().as_secs_f64() * 1000.0;

                if let Some(mut mesh) = mesh_opt {
                    mesh.generate_flat_normals();
                    let _ = tx.send(BuildResult {
                        mesh,
                        build_time_ms,
                        source_was_loaded,
                    });
                }
                // If mesh_opt is None, the channel drops and recv will fail gracefully
            });
    }

    /// Formats a `cargo run` command line that reproduces the current
    /// view in headless mode (writes a PNG and exits). Includes the loaded
    /// mesh path when applicable, all build parameters, the camera state,
    /// and a fresh `/tmp/microtome-view-*.png` output path.
    fn format_view_cli(&self) -> String {
        let mut parts: Vec<String> = vec![
            "cargo".into(),
            "run".into(),
            "--release".into(),
            "-q".into(),
            "-p".into(),
            "isosurface-viewer".into(),
            "--".into(),
            "--headless".into(),
        ];
        if let FieldSource::LoadedMesh { path, .. } = &self.field_source {
            parts.push("--mesh".into());
            parts.push(quote_path(path));
            if !self.displaying_remesh {
                parts.push("--original".into());
            }
        }
        parts.push("--depth".into());
        parts.push(self.octree_depth.to_string());
        parts.push("--threshold".into());
        parts.push(format!("{:.6}", self.error_threshold));
        parts.push("--structure".into());
        parts.push(StructureArg::from(self.structure).as_str().into());
        parts.push("--sign-mode".into());
        parts.push(SignModeArg::from(self.sign_mode).as_str().into());
        parts.push("--theta".into());
        parts.push(format!("{:.6}", self.camera.theta()));
        parts.push("--phi".into());
        parts.push(format!("{:.6}", self.camera.phi()));
        parts.push("--radius".into());
        parts.push(format!("{:.6}", self.camera.radius()));
        let t = self.camera.target();
        parts.push("--target".into());
        parts.push(format!("{:.6},{:.6},{:.6}", t.x, t.y, t.z));
        if self.show_wireframe {
            parts.push("--wireframe".into());
        }
        parts.push("--output".into());
        parts.push(quote_path(&default_output_path()));
        parts.join(" ")
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
                self.displaying_remesh = result.source_was_loaded;

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
                            if ui
                                .selectable_value(
                                    &mut self.structure,
                                    Structure::KdTreeV2,
                                    "KdTree v2",
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        });

                    // Sign-generation mode for `ScannedMeshField`. Only
                    // affects rebuilds of a loaded mesh; the default
                    // scene uses an analytic SDF and skips this path.
                    egui::ComboBox::from_label("Sign mode")
                        .selected_text(match self.sign_mode {
                            SignMode::Gwn => "GWN (BVH)",
                            SignMode::FloodFill => "Flood fill",
                        })
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_value(&mut self.sign_mode, SignMode::Gwn, "GWN (BVH)")
                                .changed()
                            {
                                changed = true;
                            }
                            if ui
                                .selectable_value(
                                    &mut self.sign_mode,
                                    SignMode::FloodFill,
                                    "Flood fill",
                                )
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

                // File loading controls.
                ui.label("Load mesh from file:");
                if ui
                    .add_enabled(!self.building, egui::Button::new("Load OBJ / STL…"))
                    .clicked()
                {
                    self.want_load_dialog = true;
                }

                let loaded = matches!(self.field_source, FieldSource::LoadedMesh { .. });
                if ui
                    .add_enabled(
                        loaded && !self.building,
                        egui::Button::new("Use Default Scene"),
                    )
                    .clicked()
                {
                    self.field_source = FieldSource::DefaultScene;
                    self.displaying_remesh = false;
                    self.load_error = None;
                    self.camera.frame_bbox(Vec3::splat(-8.0), Vec3::splat(8.0));
                    self.stale = false;
                    self.needs_rebuild = true;
                }

                if let Some(err) = &self.load_error {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
                }

                ui.separator();

                // Build / mesh status.
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
                    match &self.field_source {
                        FieldSource::DefaultScene => {
                            ui.label("Source: Default scene");
                            ui.label(format!("Triangles: {}", self.triangle_count));
                            ui.label(format!("Build time: {:.1} ms", self.build_time_ms));
                        }
                        FieldSource::LoadedMesh { name, .. } => {
                            let suffix = if self.displaying_remesh {
                                "(remeshed)"
                            } else {
                                "(original)"
                            };
                            ui.label(format!("Source: {name} {suffix}"));
                            ui.label(format!("Triangles: {}", self.triangle_count));
                            if self.displaying_remesh {
                                ui.label(format!("Build time: {:.1} ms", self.build_time_ms));
                            }
                        }
                    }
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

                ui.separator();

                if ui.button("Copy CLI for view").clicked() {
                    let cli = self.format_view_cli();
                    ui.ctx().copy_text(cli.clone());
                    self.last_cli_view = Some(cli);
                }
                if let Some(cli) = &self.last_cli_view {
                    ui.add(
                        egui::TextEdit::multiline(&mut cli.as_str())
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(4)
                            .desired_width(f32::INFINITY),
                    );
                }
            });

        // Handle deferred file-dialog request now that the panel closure has
        // released the mutable borrow of `self`.
        if self.want_load_dialog {
            self.want_load_dialog = false;
            if let Some(render_state) = frame.wgpu_render_state() {
                self.load_mesh_from_dialog(&render_state.device);
            }
        }

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

/// Returns the file name for display, falling back to the full path.
fn display_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

/// Single-quotes a path for the generated CLI command iff it contains a
/// shell-significant character. Keeps the common no-spaces case readable.
fn quote_path(path: &Path) -> String {
    let s = path.display().to_string();
    let needs_quotes = s
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '$' | '`' | '\\' | '*' | '?' | '['));
    if needs_quotes {
        let escaped = s.replace('\'', "'\\''");
        format!("'{escaped}'")
    } else {
        s
    }
}
