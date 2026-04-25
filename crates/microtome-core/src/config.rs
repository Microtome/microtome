//! Printer and print job configuration types.

use serde::{Deserialize, Serialize};

/// Z-axis stepper motor stage configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZStage {
    /// Distance traveled per full revolution of the lead screw (mm).
    pub lead_mm: f64,
    /// Full steps per revolution of the stepper motor.
    pub steps_per_rev: u32,
    /// Microsteps per full step.
    pub microsteps: u32,
}

impl ZStage {
    /// Returns the distance per microstep in millimeters.
    pub fn step_distance_mm(&self) -> f64 {
        self.lead_mm / (self.steps_per_rev as f64 * self.microsteps as f64)
    }

    /// Returns the distance per microstep in microns.
    pub fn step_distance_microns(&self) -> f64 {
        self.step_distance_mm() * 1000.0
    }
}

/// Projector (DLP/LCD) resolution configuration.
///
/// Currently assumes square pixels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projector {
    /// Horizontal resolution in pixels.
    pub x_res_px: u32,
    /// Vertical resolution in pixels.
    pub y_res_px: u32,
}

/// Resin material information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resin {
    /// Manufacturer name.
    pub manufacturer: String,
    /// Product name.
    pub product_name: String,
    /// Product/part number.
    pub product_number: String,
    /// Price per unit (formatted string).
    pub price_per_unit: String,
    /// Volume per unit in milliliters (formatted string).
    pub unit_volume_ml: String,
}

/// Physical dimensions of the print volume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintVolume {
    /// Width of the print volume (mm).
    pub width_mm: f64,
    /// Depth of the print volume (mm).
    pub depth_mm: f64,
    /// Height of the print volume (mm).
    pub height_mm: f64,
}

/// Full printer hardware configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterConfig {
    /// Printer name.
    pub name: String,
    /// Printer description.
    pub description: String,
    /// Last modified timestamp (ms since epoch).
    pub last_modified: u64,
    /// Physical print volume dimensions.
    pub volume: PrintVolume,
    /// Z-axis stage configuration.
    pub z_stage: ZStage,
    /// Projector/display configuration.
    pub projector: Projector,
}

/// Print job settings controlling slicing and exposure parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJobConfig {
    /// Name of this job configuration.
    pub name: String,
    /// Description of this job.
    pub description: String,
    /// The microstep distance when this job was created (µm).
    pub step_distance_microns: f64,
    /// Number of microsteps per layer.
    pub steps_per_layer: u32,
    /// Time to wait for resin to settle after peel (ms).
    pub settle_time_ms: u32,
    /// UV exposure time per layer (ms).
    pub layer_exposure_time_ms: u32,
    /// Time the projector is blank between layers (ms).
    pub blank_time_ms: u32,
    /// Distance to retract for layer peel (mm).
    pub retract_distance_mm: f64,
    /// Z offset applied to objects when added to the scene (mm).
    pub z_offset_mm: f64,
    /// Thickness of the adhesion raft (mm). Must be <= `z_offset_mm`.
    pub raft_thickness_mm: f64,
    /// Distance to grow raft border outward (mm).
    pub raft_outset_mm: f64,
}

impl PrintJobConfig {
    /// Returns the layer height in millimeters.
    pub fn layer_height_mm(&self) -> f64 {
        self.step_distance_microns * self.steps_per_layer as f64 / 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_z_stage() -> ZStage {
        ZStage {
            lead_mm: 8.0,
            steps_per_rev: 200,
            microsteps: 16,
        }
    }

    fn sample_printer_config() -> PrinterConfig {
        PrinterConfig {
            name: "Test Printer".into(),
            description: "A test printer".into(),
            last_modified: 1700000000000,
            volume: PrintVolume {
                width_mm: 120.0,
                depth_mm: 68.0,
                height_mm: 150.0,
            },
            z_stage: sample_z_stage(),
            projector: Projector {
                x_res_px: 2560,
                y_res_px: 1440,
            },
        }
    }

    fn sample_job_config() -> PrintJobConfig {
        PrintJobConfig {
            name: "Default Job".into(),
            description: "Default settings".into(),
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

    #[test]
    fn z_stage_step_distance() {
        let stage = sample_z_stage();
        // 8.0 / (200 * 16) = 8.0 / 3200 = 0.0025 mm = 2.5 microns
        assert!((stage.step_distance_mm() - 0.0025).abs() < f64::EPSILON);
        assert!((stage.step_distance_microns() - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn layer_height_calculation() {
        let job = sample_job_config();
        // 2.5 microns * 20 steps / 1000 = 0.05 mm
        assert!((job.layer_height_mm() - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn config_serde_round_trip_preserves_values() {
        // Single test covering both config types — round-trips and checks a
        // representative field on each so a broken `#[serde]` attribute on
        // either struct surfaces here.
        let printer = sample_printer_config();
        let printer_json = serde_json::to_string_pretty(&printer).unwrap();
        let printer_back: PrinterConfig = serde_json::from_str(&printer_json).unwrap();
        assert_eq!(printer_back.name, printer.name);
        assert_eq!(printer_back.projector.x_res_px, 2560);
        assert!((printer_back.volume.width_mm - 120.0).abs() < f64::EPSILON);

        let job = sample_job_config();
        let job_json = serde_json::to_string(&job).unwrap();
        let job_back: PrintJobConfig = serde_json::from_str(&job_json).unwrap();
        assert_eq!(job_back.name, "Default Job");
        assert_eq!(job_back.steps_per_layer, 20);
        assert!((job_back.raft_outset_mm - 0.5).abs() < f64::EPSILON);
    }
}
