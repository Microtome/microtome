//! Layout helpers for the main application panels.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use glam::Vec3;
use microtome_core::{PrintJobConfig, PrintMesh, PrinterConfig, PrinterScene, SliceProgress};

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
    /// Current slice Z height in mm.
    pub slice_z: &'a mut f32,
    /// Currently selected mesh index.
    pub selected_mesh: &'a mut Option<usize>,
    /// Overhang angle in degrees.
    pub overhang_angle_degrees: &'a mut f32,
    /// Current slicing progress (0.0 to 1.0), if a job is running.
    pub slicing_progress: &'a mut Option<f32>,
    /// Receiver for slicing progress updates.
    pub progress_rx: &'a mut Option<mpsc::Receiver<SliceProgress>>,
    /// Path to save ZIP output when the slicing job completes.
    pub export_path: &'a mut Option<PathBuf>,
    /// Cancellation flag for active slicing job.
    pub cancel_flag: &'a mut Option<Arc<AtomicBool>>,
    /// Callback to load STL mesh data onto the GPU.
    pub stl_loaded: &'a mut Option<microtome_core::MeshData>,
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
        && let Some((_path, mesh_data)) = file_dialogs::open_stl_dialog()
    {
        *state.stl_loaded = Some(mesh_data);
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

    // --- Object transform controls ---
    ui.label(format!("Meshes: {}", state.scene.meshes.len()));

    if let Some(idx) = *state.selected_mesh
        && idx < state.scene.meshes.len()
    {
        ui.label(format!("Selected: mesh {idx}"));
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

/// Renders the bottom status/slice bar with Z slider and layer info.
pub fn bottom_bar(ui: &mut egui::Ui, state: &mut AppState<'_>) {
    ui.horizontal(|ui| {
        ui.label("Slice Z:");
        let max_z = state.printer_config.volume.height_mm as f32;
        ui.add(egui::Slider::new(state.slice_z, 0.0..=max_z).suffix(" mm"));

        let layer_height = state.job_config.layer_height_mm();
        if layer_height > 0.0 {
            let layer_num = (*state.slice_z as f64 / layer_height).floor() as u32;
            let total_layers = (max_z as f64 / layer_height).ceil() as u32;
            ui.separator();
            ui.label(format!(
                "Layer {layer_num} / {total_layers}  |  Z = {:.3} mm",
                *state.slice_z
            ));
        }

        if let Some(progress) = *state.slicing_progress {
            ui.separator();
            ui.add(
                egui::ProgressBar::new(progress)
                    .desired_width(120.0)
                    .show_percentage(),
            );
        }
    });
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
                mesh.scale = Vec3::splat(uniform_scale);
            }
            ui.end_row();
        });
}
