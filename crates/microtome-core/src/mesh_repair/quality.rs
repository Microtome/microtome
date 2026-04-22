//! Triangle and mesh quality metrics.
//!
//! The repair pipeline uses these for two purposes:
//!
//! - Per-pass diagnostics (`MeshQualityReport` pre- and post-pipeline).
//! - Sliver detection in [`RemoveSlivers`](super::passes::RemoveSlivers).
//!
//! All metrics are world-space; the caller chooses input units.

use glam::Vec3;

use super::half_edge::{FaceId, HalfEdgeMesh};

/// Quality metrics for a single triangle.
#[derive(Debug, Clone, Copy)]
pub struct TriangleQuality {
    /// Triangle area.
    pub area: f32,
    /// Smallest interior angle, in radians.
    pub min_angle_rad: f32,
    /// Largest interior angle, in radians.
    pub max_angle_rad: f32,
    /// Longest edge divided by the shortest edge (1.0 = equilateral).
    pub aspect_ratio: f32,
    /// Triangle failed at least one threshold in a [`QualityThresholds`].
    pub is_sliver: bool,
}

/// Thresholds a triangle must satisfy to be considered "good quality".
#[derive(Debug, Clone, Copy)]
pub struct QualityThresholds {
    /// Minimum acceptable interior angle, in degrees. Triangles with a
    /// smaller minimum angle are flagged as slivers.
    pub min_angle_deg: f32,
    /// Minimum acceptable area. Triangles with smaller area are flagged.
    /// Set to `0.0` to disable the area test.
    pub min_area: f32,
    /// Maximum acceptable aspect ratio (longest/shortest edge).
    pub max_aspect_ratio: f32,
}

impl Default for QualityThresholds {
    fn default() -> Self {
        Self {
            min_angle_deg: 5.0,
            min_area: 0.0,
            max_aspect_ratio: 50.0,
        }
    }
}

/// Aggregate mesh-level quality summary.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeshQualityReport {
    /// Live triangle count.
    pub triangle_count: u32,
    /// Smallest minimum angle found across all live triangles, in degrees.
    /// `0.0` when the mesh has no triangles.
    pub min_angle_deg: f32,
    /// Count of triangles failing the supplied thresholds.
    pub sliver_count: u32,
    /// Number of boundary loops.
    pub boundary_loop_count: u32,
    /// Count of non-manifold edges. `0` on a manifold mesh.
    pub non_manifold_edge_count: u32,
    /// Divergence-theorem approximate volume (signed). Only meaningful for
    /// closed manifold meshes; open meshes produce a domain-specific number.
    pub approx_volume: f32,
}

impl HalfEdgeMesh {
    /// Computes quality metrics for one triangular face.
    pub fn triangle_quality(&self, f: FaceId, thresholds: &QualityThresholds) -> TriangleQuality {
        let [p0, p1, p2] = self.face_positions(f);
        let e0 = p1 - p0;
        let e1 = p2 - p1;
        let e2 = p0 - p2;
        let l0 = e0.length();
        let l1 = e1.length();
        let l2 = e2.length();
        let area = 0.5 * e0.cross(-e2).length();

        // Interior angles at p0, p1, p2.
        let ang0 = angle_between(-e2, e0);
        let ang1 = angle_between(-e0, e1);
        let ang2 = angle_between(-e1, e2);
        let min_angle = ang0.min(ang1).min(ang2);
        let max_angle = ang0.max(ang1).max(ang2);

        let min_edge = l0.min(l1).min(l2);
        let max_edge = l0.max(l1).max(l2);
        let aspect_ratio = if min_edge > 0.0 {
            max_edge / min_edge
        } else {
            f32::INFINITY
        };

        let is_sliver = min_angle < thresholds.min_angle_deg.to_radians()
            || area < thresholds.min_area
            || aspect_ratio > thresholds.max_aspect_ratio;

        TriangleQuality {
            area,
            min_angle_rad: min_angle,
            max_angle_rad: max_angle,
            aspect_ratio,
            is_sliver,
        }
    }

