//! Default printer and job configuration values.

use microtome_core::{PrintJobConfig, PrintVolume, PrinterConfig, Projector, ZStage};

/// Returns the default printer configuration.
pub(super) fn default_printer_config() -> PrinterConfig {
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
pub(super) fn default_job_config() -> PrintJobConfig {
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
