//! Bounding-volume hierarchy over a triangle mesh, specialised for the
//! **generalized winding number** query used by
//! [`mesh_scan`](super::mesh_scan).
//!
//! Each internal node stores an aggregate **dipole moment** and a
//! weighted centroid (Jacobson 2013 / Barill 2018); far-field queries
//! use a single dipole evaluation instead of iterating all triangles in
//! the subtree. At the leaves an exact Van Oosterom–Strackee signed
//! solid angle is summed per triangle, so the query agrees with a
//! brute-force GWN to the accuracy of the approximation rule
//! (`d > β · radius`, β = 2) almost everywhere, and exactly near the
//! surface. Expected speedup over the naive O(N) query is O(log N)
//! amortised per point.
//!
//! Only the winding query is exposed; the BVH is pub(super) and only
//! used internally by the scan-conversion pipeline.
//!
//! # Conventions
//! - `agg_dipole = Σ 0.5·(v1−v0)×(v2−v0)` (= Σ signed vector area)
//! - `agg_centroid` is area-weighted over subtree triangles
//! - `radius` is the max distance from `agg_centroid` to any vertex in
//!   the subtree (conservative enough for the β test)

use glam::Vec3;

use crate::mesh::MeshData;

/// Threshold on `d / radius` above which we accept the dipole
/// approximation instead of recursing. β = 2 gives sub-percent winding
/// error, comfortably below the 0.5 inside/outside threshold.
const BETA: f32 = 2.0;

/// Maximum number of triangles in a leaf. Too small blows up the node
/// count; too large defeats the purpose of the tree.
const LEAF_SIZE: usize = 8;

pub(super) struct MeshBvh {
    nodes: Vec<Node>,
    triangles: Vec<Triangle>,
}

struct Triangle {
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    centroid: Vec3,
    /// `0.5 · (v1 − v0) × (v2 − v0)` — the oriented area vector, magnitude = area.
    dipole: Vec3,
}

struct Node {
    agg_centroid: Vec3,
    agg_dipole: Vec3,
    radius: f32,
    kind: NodeKind,
}

enum NodeKind {
    Internal { left: u32, right: u32 },
    Leaf { start: u32, end: u32 },
}

impl MeshBvh {
    /// Builds a BVH over the triangles of `mesh`. O(N log N).
    pub(super) fn build(mesh: &MeshData) -> Self {
        let mut triangles: Vec<Triangle> = Vec::with_capacity(mesh.indices.len() / 3);
        let tri_count = mesh.indices.len() / 3;
        for t in 0..tri_count {
            let i0 = mesh.indices[t * 3] as usize;
            let i1 = mesh.indices[t * 3 + 1] as usize;
            let i2 = mesh.indices[t * 3 + 2] as usize;
            let v0 = Vec3::from(mesh.vertices[i0].position);
            let v1 = Vec3::from(mesh.vertices[i1].position);
            let v2 = Vec3::from(mesh.vertices[i2].position);
            let centroid = (v0 + v1 + v2) / 3.0;
            let dipole = 0.5 * (v1 - v0).cross(v2 - v0);
            triangles.push(Triangle {
                v0,
                v1,
                v2,
                centroid,
                dipole,
            });
        }

        let mut nodes: Vec<Node> = Vec::new();
        if !triangles.is_empty() {
            let len = triangles.len() as u32;
            build_recursive(&mut nodes, &mut triangles, 0, len);
        }

        Self { nodes, triangles }
    }

    /// Generalized winding number at `point`. For a consistently-
    /// oriented mesh this is ≈ the integer count of components
    /// containing `point`. Error is O((radius/d)²) per far-field node
    /// and the far-field test requires `d > β · radius`, so values
    /// comfortably above or below 0.5 are faithful; points right on
    /// the surface fall through to exact leaf evaluation.
    pub(super) fn winding_number(&self, point: Vec3) -> f32 {
        if self.nodes.is_empty() {
            return 0.0;
        }
        let mut accum = 0.0f32;
        let mut stack: Vec<u32> = Vec::with_capacity(64);
        stack.push(0);
        while let Some(ni) = stack.pop() {
            let node = &self.nodes[ni as usize];
            let r = node.agg_centroid - point;
            let d2 = r.length_squared();
            if d2 > (BETA * node.radius).powi(2) {
                // Far-field: dipole approximation. Contribution to
                // `Σ Ω` is `(dipole · r) / |r|³`; we divide by 4π at
                // the very end.
                let d = d2.sqrt();
                accum += node.agg_dipole.dot(r) / (d2 * d);
                continue;
            }
            match node.kind {
                NodeKind::Internal { left, right } => {
                    stack.push(left);
                    stack.push(right);
                }
                NodeKind::Leaf { start, end } => {
                    for i in start..end {
                        accum += exact_signed_solid_angle(&self.triangles[i as usize], point);
                    }
                }
            }
        }
        accum / (4.0 * std::f32::consts::PI)
    }

