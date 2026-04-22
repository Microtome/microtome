//! Garland-Heckbert per-vertex quadric matrices.
//!
//! Each [`VertexQuadric`] wraps a [`QefSolver`] accumulating face-plane,
//! boundary-plane, and feature-plane constraints from a vertex's
//! incident geometry. Edge-collapse cost is `(Q_u + Q_v).evaluate(p_opt)`;
//! `p_opt` is the QEF solver's optimum.
//!
//! Weights:
//! - face planes: weight 1
//! - boundary planes: weight `k_boundary × avg_face_area` (default 1000×)
//! - feature planes: weight `k_feature × avg_face_area` (default 100×)
//!
//! These are the classic Garland-Heckbert tunings; collapses across
//! boundaries / features pay a large penalty proportional to the
//! local mesh density, so they happen only when there's no alternative.

use glam::Vec3;

use super::half_edge::{FaceId, HalfEdgeMesh};
use super::vertex_class::VertexClass;
use crate::isosurface::QefSolver;

/// Per-vertex quadric for Garland-Heckbert simplification.
#[derive(Debug, Clone)]
pub struct VertexQuadric {
    /// Underlying solver. Public so callers can read raw QEF state for
    /// diagnostics.
    pub q: QefSolver,
}

impl Default for VertexQuadric {
    fn default() -> Self {
        Self::new()
    }
}

impl VertexQuadric {
    /// Constructs a fresh empty quadric.
    pub fn new() -> Self {
        Self {
            q: QefSolver::new(),
        }
    }

    /// Adds a face-plane constraint (weight 1).
    pub fn add_face_plane(&mut self, normal: Vec3, d: f32) {
        self.q.add_plane(normal, d, 1.0);
    }

    /// Adds a boundary-plane constraint with the given weight.
    pub fn add_boundary_plane(&mut self, normal: Vec3, d: f32, weight: f32) {
        self.q.add_plane(normal, d, weight);
    }

    /// Adds a feature-plane constraint with the given weight.
    pub fn add_feature_plane(&mut self, normal: Vec3, d: f32, weight: f32) {
        self.q.add_plane(normal, d, weight);
    }

    /// Merges another quadric in-place.
    pub fn combine(&mut self, other: &VertexQuadric) {
        self.q.combine(&other.q);
    }

    /// Evaluates the quadric at `p` — the squared sum of weighted
    /// plane-distance residuals. Used as an edge-collapse cost.
    pub fn evaluate(&self, p: Vec3) -> f32 {
        self.q.evaluate(p)
    }

    /// Solves for the optimum vertex position `p* = argmin Q(p)` and
    /// returns it together with `Q(p*)`.
    pub fn solve(&mut self) -> (Vec3, f32) {
        let mut p = self.q.mass_point();
        let mut err = 0.0;
        self.q.solve(&mut p, &mut err);
        (p, err)
    }
}

/// Tuning knobs for [`accumulate_for_mesh`].
#[derive(Debug, Clone, Copy)]
pub struct QuadricWeights {
    /// Multiplier for boundary-plane constraints (×avg face area).
    pub boundary: f32,
    /// Multiplier for feature-plane constraints (×avg face area).
    pub feature: f32,
}

impl Default for QuadricWeights {
    fn default() -> Self {
        Self {
            boundary: 1000.0,
            feature: 100.0,
        }
    }
}

