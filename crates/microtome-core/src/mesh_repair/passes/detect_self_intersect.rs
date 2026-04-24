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
use super::super::pass::{MeshRepairPass, PassOutcome, PassWarningKind};
use super::super::spatial::{Aabb, TriangleBvh};
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

/// Möller's triangle-triangle intersection test.
///
/// Returns `true` if the two triangles intersect in their interiors or
/// along a shared point (excluding the shared-vertex case, which the
/// caller should filter beforehand). The coplanar case projects to the
/// dominant plane and tests for 2-D segment overlap; `tolerance` is the
/// magnitude below which two plane equations are considered coplanar.
fn tri_tri_intersect(t1: &[Vec3; 3], t2: &[Vec3; 3], tolerance: f32) -> bool {
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

    // Coplanar case: planes have parallel normals AND every vertex of t1
    // lies on plane(t2) (within tolerance).
    let dir = n1.cross(n2);
    if dir.length_squared() < 1e-20 {
        if dv1.iter().all(|x| x.abs() <= tolerance) {
            return coplanar_tri_tri_overlap(t1, t2, n1);
        }
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

/// Coplanar fallback: project both triangles to the 2-D plane perpendicular
/// to the (largest-component-of-) shared normal, then test if any edge of t1
/// crosses any edge of t2 or if either triangle contains a vertex of the
/// other. (Adjacency / shared-vertex filtering happens at the caller.)
fn coplanar_tri_tri_overlap(t1: &[Vec3; 3], t2: &[Vec3; 3], normal: Vec3) -> bool {
    let drop_axis = if normal.x.abs() >= normal.y.abs() && normal.x.abs() >= normal.z.abs() {
        0
    } else if normal.y.abs() >= normal.z.abs() {
        1
    } else {
        2
    };
    let project = |p: Vec3| -> [f32; 2] {
        match drop_axis {
            0 => [p.y, p.z],
            1 => [p.x, p.z],
            _ => [p.x, p.y],
        }
    };
    let a = [project(t1[0]), project(t1[1]), project(t1[2])];
    let b = [project(t2[0]), project(t2[1]), project(t2[2])];
    // Edge-edge intersection: any of the 9 edge pairs cross.
    for i in 0..3 {
        for j in 0..3 {
            if segments_cross(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]) {
                return true;
            }
        }
    }
    // Containment: any vertex of one inside the other.
    for v in &a {
        if point_in_triangle_2d(*v, &b) {
            return true;
        }
    }
    for v in &b {
        if point_in_triangle_2d(*v, &a) {
            return true;
        }
    }
    false
}

fn segments_cross(p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], p4: [f32; 2]) -> bool {
    let d = (p2[0] - p1[0]) * (p4[1] - p3[1]) - (p2[1] - p1[1]) * (p4[0] - p3[0]);
    if d.abs() < 1e-12 {
        return false;
    }
    let s = ((p3[0] - p1[0]) * (p4[1] - p3[1]) - (p3[1] - p1[1]) * (p4[0] - p3[0])) / d;
    let t = ((p3[0] - p1[0]) * (p2[1] - p1[1]) - (p3[1] - p1[1]) * (p2[0] - p1[0])) / d;
    s > 0.0 && s < 1.0 && t > 0.0 && t < 1.0
}

fn point_in_triangle_2d(p: [f32; 2], tri: &[[f32; 2]; 3]) -> bool {
    let sign = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| -> f32 {
        (a[0] - c[0]) * (b[1] - c[1]) - (b[0] - c[0]) * (a[1] - c[1])
    };
    let d1 = sign(p, tri[0], tri[1]);
    let d2 = sign(p, tri[1], tri[2]);
    let d3 = sign(p, tri[2], tri[0]);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
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