    /// Builds a mesh-level quality report.
    pub fn quality_report(&self, thresholds: &QualityThresholds) -> MeshQualityReport {
        let mut triangle_count: u32 = 0;
        let mut min_angle_rad = f32::INFINITY;
        let mut sliver_count: u32 = 0;
        let mut volume = 0.0_f32;

        for (fi, face) in self.faces.iter().enumerate() {
            if face.removed {
                continue;
            }
            let fid = FaceId(fi as u32);
            let q = self.triangle_quality(fid, thresholds);
            triangle_count += 1;
            if q.min_angle_rad < min_angle_rad {
                min_angle_rad = q.min_angle_rad;
            }
            if q.is_sliver {
                sliver_count += 1;
            }
            // Divergence-theorem volume (p0 · (p1 × p2)) / 6 summed over faces.
            let [p0, p1, p2] = self.face_positions(fid);
            volume += p0.dot(p1.cross(p2)) / 6.0;
        }

        let min_angle_deg = if triangle_count == 0 {
            0.0
        } else {
            min_angle_rad.to_degrees()
        };

        MeshQualityReport {
            triangle_count,
            min_angle_deg,
            sliver_count,
            boundary_loop_count: self.boundary_loops().len() as u32,
            // Real count: scans live half-edges for undirected edges referenced
            // by > 2 entries. v1 ops can occasionally produce these on busy
            // DC output (see project_mesh_repair_v1 memory).
            non_manifold_edge_count: self.count_non_manifold_edges(),
            approx_volume: volume,
        }
    }
}

/// Returns the non-reflex angle between two vectors in radians.
fn angle_between(a: Vec3, b: Vec3) -> f32 {
    let na = a.length();
    let nb = b.length();
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    let cos = (a.dot(b) / (na * nb)).clamp(-1.0, 1.0);
    cos.acos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;

    fn iso(positions: Vec<Vec3>, indices: Vec<u32>) -> IsoMesh {
        let n = positions.len();
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; n],
            indices,
        }
    }

    #[test]
    fn equilateral_triangle_has_ideal_quality() {
        // Equilateral triangle with side length 1.
        let h = 0.8660254_f32; // sqrt(3) / 2
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.5, h, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .expect("construct");
        let q = mesh.triangle_quality(FaceId(0), &QualityThresholds::default());
        assert!((q.min_angle_rad - std::f32::consts::FRAC_PI_3).abs() < 1e-4);
        assert!((q.max_angle_rad - std::f32::consts::FRAC_PI_3).abs() < 1e-4);
        assert!((q.aspect_ratio - 1.0).abs() < 1e-4);
        let expected_area = 0.25 * 3.0_f32.sqrt();
        assert!((q.area - expected_area).abs() < 1e-4);
        assert!(!q.is_sliver);
    }

    #[test]
    fn needle_triangle_is_flagged_as_sliver() {
        // Long, thin triangle: (0,0) - (10,0) - (10,0.1). Min angle is tiny.
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(10.0, 0.1, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .expect("construct");
        let q = mesh.triangle_quality(FaceId(0), &QualityThresholds::default());
        assert!(q.min_angle_rad.to_degrees() < 5.0);
        assert!(q.is_sliver);
    }

    #[test]
    fn sliver_count_matches_manual_count() {
        // One equilateral + one needle.
        let h = 0.8660254_f32;
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                // Equilateral
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.5, h, 0.0),
                // Needle (disjoint)
                Vec3::new(10.0, 0.0, 0.0),
                Vec3::new(20.0, 0.0, 0.0),
                Vec3::new(20.0, 0.1, 0.0),
            ],
            vec![0, 1, 2, 3, 4, 5],
        ))
        .expect("construct");
        let report = mesh.quality_report(&QualityThresholds::default());
        assert_eq!(report.triangle_count, 2);
        assert_eq!(report.sliver_count, 1);
    }

    #[test]
    fn quality_report_on_empty_mesh() {
        let mesh = HalfEdgeMesh::new();
        let report = mesh.quality_report(&QualityThresholds::default());
        assert_eq!(report.triangle_count, 0);
        assert_eq!(report.min_angle_deg, 0.0);
        assert_eq!(report.sliver_count, 0);
    }

    #[test]
    fn boundary_loop_count_via_report() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .expect("construct");
        let report = mesh.quality_report(&QualityThresholds::default());
        assert_eq!(report.boundary_loop_count, 1);
    }

    #[test]
    fn aspect_ratio_reflects_edge_lengths() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .expect("construct");
        let q = mesh.triangle_quality(FaceId(0), &QualityThresholds::default());
        // Edges: 3, sqrt(10)≈3.162, 1. Ratio 3.162 / 1 ≈ 3.162.
        assert!((q.aspect_ratio - 10.0_f32.sqrt()).abs() < 1e-4);
    }
}
