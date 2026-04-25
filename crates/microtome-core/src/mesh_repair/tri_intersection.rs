//! Triangle-triangle intersection (Möller's test + coplanar 2-D fallback).
//!
//! Shared between [`DetectSelfIntersections`](super::passes::DetectSelfIntersections)
//! (query-only) and [`RepairSelfIntersections`](super::passes::RepairSelfIntersections)
//! (drop-pair). Both passes use the same predicate; centralising it
//! here removes a ~150-line duplicate.
//!
//! The non-coplanar path follows Möller, *A Fast Triangle-Triangle
//! Intersection Test* (1997). The coplanar fallback projects both
//! triangles to the dominant plane of the shared normal and checks for
//! 2-D edge-edge crossings or vertex-in-triangle containment.

use glam::Vec3;

/// Returns `true` if the two triangles intersect in their interiors or
/// share at least one interior point. Adjacency-by-shared-vertex must be
/// filtered by the caller — this routine only handles the geometric
/// intersection test.
///
/// `tolerance` controls the coplanar test: two triangles are treated as
/// coplanar when every vertex of `t1` lies within `tolerance` world units
/// of `plane(t2)` *and* the two plane normals are parallel.
pub fn tri_tri_intersect(t1: &[Vec3; 3], t2: &[Vec3; 3], tolerance: f32) -> bool {
    let n2 = (t2[1] - t2[0]).cross(t2[2] - t2[0]);
    let d2 = -n2.dot(t2[0]);
    let dv1 = [n2.dot(t1[0]) + d2, n2.dot(t1[1]) + d2, n2.dot(t1[2]) + d2];
    if (dv1[0] > 0.0 && dv1[1] > 0.0 && dv1[2] > 0.0)
        || (dv1[0] < 0.0 && dv1[1] < 0.0 && dv1[2] < 0.0)
    {
        return false;
    }

    let n1 = (t1[1] - t1[0]).cross(t1[2] - t1[0]);
    let d1 = -n1.dot(t1[0]);
    let dv2 = [n1.dot(t2[0]) + d1, n1.dot(t2[1]) + d1, n1.dot(t2[2]) + d1];
    if (dv2[0] > 0.0 && dv2[1] > 0.0 && dv2[2] > 0.0)
        || (dv2[0] < 0.0 && dv2[1] < 0.0 && dv2[2] < 0.0)
    {
        return false;
    }

    let dir = n1.cross(n2);
    if dir.length_squared() < 1e-20 {
        if dv1.iter().all(|x| x.abs() <= tolerance) {
            return coplanar_overlap(t1, t2, n1);
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
    let isect1 = isect_interval(p1, dv1);
    let isect2 = isect_interval(p2, dv2);
    let (lo1, hi1) = order(isect1);
    let (lo2, hi2) = order(isect2);
    !(hi1 < lo2 || hi2 < lo1)
}

/// Coplanar fallback: project both triangles to the dominant plane of
/// `normal` and test for 2-D segment-segment crossings + vertex containment.
fn coplanar_overlap(t1: &[Vec3; 3], t2: &[Vec3; 3], normal: Vec3) -> bool {
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
    for i in 0..3 {
        for j in 0..3 {
            if segments_cross(a[i], a[(i + 1) % 3], b[j], b[(j + 1) % 3]) {
                return true;
            }
        }
    }
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

fn isect_interval(proj: [f32; 3], dv: [f32; 3]) -> (f32, f32) {
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

    #[test]
    fn disjoint_triangles_dont_intersect() {
        let t1 = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let t2 = [
            Vec3::new(10.0, 10.0, 10.0),
            Vec3::new(11.0, 10.0, 10.0),
            Vec3::new(10.0, 11.0, 10.0),
        ];
        assert!(!tri_tri_intersect(&t1, &t2, 1e-6));
    }

    #[test]
    fn crossing_triangles_intersect() {
        let t1 = [
            Vec3::new(-1.0, -1.0, 0.0),
            Vec3::new(1.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        let t2 = [
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.5, 0.0, 1.0),
            Vec3::new(-0.5, 0.0, 1.0),
        ];
        assert!(tri_tri_intersect(&t1, &t2, 1e-6));
    }

    #[test]
    fn coplanar_overlapping_triangles_intersect() {
        let t1 = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 0.0, 0.0),
            Vec3::new(0.0, 4.0, 0.0),
        ];
        let t2 = [
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            Vec3::new(1.0, 2.0, 0.0),
        ];
        assert!(tri_tri_intersect(&t1, &t2, 1e-6));
    }
}