    #[cfg(test)]
    pub(super) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    #[cfg(test)]
    pub(super) fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
}

/// Van Oosterom–Strackee signed solid angle for triangle vs point. The
/// return value is the actual Ω (not Ω/2) so it can be summed with the
/// dipole-approximation contributions directly.
fn exact_signed_solid_angle(tri: &Triangle, point: Vec3) -> f32 {
    let a = tri.v0 - point;
    let b = tri.v1 - point;
    let c = tri.v2 - point;
    let la = a.length();
    let lb = b.length();
    let lc = c.length();
    // Point coincident with a vertex — this triangle's contribution is
    // singular; skip and let neighboring triangles carry the winding.
    if la < 1e-20 || lb < 1e-20 || lc < 1e-20 {
        return 0.0;
    }
    let num = a.dot(b.cross(c));
    let denom = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
    2.0 * num.atan2(denom)
}

/// Recursive top-down BVH build. Reorders `tris` in place so each
/// leaf's triangles are contiguous; returns the node index written.
fn build_recursive(nodes: &mut Vec<Node>, tris: &mut [Triangle], start: u32, end: u32) -> u32 {
    let node_idx = nodes.len() as u32;
    // Placeholder to reserve the slot; we overwrite below.
    nodes.push(Node {
        agg_centroid: Vec3::ZERO,
        agg_dipole: Vec3::ZERO,
        radius: 0.0,
        kind: NodeKind::Leaf { start, end },
    });

    let slice_start = start as usize;
    let slice_end = end as usize;
    let count = slice_end - slice_start;

    // Aggregate bbox, dipole, and area-weighted centroid over this subtree.
    let mut bbox_min = Vec3::splat(f32::INFINITY);
    let mut bbox_max = Vec3::splat(f32::NEG_INFINITY);
    let mut agg_dipole = Vec3::ZERO;
    let mut centroid_weighted = Vec3::ZERO;
    let mut total_weight = 0.0f32;
    for tri in &tris[slice_start..slice_end] {
        bbox_min = bbox_min.min(tri.v0).min(tri.v1).min(tri.v2);
        bbox_max = bbox_max.max(tri.v0).max(tri.v1).max(tri.v2);
        agg_dipole += tri.dipole;
        let area = tri.dipole.length();
        centroid_weighted += tri.centroid * area;
        total_weight += area;
    }
    let agg_centroid = if total_weight > 1e-20 {
        centroid_weighted / total_weight
    } else {
        // Degenerate subtree (all zero-area triangles) — fall back to
        // bbox center so the β test still has a sensible reference.
        (bbox_min + bbox_max) * 0.5
    };
    let mut radius = 0.0f32;
    for tri in &tris[slice_start..slice_end] {
        for v in [tri.v0, tri.v1, tri.v2] {
            let d = (v - agg_centroid).length();
            if d > radius {
                radius = d;
            }
        }
    }

    let kind = if count <= LEAF_SIZE {
        NodeKind::Leaf { start, end }
    } else {
        // Split on the longest bbox axis at the centroid median.
        let extent = bbox_max - bbox_min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        let sub = &mut tris[slice_start..slice_end];
        sub.sort_by(|a, b| {
            a.centroid[axis]
                .partial_cmp(&b.centroid[axis])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let mid = start + (count as u32) / 2;
        let left = build_recursive(nodes, tris, start, mid);
        let right = build_recursive(nodes, tris, mid, end);
        NodeKind::Internal { left, right }
    };

    nodes[node_idx as usize] = Node {
        agg_centroid,
        agg_dipole,
        radius,
        kind,
    };
    node_idx
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{BoundingBox, MeshData, MeshVertex};
    use glam::Vec3;

    /// Brute-force GWN used as the ground truth in tests. Mirrors the
    /// implementation in `mesh_scan` so we can compare apples to apples.
    fn naive_gwn(mesh: &MeshData, point: Vec3) -> f32 {
        let mut accum = 0.0f32;
        let tri_count = mesh.indices.len() / 3;
        for t in 0..tri_count {
            let i0 = mesh.indices[t * 3] as usize;
            let i1 = mesh.indices[t * 3 + 1] as usize;
            let i2 = mesh.indices[t * 3 + 2] as usize;
            let a = Vec3::from(mesh.vertices[i0].position) - point;
            let b = Vec3::from(mesh.vertices[i1].position) - point;
            let c = Vec3::from(mesh.vertices[i2].position) - point;
            let la = a.length();
            let lb = b.length();
            let lc = c.length();
            if la < 1e-20 || lb < 1e-20 || lc < 1e-20 {
                continue;
            }
            let num = a.dot(b.cross(c));
            let denom = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
            accum += 2.0 * num.atan2(denom);
        }
        accum / (4.0 * std::f32::consts::PI)
    }

    /// Builds an icosphere-like triangle mesh: an icosahedron subdivided
    /// `subdivisions` times, then projected to the unit sphere. Triangle
    /// normals face outward.
    fn make_icosphere(subdivisions: u32, radius: f32, center: Vec3) -> MeshData {
        // 12 icosahedron vertices.
        let phi = (1.0 + 5.0f32.sqrt()) / 2.0;
        let raw = [
            Vec3::new(-1.0, phi, 0.0),
            Vec3::new(1.0, phi, 0.0),
            Vec3::new(-1.0, -phi, 0.0),
            Vec3::new(1.0, -phi, 0.0),
            Vec3::new(0.0, -1.0, phi),
            Vec3::new(0.0, 1.0, phi),
            Vec3::new(0.0, -1.0, -phi),
            Vec3::new(0.0, 1.0, -phi),
            Vec3::new(phi, 0.0, -1.0),
            Vec3::new(phi, 0.0, 1.0),
            Vec3::new(-phi, 0.0, -1.0),
            Vec3::new(-phi, 0.0, 1.0),
        ];
        let mut verts: Vec<Vec3> = raw.iter().map(|v| v.normalize()).collect();

        // 20 faces, CCW from outside (outward normals).
        let mut faces: Vec<[u32; 3]> = vec![
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];

        // Subdivide.
        for _ in 0..subdivisions {
            let mut new_faces = Vec::with_capacity(faces.len() * 4);
            let mut midpoint_cache: std::collections::HashMap<(u32, u32), u32> =
                std::collections::HashMap::new();
            let mut get_mid = |a: u32, b: u32, verts: &mut Vec<Vec3>| -> u32 {
                let key = if a < b { (a, b) } else { (b, a) };
                if let Some(&idx) = midpoint_cache.get(&key) {
                    return idx;
                }
                let m = ((verts[a as usize] + verts[b as usize]) * 0.5).normalize();
                verts.push(m);
                let idx = (verts.len() - 1) as u32;
                midpoint_cache.insert(key, idx);
                idx
            };
            for f in &faces {
                let a = f[0];
                let b = f[1];
                let c = f[2];
                let ab = get_mid(a, b, &mut verts);
                let bc = get_mid(b, c, &mut verts);
                let ca = get_mid(c, a, &mut verts);
                new_faces.push([a, ab, ca]);
                new_faces.push([b, bc, ab]);
                new_faces.push([c, ca, bc]);
                new_faces.push([ab, bc, ca]);
            }
            faces = new_faces;
        }

        // Build MeshData, scaled and translated.
        let mut vertices: Vec<MeshVertex> = Vec::with_capacity(faces.len() * 3);
        let mut indices: Vec<u32> = Vec::with_capacity(faces.len() * 3);
        for f in &faces {
            let p0 = verts[f[0] as usize] * radius + center;
            let p1 = verts[f[1] as usize] * radius + center;
            let p2 = verts[f[2] as usize] * radius + center;
            let n = (p1 - p0).cross(p2 - p0).normalize_or_zero();
            let base = vertices.len() as u32;
            vertices.push(MeshVertex {
                position: [p0.x, p0.y, p0.z],
                normal: [n.x, n.y, n.z],
            });
            vertices.push(MeshVertex {
                position: [p1.x, p1.y, p1.z],
                normal: [n.x, n.y, n.z],
            });
            vertices.push(MeshVertex {
                position: [p2.x, p2.y, p2.z],
                normal: [n.x, n.y, n.z],
            });
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
        }

        let bbox = BoundingBox {
            min: center - Vec3::splat(radius),
            max: center + Vec3::splat(radius),
        };
        let volume = (4.0 / 3.0) * std::f64::consts::PI * (radius as f64).powi(3);
        MeshData {
            vertices,
            indices,
            bbox,
            volume,
        }
    }

    #[test]
    fn bvh_builds_over_icosphere() {
        let mesh = make_icosphere(2, 1.0, Vec3::ZERO);
        let bvh = MeshBvh::build(&mesh);
        assert_eq!(bvh.triangle_count(), mesh.indices.len() / 3);
        assert!(bvh.node_count() > 1, "BVH must have at least one split");
    }

    #[test]
    fn bvh_gwn_agrees_with_naive_on_icosphere() {
        // Medium-sized mesh (320 triangles at subdivision 2). The
        // dipole approximation with β=2 has O((r/d)²) error per
        // approximated node, which at interior points accumulates to
        // a few percent — well below the 0.5 inside/outside threshold,
        // but not exact. We assert the weaker bound.
        let mesh = make_icosphere(2, 1.0, Vec3::ZERO);
        let bvh = MeshBvh::build(&mesh);
        let probes = [
            Vec3::ZERO,                  // deep interior
            Vec3::new(0.5, 0.0, 0.0),    // off-center interior
            Vec3::new(0.9, 0.3, 0.2),    // near surface, inside
            Vec3::new(1.1, 0.0, 0.0),    // just outside
            Vec3::new(3.0, 2.0, -1.0),   // far away
            Vec3::new(-5.0, -5.0, -5.0), // very far
        ];
        for p in probes {
            let naive = naive_gwn(&mesh, p);
            let fast = bvh.winding_number(p);
            assert!(
                (naive - fast).abs() < 5e-2,
                "BVH/naive mismatch at {p:?}: naive={naive}, fast={fast}"
            );
        }
    }

    #[test]
    fn bvh_gwn_is_exact_far_from_mesh() {
        // At truly far distances every node passes the β test with
        // room to spare, so the approximation error shrinks with d⁻²
        // and the BVH result converges to naive.
        let mesh = make_icosphere(2, 1.0, Vec3::ZERO);
        let bvh = MeshBvh::build(&mesh);
        for p in [
            Vec3::new(20.0, 0.0, 0.0),
            Vec3::new(-15.0, 10.0, 7.0),
            Vec3::new(0.0, 0.0, 50.0),
        ] {
            let naive = naive_gwn(&mesh, p);
            let fast = bvh.winding_number(p);
            assert!(
                (naive - fast).abs() < 1e-4,
                "far-field mismatch at {p:?}: naive={naive}, fast={fast}"
            );
        }
    }

    #[test]
    fn bvh_gwn_inside_outside_threshold_robust() {
        // The important correctness property: whatever the exact
        // value, the sign (w≥0.5) agrees with naive at all probes.
        let mesh = make_icosphere(2, 1.0, Vec3::ZERO);
        let bvh = MeshBvh::build(&mesh);
        let probes = [
            (Vec3::ZERO, true),
            (Vec3::new(0.5, 0.0, 0.0), true),
            (Vec3::new(0.9, 0.0, 0.0), true),
            (Vec3::new(1.1, 0.0, 0.0), false),
            (Vec3::new(5.0, 5.0, 5.0), false),
            (Vec3::new(-10.0, 0.0, 0.0), false),
        ];
        for (p, expected_inside) in probes {
            let naive = naive_gwn(&mesh, p);
            let fast = bvh.winding_number(p);
            assert_eq!(
                naive >= 0.5,
                expected_inside,
                "naive threshold wrong at {p:?} (w={naive})"
            );
            assert_eq!(
                fast >= 0.5,
                expected_inside,
                "BVH threshold wrong at {p:?} (w={fast})"
            );
        }
    }

    #[test]
    fn bvh_gwn_inside_outside_matches_threshold() {
        let mesh = make_icosphere(2, 1.0, Vec3::ZERO);
        let bvh = MeshBvh::build(&mesh);
        assert!(bvh.winding_number(Vec3::ZERO) > 0.9, "center ≈ 1 inside");
        assert!(
            bvh.winding_number(Vec3::new(5.0, 0.0, 0.0)).abs() < 0.05,
            "far outside ≈ 0"
        );
    }

    #[test]
    fn bvh_empty_mesh_returns_zero() {
        let mesh = MeshData {
            vertices: Vec::new(),
            indices: Vec::new(),
            bbox: BoundingBox {
                min: Vec3::ZERO,
                max: Vec3::ZERO,
            },
            volume: 0.0,
        };
        let bvh = MeshBvh::build(&mesh);
        assert_eq!(bvh.node_count(), 0);
        assert_eq!(bvh.winding_number(Vec3::ZERO), 0.0);
    }
}
