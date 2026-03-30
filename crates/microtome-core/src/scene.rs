//! Print scene management — volume box and mesh collection.

use glam::Vec3;

use crate::config::{PrintVolume, PrinterConfig};
use crate::mesh::{BoundingBox, PrintMesh};

/// The print volume box defining the physical build area.
///
/// Centered on XY at the origin, extending from Z=0 upward.
#[derive(Debug, Clone)]
pub struct PrintVolumeBox {
    /// Width in mm (X axis).
    pub width: f64,
    /// Depth in mm (Y axis).
    pub depth: f64,
    /// Height in mm (Z axis).
    pub height: f64,
}

impl PrintVolumeBox {
    /// Creates a new print volume from dimensions.
    pub fn new(width: f64, depth: f64, height: f64) -> Self {
        Self {
            width,
            depth,
            height,
        }
    }

    /// Creates a print volume from a [`PrintVolume`] config.
    pub fn from_config(volume: &PrintVolume) -> Self {
        Self::new(volume.width_mm, volume.depth_mm, volume.height_mm)
    }

    /// Returns the axis-aligned bounding box of the print volume.
    ///
    /// Centered on XY, Z goes from 0 to height.
    pub fn bounding_box(&self) -> BoundingBox {
        let half_w = self.width as f32 / 2.0;
        let half_d = self.depth as f32 / 2.0;
        BoundingBox {
            min: Vec3::new(-half_w, -half_d, 0.0),
            max: Vec3::new(half_w, half_d, self.height as f32),
        }
    }

    /// Resizes the volume to match the given config.
    pub fn resize(&mut self, volume: &PrintVolume) {
        self.width = volume.width_mm;
        self.depth = volume.depth_mm;
        self.height = volume.height_mm;
    }
}

/// The complete print scene containing the volume and all meshes.
#[derive(Debug, Clone)]
pub struct PrinterScene {
    /// The print volume box.
    pub volume: PrintVolumeBox,
    /// All meshes in the scene.
    pub meshes: Vec<PrintMesh>,
    /// Overhang visualization angle in radians.
    overhang_angle: f64,
}

impl PrinterScene {
    /// Creates a new scene with default 100x100x100mm volume.
    pub fn new() -> Self {
        Self {
            volume: PrintVolumeBox::new(100.0, 100.0, 100.0),
            meshes: Vec::new(),
            overhang_angle: 0.0,
        }
    }

    /// Creates a scene configured from a [`PrinterConfig`].
    pub fn from_config(config: &PrinterConfig) -> Self {
        Self {
            volume: PrintVolumeBox::from_config(&config.volume),
            meshes: Vec::new(),
            overhang_angle: 0.0,
        }
    }

    /// Adds a mesh to the scene.
    pub fn add_mesh(&mut self, mesh: PrintMesh) {
        self.meshes.push(mesh);
    }

    /// Removes a mesh by index. Returns the removed mesh.
    pub fn remove_mesh(&mut self, index: usize) -> PrintMesh {
        self.meshes.remove(index)
    }

    /// Returns the overhang visualization angle in radians.
    pub fn overhang_angle(&self) -> f64 {
        self.overhang_angle
    }

    /// Sets the overhang visualization angle in radians.
    pub fn set_overhang_angle(&mut self, radians: f64) {
        self.overhang_angle = radians % (2.0 * std::f64::consts::PI);
    }

    /// Sets the overhang visualization angle in degrees.
    pub fn set_overhang_angle_degrees(&mut self, degrees: f64) {
        self.set_overhang_angle(degrees.to_radians());
    }

    /// Returns the overhang angle in degrees.
    pub fn overhang_angle_degrees(&self) -> f64 {
        self.overhang_angle.to_degrees()
    }

    /// Returns the cosine of the overhang angle (used in shaders).
    pub fn overhang_cos(&self) -> f64 {
        self.overhang_angle.cos()
    }
}

impl Default for PrinterScene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_volume_dimensions() {
        let scene = PrinterScene::new();
        assert!((scene.volume.width - 100.0).abs() < f64::EPSILON);
        assert!((scene.volume.depth - 100.0).abs() < f64::EPSILON);
        assert!((scene.volume.height - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn volume_bounding_box() {
        let vol = PrintVolumeBox::new(120.0, 68.0, 150.0);
        let bbox = vol.bounding_box();
        let eps = 1e-6;
        assert!((bbox.min.x - (-60.0)).abs() < eps);
        assert!((bbox.min.y - (-34.0)).abs() < eps);
        assert!((bbox.min.z - 0.0).abs() < eps);
        assert!((bbox.max.x - 60.0).abs() < eps);
        assert!((bbox.max.y - 34.0).abs() < eps);
        assert!((bbox.max.z - 150.0).abs() < eps);
    }

    #[test]
    fn from_printer_config() {
        let config = PrinterConfig {
            name: "Test".into(),
            description: "".into(),
            last_modified: 0,
            volume: PrintVolume {
                width_mm: 120.0,
                depth_mm: 68.0,
                height_mm: 150.0,
            },
            z_stage: crate::config::ZStage {
                lead_mm: 8.0,
                steps_per_rev: 200,
                microsteps: 16,
            },
            projector: crate::config::Projector {
                x_res_px: 2560,
                y_res_px: 1440,
            },
        };
        let scene = PrinterScene::from_config(&config);
        assert!((scene.volume.width - 120.0).abs() < f64::EPSILON);
        assert!((scene.volume.height - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn overhang_angle_conversion() {
        let mut scene = PrinterScene::new();
        scene.set_overhang_angle_degrees(45.0);
        assert!((scene.overhang_angle_degrees() - 45.0).abs() < 1e-10);
        assert!((scene.overhang_cos() - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-10);
    }

    #[test]
    fn add_remove_meshes() {
        let mut scene = PrinterScene::new();
        assert_eq!(scene.meshes.len(), 0);

        // Create a minimal mesh for testing
        let mesh_data = crate::mesh::MeshData {
            vertices: vec![],
            indices: vec![],
            bbox: crate::mesh::BoundingBox {
                min: glam::Vec3::ZERO,
                max: glam::Vec3::ONE,
            },
            volume: 1.0,
        };
        scene.add_mesh(crate::mesh::PrintMesh::new(mesh_data.clone()));
        scene.add_mesh(crate::mesh::PrintMesh::new(mesh_data));
        assert_eq!(scene.meshes.len(), 2);

        scene.remove_mesh(0);
        assert_eq!(scene.meshes.len(), 1);
    }
}
