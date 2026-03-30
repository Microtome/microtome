//! Main application state and UI layout for Microtome.

use microtome_core::{PrintJobConfig, PrintVolume, PrinterConfig, PrinterScene, Projector, ZStage};

use crate::camera::OrbitCamera;

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
}

impl MicrotomeApp {
    /// Creates a new application with default configurations.
    pub fn new(_cc: &eframe::CreationContext) -> Self {
        let printer_config = default_printer_config();
        let scene = PrinterScene::from_config(&printer_config);
        Self {
            scene,
            printer_config,
            job_config: default_job_config(),
            camera: OrbitCamera::new(),
            slice_z: 0.0,
            selected_mesh: None,
        }
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

                if ui.button("Load STL...").clicked() {
                    log::info!("Load STL requested");
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
            let (response, _painter) =
                ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            self.camera.handle_input(&response);

            // Placeholder text centered in the viewport
            let center = response.rect.center();
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                "3D Viewport",
                egui::FontId::proportional(24.0),
                ui.visuals().text_color(),
            );
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
