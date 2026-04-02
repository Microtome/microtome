//! Editable configuration widgets for printer and job settings.

use microtome_core::{PrintJobConfig, PrinterConfig};

/// Renders editable fields for a [`PrinterConfig`] using drag-value widgets.
///
/// Includes volume dimensions, projector resolution, and Z-stage parameters.
pub fn printer_config_editor(ui: &mut egui::Ui, config: &mut PrinterConfig) {
    ui.label("Printer Configuration");

    egui::Grid::new("printer_config_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut config.name);
            ui.end_row();

            ui.label("Volume W:");
            ui.add(
                egui::DragValue::new(&mut config.volume.width_mm)
                    .speed(0.5)
                    .range(1.0..=1000.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Volume D:");
            ui.add(
                egui::DragValue::new(&mut config.volume.depth_mm)
                    .speed(0.5)
                    .range(1.0..=1000.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Volume H:");
            ui.add(
                egui::DragValue::new(&mut config.volume.height_mm)
                    .speed(0.5)
                    .range(1.0..=1000.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Proj X:");
            ui.add(
                egui::DragValue::new(&mut config.projector.x_res_px)
                    .speed(1.0)
                    .range(1..=8192)
                    .suffix(" px"),
            );
            ui.end_row();

            ui.label("Proj Y:");
            ui.add(
                egui::DragValue::new(&mut config.projector.y_res_px)
                    .speed(1.0)
                    .range(1..=8192)
                    .suffix(" px"),
            );
            ui.end_row();

            ui.label("Lead:");
            ui.add(
                egui::DragValue::new(&mut config.z_stage.lead_mm)
                    .speed(0.1)
                    .range(0.1..=100.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Steps/rev:");
            ui.add(
                egui::DragValue::new(&mut config.z_stage.steps_per_rev)
                    .speed(1.0)
                    .range(1..=1000),
            );
            ui.end_row();

            ui.label("Microsteps:");
            ui.add(
                egui::DragValue::new(&mut config.z_stage.microsteps)
                    .speed(1.0)
                    .range(1..=256),
            );
            ui.end_row();
        });
}

/// Renders editable fields for a [`PrintJobConfig`] using drag-value widgets.
///
/// Shows computed layer height and all exposure/retract parameters.
pub fn job_config_editor(ui: &mut egui::Ui, config: &mut PrintJobConfig) {
    ui.label("Print Job Configuration");

    egui::Grid::new("job_config_grid")
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label("Layer height:");
            ui.label(format!("{:.3} mm", config.layer_height_mm()));
            ui.end_row();

            ui.label("Step dist:");
            ui.add(
                egui::DragValue::new(&mut config.step_distance_microns)
                    .speed(0.1)
                    .range(0.1..=100.0)
                    .suffix(" um"),
            );
            ui.end_row();

            ui.label("Steps/layer:");
            ui.add(
                egui::DragValue::new(&mut config.steps_per_layer)
                    .speed(1.0)
                    .range(1..=1000),
            );
            ui.end_row();

            ui.label("Exposure:");
            ui.add(
                egui::DragValue::new(&mut config.layer_exposure_time_ms)
                    .speed(10.0)
                    .range(1..=60000)
                    .suffix(" ms"),
            );
            ui.end_row();

            ui.label("Settle:");
            ui.add(
                egui::DragValue::new(&mut config.settle_time_ms)
                    .speed(10.0)
                    .range(0..=60000)
                    .suffix(" ms"),
            );
            ui.end_row();

            ui.label("Blank:");
            ui.add(
                egui::DragValue::new(&mut config.blank_time_ms)
                    .speed(10.0)
                    .range(0..=60000)
                    .suffix(" ms"),
            );
            ui.end_row();

            ui.label("Retract:");
            ui.add(
                egui::DragValue::new(&mut config.retract_distance_mm)
                    .speed(0.1)
                    .range(0.0..=50.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Z offset:");
            ui.add(
                egui::DragValue::new(&mut config.z_offset_mm)
                    .speed(0.01)
                    .range(0.0..=10.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Raft thick:");
            ui.add(
                egui::DragValue::new(&mut config.raft_thickness_mm)
                    .speed(0.01)
                    .range(0.0..=10.0)
                    .suffix(" mm"),
            );
            ui.end_row();

            ui.label("Raft outset:");
            ui.add(
                egui::DragValue::new(&mut config.raft_outset_mm)
                    .speed(0.01)
                    .range(0.0..=10.0)
                    .suffix(" mm"),
            );
            ui.end_row();
        });
}
