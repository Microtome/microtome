//! Volume data scalar field loaded from an image stack.
//!
//! Implements [`ScalarField`] for volumetric datasets stored as a sequence
//! of 2D image slices (e.g., CT/MRI scans or 3D texture data).

use std::path::Path;

use glam::{IVec3, Vec3};

use crate::error::{MicrotomeError, Result};

use super::indicators::{PositionCode, pos_to_code};
use super::scalar_field::ScalarField;

/// A scalar field backed by volumetric image data.
///
/// The data is stored as a flat `Vec<u8>` with dimensions `width × height × levels`.
/// Values are converted to signed distances by subtracting the isovalue.
pub struct VolumeData {
    /// Raw voxel data (one byte per voxel).
    data: Vec<u8>,
    /// Width of each slice (X dimension).
    width: u32,
    /// Height of each slice (Y dimension).
    height: u32,
    /// Number of slices (Z dimension).
    levels: u32,
    /// Isovalue threshold (voxels below this are inside).
    isovalue: f32,
    /// Minimum grid coordinate.
    min_code: PositionCode,
    /// Scale factor from grid coordinates to voxel indices.
    scale: PositionCode,
    /// Voxel grid cell size (for coordinate conversions).
    unit_size: f32,
}

impl VolumeData {
    /// Creates a new volume data from raw voxel bytes.
    ///
    /// # Arguments
    /// * `data` — Raw voxel data, length must be `width * height * levels`
    /// * `width` — Width of each slice
    /// * `height` — Height of each slice
    /// * `levels` — Number of slices
    /// * `isovalue` — Threshold for inside/outside (default in C++ was 31.5)
    /// * `min_code` — Minimum grid coordinate
    /// * `scale` — Scale factor from grid to voxel coordinates
    /// * `unit_size` — Voxel grid cell size
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data: Vec<u8>,
        width: u32,
        height: u32,
        levels: u32,
        isovalue: f32,
        min_code: PositionCode,
        scale: PositionCode,
        unit_size: f32,
    ) -> Self {
        Self {
            data,
            width,
            height,
            levels,
            isovalue,
            min_code,
            scale,
            unit_size,
        }
    }

    /// Loads volume data from a directory of image files.
    ///
    /// Reads all images matching the pattern `001.png`, `002.png`, etc.
    /// (or `.tif`, `.jpg`, etc.) from the given directory path.
    /// Images are loaded in sorted order as Z slices.
    pub fn from_image_stack(
        dir: &Path,
        levels: u32,
        isovalue: f32,
        min_code: PositionCode,
        scale: PositionCode,
        unit_size: f32,
    ) -> Result<Self> {
        let mut data = Vec::new();
        let mut width = 0u32;
        let mut height = 0u32;

        for i in 1..=levels {
            // Try common image extensions
            let name = format!("{:03}", i);
            let mut loaded = false;
            for ext in &["tif", "tiff", "png", "jpg", "bmp"] {
                let path = dir.join(format!("{name}.{ext}"));
                if path.exists() {
                    let img = image::open(&path)
                        .map_err(|e| MicrotomeError::Image(format!("{}: {e}", path.display())))?;
                    let gray = img.to_luma8();
                    if i == 1 {
                        width = gray.width();
                        height = gray.height();
                        data.reserve((width * height * levels) as usize);
                    }
                    data.extend_from_slice(gray.as_raw());
                    loaded = true;
                    break;
                }
            }
            if !loaded {
                return Err(MicrotomeError::Image(format!(
                    "No image found for slice {i} in {}",
                    dir.display()
                )));
            }
        }

        Ok(Self {
            data,
            width,
            height,
            levels,
            isovalue,
            min_code,
            scale,
            unit_size,
        })
    }

    /// Converts a grid code to a linear offset into the data array.
    fn code_to_offset(&self, code: PositionCode) -> Option<usize> {
        if code.x < 0 || code.y < 0 || code.z < 0 {
            return None;
        }
        let offset = code.z as usize * self.width as usize * self.height as usize
            + code.y as usize * self.width as usize
            + code.x as usize;
        let total = self.width as usize * self.height as usize * self.levels as usize;
        if offset >= total { None } else { Some(offset) }
    }

    /// Samples the volume at a grid code, averaging over the scale region.
    fn index_value(&self, code: PositionCode) -> f32 {
        let base = (code - self.min_code) * self.scale;

        // Check bounds for the base position
        if self.code_to_offset(base).is_none() {
            return self.isovalue;
        }

        let mut result = 0.0_f32;
        let mut count = 0;
        for x in 0..self.scale.x {
            for y in 0..self.scale.y {
                for z in 0..self.scale.z {
                    let sample = base + IVec3::new(x, y, z);
                    if let Some(offset) = self.code_to_offset(sample) {
                        result += self.data[offset] as f32;
                        count += 1;
                    }
                }
            }
        }

        if count > 0 {
            result / count as f32 - self.isovalue
        } else {
            self.isovalue
        }
    }
}

impl ScalarField for VolumeData {
    fn value(&self, p: Vec3) -> f32 {
        let code = pos_to_code(p, self.unit_size);
        self.index_value(code)
    }

    fn index(&self, code: PositionCode, _unit_size: f32) -> f32 {
        self.index_value(code)
    }

    fn solve(&self, p1: Vec3, p2: Vec3) -> Option<Vec3> {
        let v1 = self.value(p1);
        let v2 = self.value(p2);
        if (v2 - v1).abs() < f32::EPSILON {
            Some((p1 + p2) / 2.0)
        } else {
            Some(p1 - (p2 - p1) * v1 / (v2 - v1))
        }
    }

    fn gradient_offset(&self) -> f32 {
        self.unit_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_data_index_in_bounds() {
        // Create a 4x4x4 volume with all values = 64
        let data = vec![64u8; 4 * 4 * 4];
        let vol = VolumeData::new(data, 4, 4, 4, 32.0, IVec3::ZERO, IVec3::ONE, 1.0);
        // Value at origin: 64 - 32 = 32 (positive = outside)
        let v = vol.value(Vec3::ZERO);
        assert!((v - 32.0).abs() < 1e-3, "v = {v}");
    }

    #[test]
    fn volume_data_index_out_of_bounds() {
        let data = vec![64u8; 4 * 4 * 4];
        let vol = VolumeData::new(data, 4, 4, 4, 32.0, IVec3::ZERO, IVec3::ONE, 1.0);
        // Far outside: should return isovalue
        let v = vol.value(Vec3::new(100.0, 100.0, 100.0));
        assert!((v - 32.0).abs() < 1e-3);
    }

    #[test]
    fn volume_data_solve_linear() {
        let mut data = vec![0u8; 4 * 4 * 4];
        // Set voxel at (2,0,0) to 128 (well above isovalue)
        data[2] = 128;
        let vol = VolumeData::new(data, 4, 4, 4, 32.0, IVec3::ZERO, IVec3::ONE, 1.0);
        let p1 = Vec3::new(0.0, 0.0, 0.0);
        let p2 = Vec3::new(2.0, 0.0, 0.0);
        let result = vol.solve(p1, p2);
        assert!(result.is_some());
    }
}
