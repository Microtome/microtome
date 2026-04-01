//! Layout helpers for the main application panels.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use glam::Vec3;
use microtome_core::{PrintJobConfig, PrintMesh, PrinterConfig, PrinterScene};

use super::config_editor;
use super::file_dialogs;

/// Mutable application state passed to panel rendering functions.
///
/// Contains references to all fields that panel UI elements may read or modify.
pub struct AppState<'a> {
    /// The printer scene.
    pub scene: &'a mut PrinterScene,
    /// Hardware printer configuration.
    pub printer_config: &'a mut PrinterConfig,
    /// Print job settings.
    pub job_config: &'a mut PrintJobConfig,
    /// Currently selected mesh index.
    pub selected_mesh: &'a mut Option<usize>,
    /// Overhang angle in degrees.
    pub overhang_angle_degrees: &'a mut f32,
    /// Current slicing progress (0.0 to 1.0), if a job is running.
    pub slicing_progress: &'a mut Option<f32>,
    /// Path to save ZIP output when the slicing job completes.
    pub export_path: &'a mut Option<PathBuf>,
    /// Cancellation flag for active slicing job.
    pub cancel_flag: &'a mut Option<Arc<AtomicBool>>,
    /// Loaded STL: (filename, mesh data) to be processed by the app.
    pub stl_loaded: &'a mut Option<(String, microtome_core::MeshData)>,
}

/// Renders the left control panel with file, config, and mesh controls.
pub fn controls_panel(ui: &mut egui::Ui, state: &mut AppState<'_>) {
    ui.heading("Microtome");
    ui.separator();

    // --- Load STL button ---
    let job_active = state.slicing_progress.is_some();

    if ui
        .add_enabled(!job_active, egui::Button::new("Load STL..."))
        .clicked()
        && let Some((path, mesh_data)) = file_dialogs::open_stl_dialog()
    {
        // Extract just the filename from the full path
        let filename = std::path::Path::new(&path)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or(path);
        *state.stl_loaded = Some((filename, mesh_data));
    }

    ui.separator();

    // --- Printer config editor ---
    egui::CollapsingHeader::new("Printer Config")
        .default_open(false)
        .show(ui, |ui| {
            config_editor::printer_config_editor(ui, state.printer_config);
        });

    ui.separator();

    // --- Job config editor ---
    egui::CollapsingHeader::new("Job Config")
        .default_open(false)
        .show(ui, |ui| {
            config_editor::job_config_editor(ui, state.job_config);
        });

    ui.separator();

    // --- Overhang angle slider ---
    ui.label("Overhang Visualization");
    ui.add(
        egui::Slider::new(state.overhang_angle_degrees, 0.0..=90.0)
            .suffix("°")
            .text("Angle"),
    );
    state
        .scene
        .set_overhang_angle_degrees(*state.overhang_angle_degrees as f64);

    ui.separator();

    // --- Object list ---
    ui.label(format!("Objects ({})", state.scene.meshes.len()));

    for i in 0..state.scene.meshes.len() {
        let is_selected = *state.selected_mesh == Some(i);
        let label = &state.scene.meshes[i].name;
        if ui.selectable_label(is_selected, label).clicked() {
            *state.selected_mesh = if is_selected { None } else { Some(i) };
        }
    }

    if let Some(idx) = *state.selected_mesh
        && idx < state.scene.meshes.len()
    {
        ui.separator();
        mesh_transform_editor(ui, &mut state.scene.meshes[idx]);
    }

    ui.separator();

    // --- Export button / progress ---
    if job_active {
        if let Some(progress) = *state.slicing_progress {
            ui.label("Exporting...");
            ui.add(egui::ProgressBar::new(progress).show_percentage());
            if ui.button("Cancel").clicked()
                && let Some(flag) = state.cancel_flag.as_ref()
            {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    } else if ui.button("Export to ZIP").clicked()
        && let Some(path) = file_dialogs::export_zip_dialog()
    {
        *state.export_path = Some(path);
    }
}

/// Renders position, rotation, and scale controls for a single mesh.
fn mesh_transform_editor(ui: &mut egui::Ui, mesh: &mut PrintMesh) {
    ui.label("Transform");
    egui::Grid::new("mesh_transform_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Pos X:");
            ui.add(
                egui::DragValue::new(&mut mesh.position.x)
                    .speed(0.5)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Pos Y:");
            ui.add(
                egui::DragValue::new(&mut mesh.position.y)
                    .speed(0.5)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Pos Z:");
            ui.add(
                egui::DragValue::new(&mut mesh.position.z)
                    .speed(0.5)
                    .suffix(" mm"),
            );
            ui.end_row();

            // Rotation controls (display in degrees, store in radians)
            let mut rot_x_deg = mesh.rotation.x.to_degrees();
            let mut rot_y_deg = mesh.rotation.y.to_degrees();
            let mut rot_z_deg = mesh.rotation.z.to_degrees();

            ui.label("Rot X:");
            if ui
                .add(egui::DragValue::new(&mut rot_x_deg).speed(1.0).suffix("°"))
                .changed()
            {
                mesh.rotation.x = rot_x_deg.to_radians();
            }
            ui.end_row();

            ui.label("Rot Y:");
            if ui
                .add(egui::DragValue::new(&mut rot_y_deg).speed(1.0).suffix("°"))
                .changed()
            {
                mesh.rotation.y = rot_y_deg.to_radians();
            }
            ui.end_row();

            ui.label("Rot Z:");
            if ui
                .add(egui::DragValue::new(&mut rot_z_deg).speed(1.0).suffix("°"))
                .changed()
            {
                mesh.rotation.z = rot_z_deg.to_radians();
            }
            ui.end_row();

            ui.label("Scale:");
            let mut uniform_scale = mesh.scale.x;
            if ui
                .add(
                    egui::DragValue::new(&mut uniform_scale)
                        .speed(0.01)
                        .range(0.01..=100.0),
                )
                .changed()
            {
                // Keep the bbox center stationary when scaling.
                // world_center = position + rotation * (bbox_center * scale)
                let bbox_center = mesh.mesh_data.bbox.center();
                let rot = glam::Quat::from_euler(
                    glam::EulerRot::XYZ,
                    mesh.rotation.x,
                    mesh.rotation.y,
                    mesh.rotation.z,
                );
                let old_center = mesh.position + rot * (bbox_center * mesh.scale);
                let new_scale = Vec3::splat(uniform_scale);
                mesh.position = old_center - rot * (bbox_center * new_scale);
                mesh.scale = new_scale;
            }
            ui.end_row();
        });
}
