//! Self-intersection repair (face-removal-based).
//!
//! Detects self-intersecting triangle pairs the same way
//! [`DetectSelfIntersections`](super::detect_self_intersect::DetectSelfIntersections)
//! does (BVH AABB queries + Möller tri-tri test, with the v2.5 coplanar
//! 2-D fallback) and removes both faces of every intersecting pair.
//! Removal is lossy — it leaves holes that
//! [`FillSmallHoles`](super::fill_holes::FillSmallHoles) is expected to
//! patch downstream — but it eliminates the self-intersection without
//! the complexity of constrained-edge re-triangulation.
//!
//! For v3 first cut: the simple drop-pair strategy. A future variant
//! could split + re-triangulate at the intersection segment to preserve
//! more geometry; that lands as v4 work.

use std::collections::HashSet;

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{FaceId, HalfEdgeMesh};
use super::super::pass::{MeshRepairPass, PassOutcome, PassWarningKind};
use super::super::spatial::{Aabb, TriangleBvh};
use super::super::tri_intersection::tri_tri_intersect;
use crate::mesh_repair::RepairContext;

/// Repairs self-intersections by dropping intersecting face pairs.
#[derive(Debug, Clone)]
pub struct RepairSelfIntersections {
    /// Tolerance for the Möller plane-side test and the coplanar
    /// 2-D fallback. Default `1e-6`.
    pub tolerance: f32,
}

impl Default for RepairSelfIntersections {
    fn default() -> Self {
        Self { tolerance: 1e-6 }
    }
}

impl MeshRepairPass for RepairSelfIntersections {
    fn name(&self) -> &'static str {
        "repair_self_intersections"
    }

    fn requires_manifold(&self) -> bool {
        false
    }

    fn reclassifies(&self) -> bool {
        true
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        _ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        let mut outcome = PassOutcome::noop(self.name());

        // Snapshot live faces.
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
        if faces.len() < 2 {
            return Ok(outcome);
        }
        let triangles: Vec<[Vec3; 3]> = faces.iter().map(|f| f.2).collect();
        let Some(bvh) = TriangleBvh::build(&triangles) else {
            return Ok(outcome);
        };

        // Collect every face index that participates in at least one
        // intersection. Using a set lets us drop both members of each
        // pair without relying on iteration order.
        let tolerance = self.tolerance;
        let mut to_drop: HashSet<u32> = HashSet::new();
        for i in 0..faces.len() {
            let (_, vs_a, pos_a) = (faces[i].0, faces[i].1, faces[i].2);
            let aabb_a = Aabb::from_triangle(&pos_a);
            bvh.visit_overlapping(aabb_a, |j| {
                if j <= i {
                    return;
                }
                let (_, vs_b, pos_b) = (faces[j].0, faces[j].1, faces[j].2);
                if shares_vertex(&vs_a, &vs_b) {
                    return;
                }
                if tri_tri_intersect(&pos_a, &pos_b, tolerance) {
                    to_drop.insert(faces[i].0.0);
                    to_drop.insert(faces[j].0.0);
                }
            });
        }
        if to_drop.is_empty() {
            return Ok(outcome);
        }

        let mut dropped = 0u32;
        for fid_raw in &to_drop {
            let fid = FaceId(*fid_raw);
            // remove_degenerate_face is robust against re-removal because
            // it checks face_is_live first; we ignore the InvalidHandle
            // result which can only come from a face that was already
            // marked removed.
            if mesh.face_is_live(fid) && mesh.remove_degenerate_face(fid).is_ok() {
                dropped += 1;
            }
        }

        outcome.stats.faces_removed += dropped;
        if dropped > 0 {
            outcome.warn(
                PassWarningKind::Clamped,
                format!(
                    "dropped {dropped} faces participating in self-intersection (holes left for FillSmallHoles)"
                ),
            );
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
    fn repair_drops_two_crossing_triangles() {
        // Two triangles that cross in space; disjoint vertex sets.
        let positions = vec![
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.5, 0.0, 1.0),
            Vec3::new(-0.5, 0.0, 1.0),
        ];
        let indices = vec![0, 1, 2, 3, 4, 5];
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("build");
        let pre_face_count = mesh.face_count();
        let pass = RepairSelfIntersections::default();
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("repair");
        assert_eq!(outcome.stats.faces_removed, 2);
        assert_eq!(mesh.face_count(), pre_face_count - 2);
    }

    #[test]
    fn repair_no_op_on_clean_tetrahedron() {
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let indices = vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3];
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("build");
        let pre_face_count = mesh.face_count();
        let pass = RepairSelfIntersections::default();
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("repair");
        assert_eq!(outcome.stats.faces_removed, 0);
        assert_eq!(mesh.face_count(), pre_face_count);
    }
}
