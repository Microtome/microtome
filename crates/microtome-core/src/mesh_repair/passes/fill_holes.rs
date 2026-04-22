//! Fill small boundary loops (holes) by fanning from a centroid vertex.
//!
//! Targets DC output where the isosurface is "open" at a cell boundary
//! (typically around thin features the extraction couldn't close) and
//! would otherwise show up as a visible hole in the output mesh. Loops
//! larger than a budget are left alone and reported as
//! [`PassWarningKind::BudgetExceeded`].

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{HalfEdgeId, HalfEdgeMesh};
use super::super::pass::{MeshRepairPass, PassOutcome, PassWarningKind};

/// How to triangulate a boundary loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HoleFillMethod {
    /// Add a new centroid vertex and fan-triangulate.
    CentroidFan,
    /// Ear-clip in the best-fit plane. v1 falls back to `CentroidFan`.
    EarClip,
    /// Pick `CentroidFan` for loops of length ≤ 4, `EarClip` otherwise.
    /// v1 always uses `CentroidFan` because ear-clipping lands in v2.
    Auto,
}

/// Fills boundary loops up to a configurable perimeter budget.
#[derive(Debug, Clone)]
pub struct FillSmallHoles {
    /// Maximum boundary-loop length (count of half-edges) to attempt to
    /// close. Larger loops emit a `BudgetExceeded` warning and are left.
    pub max_boundary_length: u32,
    /// Which triangulation method to use.
    pub method: HoleFillMethod,
}

impl Default for FillSmallHoles {
    fn default() -> Self {
        Self {
            max_boundary_length: 8,
            method: HoleFillMethod::Auto,
        }
    }
}

impl MeshRepairPass for FillSmallHoles {
    fn name(&self) -> &'static str {
        "fill_small_holes"
    }

    fn apply(&self, mesh: &mut HalfEdgeMesh) -> Result<PassOutcome, PassError> {
        let mut outcome = PassOutcome::noop(self.name());
        let loops = mesh.boundary_loops();
        for loop_hes in &loops {
            let n = loop_hes.len() as u32;
            if n > self.max_boundary_length {
                outcome.warn(
                    PassWarningKind::BudgetExceeded,
                    format!(
                        "boundary loop of {n} half-edges exceeds budget {}",
                        self.max_boundary_length
                    ),
                );
                continue;
            }
            if n < 3 {
                outcome.warn(
                    PassWarningKind::Skipped,
                    format!("boundary loop of length {n} is too small to fill"),
                );
                continue;
            }
            // v1: always centroid-fan. Method enum exists for v2 ear-clip.
            let _ = self.method;
            let centroid = loop_centroid(mesh, loop_hes);
            match mesh.add_fan_over_boundary_loop(loop_hes, centroid) {
                Ok((_w, faces)) => {
                    outcome.stats.vertices_added += 1;
                    outcome.stats.faces_added += faces.len() as u32;
                    outcome.stats.holes_filled += 1;
                }
                Err(err) => {
                    outcome.warn(
                        PassWarningKind::Skipped,
                        format!("could not fan-close loop: {err}"),
                    );
                }
            }
        }
        Ok(outcome)
    }
}

fn loop_centroid(mesh: &HalfEdgeMesh, loop_hes: &[HalfEdgeId]) -> Vec3 {
    let mut sum = Vec3::ZERO;
    let mut n = 0;
    for &h in loop_hes {
        sum += mesh.vertex_position(mesh.he_tail(h));
        n += 1;
    }
    if n == 0 { Vec3::ZERO } else { sum / (n as f32) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;

    /// Cube with the +z face removed (same as half_edge test helper).
    fn cube_with_hole() -> IsoMesh {
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ];
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 1,  0, 3, 2,
            0, 1, 5,  0, 5, 4,
            1, 2, 6,  1, 6, 5,
            2, 3, 7,  2, 7, 6,
            3, 0, 4,  3, 4, 7,
        ];
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; 8],
            indices,
        }
    }

    #[test]
    fn fill_closes_single_four_edge_hole() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&cube_with_hole()).expect("build");
        assert_eq!(mesh.boundary_loops().len(), 1);
        let pass = FillSmallHoles::default();
        let outcome = pass.apply(&mut mesh).expect("fill");
        assert_eq!(outcome.stats.holes_filled, 1);
        assert_eq!(outcome.stats.vertices_added, 1);
        assert_eq!(outcome.stats.faces_added, 4);
        // After filling, no more boundary loops.
        assert!(mesh.boundary_loops().is_empty());
        assert!(mesh.is_manifold());
    }

    #[test]
    fn fill_warns_on_oversize_loop() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&cube_with_hole()).expect("build");
        // The hole has 4 edges. Budget of 3 forces a skip.
        let pass = FillSmallHoles {
            max_boundary_length: 3,
            method: HoleFillMethod::Auto,
        };
        let outcome = pass.apply(&mut mesh).expect("fill");
        assert_eq!(outcome.stats.holes_filled, 0);
        assert_eq!(outcome.warnings.len(), 1);
        assert_eq!(outcome.warnings[0].kind, PassWarningKind::BudgetExceeded);
    }

    #[test]
    fn fill_no_op_on_closed_mesh() {
        let closed = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            normals: vec![Vec3::Z; 4],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        };
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&closed).expect("build");
        let pass = FillSmallHoles::default();
        let outcome = pass.apply(&mut mesh).expect("fill");
        assert_eq!(outcome.stats.holes_filled, 0);
        assert!(outcome.warnings.is_empty());
    }
}
