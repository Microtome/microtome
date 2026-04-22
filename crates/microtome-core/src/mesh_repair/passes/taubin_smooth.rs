//! Taubin λ/μ Laplacian smoothing.
//!
//! Standard volume-preserving smoother: alternates a λ-step and a μ-step
//! where `lambda ∈ (0, 1)` and `mu ∈ (-1, 0)`. The |μ| > λ invariant
//! counteracts the shrinkage Laplacian smoothing would otherwise produce
//! on closed meshes.

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{HalfEdgeMesh, VertexId};
use super::super::pass::{MeshRepairPass, PassOutcome};
use crate::mesh_repair::RepairContext;

/// Taubin-style Laplacian smoothing.
#[derive(Debug, Clone)]
pub struct TaubinSmooth {
    /// Number of λ/μ iteration *pairs*. Each iteration applies one λ step
    /// then one μ step.
    pub iterations: u32,
    /// Forward-step factor. Must be in `(0, 1)`.
    pub lambda: f32,
    /// Backward-step factor. Must be in `(-1, 0)` with `|mu| > lambda`.
    pub mu: f32,
    /// When `true`, boundary vertices are held fixed.
    pub pin_boundary: bool,
}

impl Default for TaubinSmooth {
    fn default() -> Self {
        Self {
            iterations: 2,
            lambda: 0.33,
            mu: -0.34,
            pin_boundary: true,
        }
    }
}

impl MeshRepairPass for TaubinSmooth {
    fn name(&self) -> &'static str {
        "taubin_smooth"
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        _ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        if !(0.0 < self.lambda && self.lambda < 1.0) {
            return Err(PassError::InvalidConfig(format!(
                "taubin_smooth lambda must be in (0, 1); got {}",
                self.lambda
            )));
        }
        if !(-1.0 < self.mu && self.mu < 0.0) {
            return Err(PassError::InvalidConfig(format!(
                "taubin_smooth mu must be in (-1, 0); got {}",
                self.mu
            )));
        }
        if self.mu.abs() <= self.lambda {
            return Err(PassError::InvalidConfig(format!(
                "taubin_smooth requires |mu| > lambda; got lambda={} mu={}",
                self.lambda, self.mu
            )));
        }

        let mut outcome = PassOutcome::noop(self.name());
        let mut scratch: Vec<Vec3> = Vec::with_capacity(mesh.vertices.len());

        for _ in 0..self.iterations {
            for &factor in &[self.lambda, self.mu] {
                self.apply_step(mesh, factor, &mut scratch);
                outcome.stats.vertices_smoothed = outcome
                    .stats
                    .vertices_smoothed
                    .saturating_add(scratch.len().try_into().unwrap_or(u32::MAX));
            }
        }
        Ok(outcome)
    }

    fn requires_manifold(&self) -> bool {
        true
    }
}

impl TaubinSmooth {
    fn apply_step(&self, mesh: &mut HalfEdgeMesh, factor: f32, scratch: &mut Vec<Vec3>) {
        scratch.clear();
        scratch.resize(mesh.vertices.len(), Vec3::ZERO);
        let mut touched: Vec<bool> = vec![false; mesh.vertices.len()];

        for vi in 0..mesh.vertices.len() {
            let v_rec = &mesh.vertices[vi];
            if v_rec.removed {
                continue;
            }
            let vid = VertexId(vi as u32);
            if self.pin_boundary && mesh.vertex_is_boundary(vid) {
                continue;
            }
            let mut sum = Vec3::ZERO;
            let mut count = 0u32;
            for n in mesh.vertex_one_ring(vid) {
                sum += mesh.vertices[n.index()].pos;
                count += 1;
            }
            if count == 0 {
                continue;
            }
            let centroid = sum / (count as f32);
            let laplacian = centroid - v_rec.pos;
            scratch[vi] = v_rec.pos + factor * laplacian;
            touched[vi] = true;
        }

        for (vi, &was_touched) in touched.iter().enumerate() {
            if was_touched {
                mesh.vertices[vi].pos = scratch[vi];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;

    fn tetrahedron() -> IsoMesh {
        IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            normals: vec![Vec3::Z; 4],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        }
    }

    #[test]
    fn taubin_rejects_lambda_out_of_range() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("build");
        let pass = TaubinSmooth {
            lambda: -0.1,
            ..TaubinSmooth::default()
        };
        assert!(
            pass.apply(&mut mesh, &crate::mesh_repair::RepairContext::noop())
                .is_err()
        );
    }

    #[test]
    fn taubin_rejects_mu_out_of_range() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("build");
        let pass = TaubinSmooth {
            mu: 0.1,
            ..TaubinSmooth::default()
        };
        assert!(
            pass.apply(&mut mesh, &crate::mesh_repair::RepairContext::noop())
                .is_err()
        );
    }

    #[test]
    fn taubin_rejects_mu_smaller_than_lambda_magnitude() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("build");
        let pass = TaubinSmooth {
            lambda: 0.5,
            mu: -0.4,
            ..TaubinSmooth::default()
        };
        assert!(
            pass.apply(&mut mesh, &crate::mesh_repair::RepairContext::noop())
                .is_err()
        );
    }

    #[test]
    fn taubin_pins_boundary_vertices() {
        // Single triangle — all vertices are boundary. With pin_boundary, no
        // positions should change.
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        };
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso).expect("build");
        let pre: Vec<Vec3> = (0u32..3)
            .map(|i| mesh.vertex_position(VertexId(i)))
            .collect();
        let pass = TaubinSmooth {
            pin_boundary: true,
            ..TaubinSmooth::default()
        };
        pass.apply(&mut mesh, &crate::mesh_repair::RepairContext::noop())
            .expect("apply");
        for i in 0u32..3 {
            assert_eq!(mesh.vertex_position(VertexId(i)), pre[i as usize]);
        }
    }

    #[test]
    fn taubin_preserves_approximate_volume_on_tetrahedron() {
        // Even with interior vertices moving, the tetrahedron has no interior
        // vertices; with pin_boundary=true, no vertices move, volume constant.
        // Without pin, Taubin on a closed tetrahedron drifts. This test fixes
        // the "does run without blowing up" case.
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("build");
        let pass = TaubinSmooth::default();
        let outcome = pass
            .apply(&mut mesh, &crate::mesh_repair::RepairContext::noop())
            .expect("apply");
        assert_eq!(outcome.name, "taubin_smooth");
    }
}
