//! Self-intersection detection (query-only).
//!
//! Builds a [`TriangleBvh`](super::super::spatial::TriangleBvh) over the
//! live faces, then for each face issues an AABB-overlap query — only the
//! hits get the Möller triangle-triangle intersection test. Adjacent
//! triangles (those sharing at least one vertex) are filtered out so a
//! shared edge or vertex doesn't spuriously fire. Emits one
//! [`PassWarningKind::Skipped`] warning per intersecting pair; does not
//! modify geometry.
//!
//! Coplanar / near-coplanar pairs were punted in v2 first cut. v2.5 adds
//! a 2-D segment-overlap fallback that catches them.

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{FaceId, HalfEdgeMesh};
use super::super::pass::{MeshRepairPass, PassOutcome, PassStage, PassWarningKind};
use super::super::spatial::{Aabb, TriangleBvh};
use super::super::tri_intersection::tri_tri_intersect;
use crate::mesh_repair::RepairContext;

/// Detects self-intersecting triangle pairs.
#[derive(Debug, Clone)]
pub struct DetectSelfIntersections {
    /// Tolerance for the plane-side test (currently unused; reserved for
    /// future tuning of the coplanar / near-coplanar case).
    pub tolerance: f32,
}

impl Default for DetectSelfIntersections {
    fn default() -> Self {
        Self { tolerance: 1e-6 }
    }
}

impl MeshRepairPass for DetectSelfIntersections {
    fn name(&self) -> &'static str {
        "detect_self_intersections"
    }

    fn stage(&self) -> PassStage {
        PassStage::HalfEdge
    }

    fn requires_manifold(&self) -> bool {
        false
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        _ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        let mut outcome = PassOutcome::noop(self.name());

        // Collect live face data once.
        let faces: Vec<(FaceId, [u32; 3], [Vec3; 3])> = (0..mesh.faces.len())
            .filter_map(|fi| {
                let face = &mesh.faces[fi];
                if face.removed {
                    return None;
                }
                let fid = FaceId(fi as u32);
                let verts = mesh.face_vertices(fid);
                let positions = mesh.face_positions(fid);
                Some((fid, [verts[0].0, verts[1].0, verts[2].0], positions))
            })
            .collect();

        if faces.is_empty() {
            return Ok(outcome);
        }
        let triangles: Vec<[Vec3; 3]> = faces.iter().map(|f| f.2).collect();
        let Some(bvh) = TriangleBvh::build(&triangles) else {
            return Ok(outcome);
        };

        let tolerance = self.tolerance;
        for i in 0..faces.len() {
            let (a, vs_a, pos_a) = (faces[i].0, faces[i].1, faces[i].2);
            let aabb_a = Aabb::from_triangle(&pos_a);
            bvh.visit_overlapping(aabb_a, |j| {
                if j <= i {
                    return; // unordered pair; visit each once.
                }
                let (b, vs_b, pos_b) = (faces[j].0, faces[j].1, faces[j].2);
                if shares_vertex(&vs_a, &vs_b) {
                    return;
                }
                if tri_tri_intersect(&pos_a, &pos_b, tolerance) {
                    outcome.warn(
                        PassWarningKind::Skipped,
                        format!("self-intersection: {a:?} ∩ {b:?}"),
                    );
                }
            });
        }
        Ok(outcome)
    }
}

fn shares_vertex(a: &[u32; 3], b: &[u32; 3]) -> bool {
    for av in a {
        for bv in b {
            if av == bv {
                return true;
            }
        }
    }
    false
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
    fn detect_finds_two_crossing_triangles() {
        // Two triangles that cross each other in space.
        // Triangle A: in the xy-plane around origin.
        // Triangle B: in the xz-plane crossing through it.
        let positions = vec![
            // Tri A
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            // Tri B (disjoint vertex set)
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.5, 0.0, 1.0),
            Vec3::new(-0.5, 0.0, 1.0),
        ];
        let indices = vec![0, 1, 2, 3, 4, 5];
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("build");
        let pass = DetectSelfIntersections::default();
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("detect");
        assert!(
            !outcome.warnings.is_empty(),
            "should report at least one self-intersection"
        );
    }

    #[test]
    fn detect_ignores_adjacent_triangles() {
        // Two triangles sharing an edge — touching but not crossing.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        ];
        let indices = vec![0, 1, 2, 1, 3, 2];
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("build");
        let pass = DetectSelfIntersections::default();
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("detect");
        assert!(
            outcome.warnings.is_empty(),
            "adjacent triangles should not report intersection"
        );
    }

    #[test]
    fn detect_finds_coplanar_overlap() {
        // Two coplanar triangles in z=0 that overlap in 2-D; share no vertex.
        let positions = vec![
            // Tri A: large
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
            // Tri B: small, fully inside A; disjoint vertex set.
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
        ];
        let indices = vec![0, 1, 2, 3, 4, 5];
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("build");
        let pass = DetectSelfIntersections::default();
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("detect");
        assert!(
            !outcome.warnings.is_empty(),
            "coplanar overlap should be detected"
        );
    }

    #[test]
    fn detect_emits_zero_on_clean_tetrahedron() {
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let indices = vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("build");
        let pass = DetectSelfIntersections::default();
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("detect");
        assert!(outcome.warnings.is_empty());
    }
}
