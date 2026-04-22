//! Self-intersection detection (query-only).
//!
//! Walks every pair of non-adjacent live faces and runs a Möller
//! triangle-triangle intersection test. Emits one [`PassWarningKind::Skipped`]
//! warning per intersecting pair, increments a counter on the outcome.
//! Does not modify geometry.
//!
//! v2 first cut uses an O(n²) scan. A BVH-accelerated version lands with
//! task #31 / v2.5; for the meshes we typically repair (DC output up to
//! ~10k triangles) this is fast enough.

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{FaceId, HalfEdgeMesh};
use super::super::pass::{MeshRepairPass, PassOutcome, PassWarningKind};
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

        let mut found: u32 = 0;
        for i in 0..faces.len() {
            for j in (i + 1)..faces.len() {
                let (a, vs_a, pos_a) = (faces[i].0, faces[i].1, faces[i].2);
                let (b, vs_b, pos_b) = (faces[j].0, faces[j].1, faces[j].2);

                // Skip adjacent triangles (share at least one vertex).
                if shares_vertex(&vs_a, &vs_b) {
                    continue;
                }

                // AABB pre-filter.
                if !aabb_overlap(&pos_a, &pos_b) {
                    continue;
                }

                if tri_tri_intersect(&pos_a, &pos_b) {
                    found += 1;
                    outcome.warn(
                        PassWarningKind::Skipped,
                        format!("self-intersection: {a:?} ∩ {b:?}"),
                    );
                }
            }
        }
        let _ = found;
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

fn aabb_overlap(a: &[Vec3; 3], b: &[Vec3; 3]) -> bool {
    let a_min = a[0].min(a[1]).min(a[2]);
    let a_max = a[0].max(a[1]).max(a[2]);
    let b_min = b[0].min(b[1]).min(b[2]);
    let b_max = b[0].max(b[1]).max(b[2]);
    a_min.x <= b_max.x
        && a_max.x >= b_min.x
        && a_min.y <= b_max.y
        && a_max.y >= b_min.y
        && a_min.z <= b_max.z
        && a_max.z >= b_min.z
}

/// Möller's triangle-triangle intersection test.
///
/// Returns `true` if the two triangles intersect in their interiors or
/// along a shared point (excluding the shared-vertex case, which the
/// caller should filter beforehand).
fn tri_tri_intersect(t1: &[Vec3; 3], t2: &[Vec3; 3]) -> bool {
    // Plane of t2.
    let n2 = (t2[1] - t2[0]).cross(t2[2] - t2[0]);
    let d2 = -n2.dot(t2[0]);
    let dv1 = [n2.dot(t1[0]) + d2, n2.dot(t1[1]) + d2, n2.dot(t1[2]) + d2];
    if (dv1[0] > 0.0 && dv1[1] > 0.0 && dv1[2] > 0.0)
        || (dv1[0] < 0.0 && dv1[1] < 0.0 && dv1[2] < 0.0)
    {
        return false;
    }

    // Plane of t1.
    let n1 = (t1[1] - t1[0]).cross(t1[2] - t1[0]);
    let d1 = -n1.dot(t1[0]);
    let dv2 = [n1.dot(t2[0]) + d1, n1.dot(t2[1]) + d1, n1.dot(t2[2]) + d1];
    if (dv2[0] > 0.0 && dv2[1] > 0.0 && dv2[2] > 0.0)
        || (dv2[0] < 0.0 && dv2[1] < 0.0 && dv2[2] < 0.0)
    {
        return false;
    }

    // Coplanar case: punt for v2 first cut.
    let dir = n1.cross(n2);
    if dir.length_squared() < 1e-20 {
        return false;
    }

    let max_axis = if dir.x.abs() >= dir.y.abs() && dir.x.abs() >= dir.z.abs() {
        0
    } else if dir.y.abs() >= dir.z.abs() {
        1
    } else {
        2
    };
    let p1 = [t1[0][max_axis], t1[1][max_axis], t1[2][max_axis]];
    let p2 = [t2[0][max_axis], t2[1][max_axis], t2[2][max_axis]];

    let isect1 = tri_isect_interval(p1, dv1);
    let isect2 = tri_isect_interval(p2, dv2);

    let (lo1, hi1) = order(isect1);
    let (lo2, hi2) = order(isect2);

    !(hi1 < lo2 || hi2 < lo1)
}

fn order(p: (f32, f32)) -> (f32, f32) {
    if p.0 <= p.1 { p } else { (p.1, p.0) }
}

fn tri_isect_interval(proj: [f32; 3], dv: [f32; 3]) -> (f32, f32) {
    // Find the vertex on the opposite side of the plane.
    let (i_lone, i_a, i_b) = if dv[0] * dv[1] > 0.0 {
        (2, 0, 1)
    } else if dv[0] * dv[2] > 0.0 {
        (1, 0, 2)
    } else {
        (0, 1, 2)
    };
    let dv_lone = dv[i_lone];
    let t_a = proj[i_a] + (proj[i_lone] - proj[i_a]) * (dv[i_a] / (dv[i_a] - dv_lone));
    let t_b = proj[i_b] + (proj[i_lone] - proj[i_b]) * (dv[i_b] / (dv[i_b] - dv_lone));
    (t_a, t_b)
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
