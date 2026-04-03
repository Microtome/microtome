//! Isosurface viewer application state and eframe integration.

use std::sync::Arc;
use std::time::Instant;

use glam::{IVec3, Vec3};
use wgpu::util::DeviceExt;

use microtome_core::isosurface::{
    Aabb, Cylinder, Difference, Intersection, IsoMesh, KdTreeNode, OctreeNode, ScalarField,
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
    /// Cached iso mesh for wireframe generation on GPU init.
    cached_iso_mesh: Option<IsoMesh>,
    /// Whether the renderer needs updated wireframe data.
    wireframe_dirty: bool,
}

impl Default for IsosurfaceApp {
    fn default() -> Self {
        Self {
            camera: OrbitCamera::new(),
            gpu_mesh: None,
            triangle_count: 0,
            build_time_ms: 0.0,
            structure: Structure::Octree,
            error_threshold: 1e-7,
            octree_depth: 8,
            show_wireframe: false,
            needs_rebuild: true,
            cached_iso_mesh: None,
            wireframe_dirty: false,
        }
    }
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

        Self::default()
    }

    /// Builds the default SDF scene.
    fn build_default_scene() -> Box<dyn ScalarField> {
        // Cylinder - smaller cylinder, intersected with AABB (matches C++ example)
        Box::new(Intersection::new(
            Difference::new(
                Cylinder::new(Vec3::new(0.0, 0.0, 4.0)),
                Cylinder::new(Vec3::new(0.0, 0.0, 3.2)),
            ),
            Aabb::new(Vec3::splat(-4.0), Vec3::splat(4.0)),
        ))
    }

    /// Builds an isosurface mesh from the scalar field.
    fn build_mesh(
        field: &dyn ScalarField,
        depth: u32,
        structure: Structure,
        threshold: f32,
    ) -> Option<IsoMesh> {
        let unit_size = 16.0 / (depth as f32 * depth as f32);
        let size_code = IVec3::splat(1 << (depth - 1));
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

    /// Rebuilds the isosurface mesh and uploads to GPU.
    fn rebuild(&mut self, device: &wgpu::Device) {
        let depth = self.octree_depth;
        let structure = self.structure;
        let threshold = self.error_threshold;

        let start = Instant::now();

        // Build on a thread with a large stack for deep recursion.
        // The field is constructed inside the thread to avoid Send requirements.
        let build_result = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                let field = Self::build_default_scene();
                Self::build_mesh(field.as_ref(), depth, structure, threshold)
            });

        let mesh_opt = match build_result {
            Ok(handle) => match handle.join() {
                Ok(m) => m,
                Err(_) => {
                    log::error!("Isosurface build thread panicked");
                    None
                }
            },
            Err(e) => {
                log::error!("Failed to spawn build thread: {e}");
                None
            }
        };

        self.build_time_ms = start.elapsed().as_secs_f64() * 1000.0;

        if let Some(mut mesh) = mesh_opt {
            mesh.generate_flat_normals();
            self.triangle_count = mesh.triangle_count();

            let mesh_data = mesh.to_mesh_data();

            if !mesh_data.vertices.is_empty() && !mesh_data.indices.is_empty() {
                let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("iso_vertex_buffer"),
                    contents: bytemuck::cast_slice(&mesh_data.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });

                let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
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

                self.cached_iso_mesh = Some(mesh);
                self.wireframe_dirty = true;
            } else {
                self.gpu_mesh = None;
                self.cached_iso_mesh = None;
            }
        } else {
            self.triangle_count = 0;
            self.gpu_mesh = None;
            self.cached_iso_mesh = None;
        }

        self.needs_rebuild = false;
    }
}

impl eframe::App for IsosurfaceApp {
    /// Main UI rendering called each frame.
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        // Get wgpu device for GPU operations.
        if self.needs_rebuild
            && let Some(render_state) = frame.wgpu_render_state()
        {
            self.rebuild(&render_state.device);
        }

        // Update wireframe if needed.
        if self.wireframe_dirty {
            if let Some(ref iso_mesh) = self.cached_iso_mesh
                && let Some(render_state) = frame.wgpu_render_state()
            {
                let mut renderer_guard = render_state.renderer.write();
                if let Some(renderer) = renderer_guard
                    .callback_resources
                    .get_mut::<ViewportRenderer>()
                {
                    renderer.update_wireframe_lines(&render_state.device, iso_mesh);
                }
            }
            self.wireframe_dirty = false;
        }

        egui::Panel::left("controls")
            .default_size(260.0)
            .show_inside(ui, |ui| {
                ui.heading("Isosurface Viewer");
                ui.separator();

                // Structure selection.
                let mut changed = false;
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

                if ui.button("Rebuild").clicked() || changed {
                    self.needs_rebuild = true;
                }

                ui.separator();

                ui.label(format!("Triangles: {}", self.triangle_count));
                ui.label(format!("Build time: {:.1} ms", self.build_time_ms));

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

        // Central viewport area.
        let available = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(available, egui::Sense::click_and_drag());

        self.camera.handle_input(&response);

        let aspect = rect.width() / rect.height().max(1.0);
        let view = self.camera.view_matrix();
        let proj = self.camera.projection_matrix(aspect);
        let view_proj = proj * view;

        let ppp = ui.ctx().pixels_per_point();
        let width = (rect.width() * ppp) as u32;
        let height = (rect.height() * ppp) as u32;

        let mesh_arc = self.gpu_mesh.as_ref().map(|gm| Arc::clone(&gm.buffers));

        let callback = ViewportPaintCallback {
            view_proj,
            mesh: mesh_arc,
            width,
            height,
            show_wireframe: self.show_wireframe,
        };

        ui.painter().add(egui::PaintCallback {
            rect,
            callback: Arc::new(callback),
        });

        // Request repaint while dragging for smooth interaction.
        if response.dragged() || response.drag_stopped() {
            ui.ctx().request_repaint();
        }
    }
}
