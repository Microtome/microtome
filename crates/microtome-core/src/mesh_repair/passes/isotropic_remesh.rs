//! Isotropic remeshing: alternating split / collapse / relax / reproject
//! to drive a mesh toward uniform triangle size.
//!
//! Composite pass built on top of half-edge ops, AngleRelax, and (when
//! `ctx.target` is supplied) ReprojectToSurface. Per iteration:
//!
//! 1. Split every edge longer than `4/3 × target_edge_length`.
//! 2. Collapse every edge shorter than `4/5 × target_edge_length`.
//! 3. Tangential relaxation (one pass of AngleRelax).
//! 4. Reproject vertices onto `ctx.target` (if Some).
//!
//! Edge flipping for valence-equalisation lands with v2.5; v2 first cut
//! skips that step.

use super::super::error::PassError;
use super::super::half_edge::{HalfEdgeId, HalfEdgeMesh};
use super::super::pass::{MeshRepairPass, PassOutcome};
use super::angle_relax::AngleRelax;
use super::reproject::ReprojectToSurface;
use crate::mesh_repair::RepairContext;

/// Composite isotropic remeshing pass.
#[derive(Debug, Clone)]
pub struct IsotropicRemesh {
    /// Target world-space edge length.
    pub target_edge_length: f32,
    /// Number of split / collapse / relax / reproject rounds.
    pub iterations: u32,
}

impl Default for IsotropicRemesh {
    fn default() -> Self {
        Self {
            target_edge_length: 0.0,
            iterations: 3,
        }
    }
}

impl MeshRepairPass for IsotropicRemesh {
    fn name(&self) -> &'static str {
        "isotropic_remesh"
    }

    fn reclassifies(&self) -> bool {
        true
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        if self.target_edge_length <= 0.0 || self.target_edge_length.is_nan() {
            return Err(PassError::InvalidConfig(format!(
                "isotropic_remesh target_edge_length must be > 0; got {}",
                self.target_edge_length
            )));
        }
        let mut outcome = PassOutcome::noop(self.name());
        let split_threshold = self.target_edge_length * 4.0 / 3.0;
        let collapse_threshold = self.target_edge_length * 4.0 / 5.0;

        for _ in 0..self.iterations {
            // Phase 1: split long edges.
            let long_edges: Vec<HalfEdgeId> = mesh
                .edge_iter()
                .filter(|h| mesh.edge_length(*h) > split_threshold)
                .collect();
            for he in long_edges {
                if !mesh.half_edge_is_live(he) {
                    continue;
                }
                let mid = (mesh.vertex_position(mesh.he_tail(he))
                    + mesh.vertex_position(mesh.he_head(he)))
                    * 0.5;
                if mesh.split_edge(he, mid).is_ok() {
                    outcome.stats.edges_split += 1;
                    outcome.stats.vertices_added += 1;
                }
            }

            // Phase 2: collapse short edges.
            let short_edges: Vec<HalfEdgeId> = mesh
                .edge_iter()
                .filter(|h| mesh.edge_length(*h) < collapse_threshold)
                .collect();
            for he in short_edges {
                if !mesh.half_edge_is_live(he) {
                    continue;
                }
                if mesh.collapse_edge(he).is_ok() {
                    outcome.stats.edges_collapsed += 1;
                }
            }

            // Phase 3: tangential relaxation (one round).
            let _ = AngleRelax {
                iterations: 1,
                step: 0.5,
            }
            .apply(mesh, ctx)?;

            // Phase 4: reproject onto target if available.
            if ctx.target.is_some() {
                let _ = ReprojectToSurface::default().apply(mesh, ctx)?;
            }
        }

        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;
    use glam::Vec3;

    fn iso(positions: Vec<Vec3>, indices: Vec<u32>) -> IsoMesh {
        let n = positions.len();
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; n],
            indices,
        }
    }

    /// Tetrahedron with one large face split into 9 small triangles.
    /// The 9 small ones have edge length ~0.33; the rest are length 1.
    fn mixed_edge_lengths() -> HalfEdgeMesh {
        // Octahedron — uniform-ish edge length sqrt(2) ≈ 1.41.
        let positions = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 4,  2, 1, 4,  1, 3, 4,  3, 0, 4,
            2, 0, 5,  1, 2, 5,  3, 1, 5,  0, 3, 5,
        ];
        HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("octa")
    }

    #[test]
    fn remesh_rejects_invalid_target_edge_length() {
        let mut mesh = mixed_edge_lengths();
        let pass = IsotropicRemesh::default(); // target_edge_length=0
        let ctx = RepairContext::noop();
        assert!(pass.apply(&mut mesh, &ctx).is_err());
    }

    #[test]
    fn remesh_splits_when_target_smaller_than_existing() {
        let mut mesh = mixed_edge_lengths();
        let pre_faces = mesh.face_count();
        let pass = IsotropicRemesh {
            target_edge_length: 0.5, // way smaller than ~1.41 octa edges
            iterations: 1,
        };
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("remesh");
        assert!(
            outcome.stats.edges_split > 0,
            "should split edges much longer than target"
        );
        assert!(
            mesh.face_count() > pre_faces,
            "splitting should increase face count: pre={pre_faces} post={}",
            mesh.face_count()
        );
    }

    #[test]
    fn remesh_collapses_when_target_larger_than_existing() {
        let mut mesh = mixed_edge_lengths();
        let pass = IsotropicRemesh {
            target_edge_length: 100.0, // way larger than any existing edge
            iterations: 1,
        };
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("remesh");
        // At least one collapse should have happened (every edge is < target).
        assert!(outcome.stats.edges_collapsed > 0);
    }
}