/// Builds per-vertex quadrics for an entire half-edge mesh.
///
/// The returned `Vec` is indexed by `VertexId.index()`. Removed vertices
/// get an empty quadric (zero solver state). Boundary and feature
/// classification is read from `mesh.vertex_class`; the caller is
/// expected to have run [`VertexClassifier`](super::vertex_class::VertexClassifier)
/// first.
pub fn accumulate_for_mesh(mesh: &HalfEdgeMesh, weights: QuadricWeights) -> Vec<VertexQuadric> {
    let mut quadrics: Vec<VertexQuadric> = (0..mesh.vertices.len())
        .map(|_| VertexQuadric::new())
        .collect();

    // Pass 1: face planes contribute to all 3 incident vertex quadrics.
    let mut total_area = 0.0_f32;
    let mut face_area_count = 0u32;
    for fi in 0..mesh.faces.len() {
        let face = &mesh.faces[fi];
        if face.removed {
            continue;
        }
        let fid = FaceId(fi as u32);
        let [p0, p1, p2] = mesh.face_positions(fid);
        let normal = (p1 - p0).cross(p2 - p0);
        let area = normal.length() * 0.5;
        total_area += area;
        face_area_count += 1;
        let n = normal.normalize_or_zero();
        if n == Vec3::ZERO {
            continue;
        }
        let d = n.dot(p0);
        for v in mesh.face_vertices(fid) {
            quadrics[v.index()].add_face_plane(n, d);
        }
    }

    let avg_area = if face_area_count == 0 {
        1.0
    } else {
        total_area / face_area_count as f32
    };
    let boundary_weight = weights.boundary * avg_area;
    let feature_weight = weights.feature * avg_area;

    // Pass 2: boundary edges. For each boundary half-edge (twin INVALID),
    // construct a "boundary plane" perpendicular to the incident face's
    // normal and containing the edge. This penalises collapses that move
    // the boundary off its current line.
    for hi in 0..mesh.half_edges.len() {
        let h = &mesh.half_edges[hi];
        if h.removed || h.twin.is_valid() {
            continue;
        }
        let hid = super::half_edge::HalfEdgeId(hi as u32);
        let face = mesh.he_face(hid);
        if !mesh.face_is_live(face) {
            continue;
        }
        let [p0, p1, p2] = mesh.face_positions(face);
        let face_normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
        if face_normal == Vec3::ZERO {
            continue;
        }
        let tail = mesh.he_tail(hid);
        let head = mesh.he_head(hid);
        let edge_dir =
            (mesh.vertex_position(head) - mesh.vertex_position(tail)).normalize_or_zero();
        if edge_dir == Vec3::ZERO {
            continue;
        }
        let boundary_normal = face_normal.cross(edge_dir).normalize_or_zero();
        if boundary_normal == Vec3::ZERO {
            continue;
        }
        let d = boundary_normal.dot(mesh.vertex_position(tail));
        quadrics[tail.index()].add_boundary_plane(boundary_normal, d, boundary_weight);
        quadrics[head.index()].add_boundary_plane(boundary_normal, d, boundary_weight);
    }

    // Pass 3: feature edges. For every interior edge whose endpoints are
    // both classified Feature (or Fixed), add the two adjacent face planes
    // to the endpoints' quadrics with feature weight. Collapsing across
    // such a crease costs both plane constraints; collapsing along it
    // leaves one of them satisfied.
    for h in mesh.edge_iter() {
        let twin = mesh.he_twin(h);
        if !twin.is_valid() {
            continue;
        }
        let u = mesh.he_tail(h);
        let v = mesh.he_head(h);
        if !is_feature(mesh.vertex_class(u)) || !is_feature(mesh.vertex_class(v)) {
            continue;
        }
        let f1 = mesh.he_face(h);
        let f2 = mesh.he_face(twin);
        for f in [f1, f2] {
            if !mesh.face_is_live(f) {
                continue;
            }
            let [p0, p1, p2] = mesh.face_positions(f);
            let n = (p1 - p0).cross(p2 - p0).normalize_or_zero();
            if n == Vec3::ZERO {
                continue;
            }
            let d = n.dot(p0);
            quadrics[u.index()].add_feature_plane(n, d, feature_weight);
            quadrics[v.index()].add_feature_plane(n, d, feature_weight);
        }
    }

    quadrics
}

fn is_feature(c: VertexClass) -> bool {
    matches!(c, VertexClass::Feature | VertexClass::Fixed)
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
    fn quadric_evaluates_zero_on_added_plane() {
        let mut q = VertexQuadric::new();
        q.add_face_plane(Vec3::Z, 0.0);
        let on_plane = Vec3::new(2.0, -3.0, 0.0);
        assert!(q.evaluate(on_plane).abs() < 1e-4);
    }

    #[test]
    fn quadric_combine_is_additive() {
        let mut a = VertexQuadric::new();
        let mut b = VertexQuadric::new();
        a.add_face_plane(Vec3::Z, 0.0);
        b.add_face_plane(Vec3::X, 0.0);
        a.combine(&b);
        // (1, 0, 1) is at distance 1 from each plane → sum 2.
        assert!((a.evaluate(Vec3::new(1.0, 0.0, 1.0)) - 2.0).abs() < 1e-4);
    }

    #[test]
    fn quadric_solves_for_three_planes_intersection() {
        let mut q = VertexQuadric::new();
        // Three orthogonal planes intersecting at (1, 2, 3).
        q.add_face_plane(Vec3::X, 1.0);
        q.add_face_plane(Vec3::Y, 2.0);
        q.add_face_plane(Vec3::Z, 3.0);
        let (p, err) = q.solve();
        assert!((p - Vec3::new(1.0, 2.0, 3.0)).length() < 1e-3);
        assert!(err < 1e-3);
    }

    #[test]
    fn accumulate_for_mesh_assigns_face_planes_to_each_vertex() {
        // Single triangle in the z=0 plane. All three vertices should have
        // a quadric whose evaluate at z=1 returns 1 (squared distance).
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        ))
        .expect("build");
        let quadrics = accumulate_for_mesh(&mesh, QuadricWeights::default());
        for v in 0u32..3 {
            // Note: each vertex also receives boundary-plane contributions
            // because the triangle's edges are all boundary. So we test
            // along the +z direction where the boundary planes (in-plane
            // perpendiculars) don't contribute.
            // The face plane is z=0, evaluated at z=2 should give ≥ 4
            // (the face plane contributes 4; boundary planes are vertical
            // around the triangle and don't contribute at z=2).
            let val = quadrics[v as usize].evaluate(Vec3::new(0.5, 0.5, 2.0));
            assert!(
                val >= 4.0 - 1e-3,
                "vertex {v} quadric eval = {val}, expected ≥ 4 (face-plane contribution)"
            );
        }
    }

    #[test]
    fn boundary_planes_added_only_for_boundary_edges() {
        // Tetrahedron: closed surface, no boundary edges.
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        ))
        .expect("build");
        let quadrics = accumulate_for_mesh(&mesh, QuadricWeights::default());
        // Sanity: each vertex has accumulated 3 face planes (incident faces).
        for v in 0u32..4 {
            assert_eq!(quadrics[v as usize].q.point_count(), 3);
        }
    }
}
