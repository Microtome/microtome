//! Tangent-direction helpers for class-aware passes.
//!
//! Reprojection and smoothing of `Boundary` / `Feature` vertices must move them
//! *along* the loop / crease, never off it. These helpers compute the local
//! tangent line from mesh topology so callers can project displacements onto
//! it. Returning `None` means "no well-defined tangent" — callers should fall
//! back to skipping the vertex rather than substituting a default direction.

use glam::Vec3;

use super::half_edge::{FaceId, HalfEdgeId, HalfEdgeMesh, VertexId};
use super::vertex_class::VertexClassifier;

/// Returns the unit tangent along the boundary loop at `v`, or `None` if `v`
/// is not on a boundary or the local boundary is degenerate (sharp 180°
/// reversal where the in/out directions cancel).
pub fn boundary_tangent(mesh: &HalfEdgeMesh, v: VertexId) -> Option<Vec3> {
    let out = mesh.vertex_he_out(v);
    if !out.is_valid() || mesh.he_twin(out).is_valid() {
        return None;
    }
    let into = boundary_in_he(mesh, out)?;
    let pos_v = mesh.vertex_position(v);
    let to_next = (mesh.vertex_position(mesh.he_head(out)) - pos_v).normalize_or_zero();
    let from_prev = (pos_v - mesh.vertex_position(mesh.he_tail(into))).normalize_or_zero();
    let t = (to_next + from_prev).normalize_or_zero();
    if t == Vec3::ZERO { None } else { Some(t) }
}

/// Returns the unit tangent along the feature crease at `v`, or `None` if `v`
/// has !=2 incident feature edges (corner with 3+, dangling with 1, or non-feature).
///
/// Reads the dihedral threshold from `classifier` so the edges flagged here
/// always agree with the per-vertex classification — the previous loose-float
/// parameter let callers pass a stale threshold and silently disagree with
/// the classifier.
pub fn feature_tangent(
    mesh: &HalfEdgeMesh,
    v: VertexId,
    classifier: &VertexClassifier,
) -> Option<Vec3> {
    let pos_v = mesh.vertex_position(v);
    let threshold_rad = classifier.feature_dihedral_deg.to_radians();
    let mut crease_neighbours: Vec<Vec3> = Vec::new();
    visit_outgoing(mesh, v, |h| {
        let twin = mesh.he_twin(h);
        if !twin.is_valid() {
            return;
        }
        let n1 = face_normal(mesh, mesh.he_face(h));
        let n2 = face_normal(mesh, mesh.he_face(twin));
        let cos = n1.dot(n2).clamp(-1.0, 1.0);
        if cos.acos() > threshold_rad {
            crease_neighbours.push(mesh.vertex_position(mesh.he_head(h)) - pos_v);
        }
    });
    if crease_neighbours.len() != 2 {
        return None;
    }
    let d0 = crease_neighbours[0].normalize_or_zero();
    let d1 = crease_neighbours[1].normalize_or_zero();
    // The two crease neighbours sit on opposite sides of v along the crease,
    // so d0 and d1 are roughly anti-parallel; (d0 - d1) gives the line
    // direction. Degenerate case (d0 ≈ d1) means a sharp U-turn — treat as
    // no usable tangent.
    let t = (d0 - d1).normalize_or_zero();
    if t == Vec3::ZERO { None } else { Some(t) }
}

/// Projects `disp` onto the line spanned by `tangent`. Returns the
/// component-along-tangent vector. Zero tangent yields zero output.
pub fn project_onto_tangent(disp: Vec3, tangent: Vec3) -> Vec3 {
    let t = tangent.normalize_or_zero();
    if t == Vec3::ZERO {
        Vec3::ZERO
    } else {
        t * disp.dot(t)
    }
}

fn face_normal(mesh: &HalfEdgeMesh, f: FaceId) -> Vec3 {
    let [p0, p1, p2] = mesh.face_positions(f);
    (p1 - p0).cross(p2 - p0).normalize_or_zero()
}

/// Walks the boundary loop forward from `start` until it would cycle back,
/// returning the predecessor of `start` (i.e. the boundary half-edge whose
/// head is `mesh.he_tail(start)`).
fn boundary_in_he(mesh: &HalfEdgeMesh, start: HalfEdgeId) -> Option<HalfEdgeId> {
    debug_assert!(!mesh.he_twin(start).is_valid());
    let bound = mesh.half_edge_count() + 1;
    let mut current = start;
    for _ in 0..bound {
        let next = mesh.next_boundary_he(current);
        if next == start {
            return Some(current);
        }
        current = next;
    }
    None
}

/// Visits each outgoing half-edge of `v` once, in cyclic order. Terminates at
/// the first boundary fence (does *not* wrap across it).
fn visit_outgoing(mesh: &HalfEdgeMesh, v: VertexId, mut f: impl FnMut(HalfEdgeId)) {
    let start = mesh.vertex_he_out(v);
    if !start.is_valid() {
        return;
    }
    let bound = mesh.half_edge_count() + 1;
    let mut current = start;
    let mut first = true;
    for _ in 0..bound {
        f(current);
        let prev = mesh.he_prev(current);
        let prev_twin = mesh.he_twin(prev);
        if !prev_twin.is_valid() {
            break;
        }
        if !first && prev_twin == start {
            break;
        }
        current = prev_twin;
        first = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;
    use crate::mesh_repair::vertex_class::VertexClassifier;

    fn iso(positions: Vec<Vec3>, indices: Vec<u32>) -> IsoMesh {
        let n = positions.len();
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; n],
            indices,
        }
    }

    #[test]
    fn boundary_tangent_on_open_quad_runs_along_loop() {
        // A 2-triangle quad in z=0; boundary is the four outer edges. Vertex 1
        // is on the boundary (corner of the quad). Tangent should lie in the
        // x/y plane.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(2.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        // Two strip triangles: (0,1,4) and (1,3,4) and (1,2,3)
        let indices = vec![0, 1, 4, 1, 3, 4, 1, 2, 3];
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("strip");
        // Vertex 1 sits on the bottom boundary between vertex 0 (left) and 2 (right).
        let t = boundary_tangent(&mesh, VertexId(1)).expect("tangent");
        assert!(
            t.z.abs() < 1e-5,
            "boundary tangent should lie in the surface plane, got {t:?}"
        );
        // Should be along ±x.
        assert!(
            t.x.abs() > 0.95,
            "boundary tangent should be ~±x at vertex 1, got {t:?}"
        );
    }

    #[test]
    fn feature_tangent_on_cube_edge_midpoint() {
        // Build two flat triangles meeting at a 90° crease so the shared edge
        // is a feature edge. Vertex 1 sits in the middle of the crease.
        //   p0--p1--p2     (z=0 plane)
        //    \  ||  /
        //     \ || /
        //   p3--p4--p5     (z=0, y=-1) — folded under to make 90°
        // We want a vertex on the crease with two feature edges going either
        // direction along the crease. Simplest: a vertical wall folding into a
        // floor along the y-axis at x = 0.
        let positions = vec![
            // Floor (z = 0)
            Vec3::new(-1.0, 0.0, 0.0),  // 0
            Vec3::new(0.0, 0.0, 0.0),   // 1: crease vertex (on +y leg)
            Vec3::new(-1.0, 1.0, 0.0),  // 2
            Vec3::new(0.0, 1.0, 0.0),   // 3: crease vertex (next along crease)
            Vec3::new(-1.0, -1.0, 0.0), // 4
            Vec3::new(0.0, -1.0, 0.0),  // 5: crease vertex (back along crease)
            // Wall (y = 0, z = +)
            Vec3::new(0.0, 0.0, 1.0),  // 6
            Vec3::new(0.0, 1.0, 1.0),  // 7
            Vec3::new(0.0, -1.0, 1.0), // 8
        ];
        // Floor strip on -x side meeting wall at x=0.
        // Floor triangles (CCW outward = +z normal):
        //   0,1,2  1,3,2  4,5,1  4,1,0
        // Wall triangles (CCW outward = -y normal, so winding looks like -y):
        // The wall sits at y=0 with x∈[0,0] which is degenerate — let's place
        // the wall over the +x side. Reposition: set wall vertices at x=0 to
        // x=0 but extend to x=1 too...
        //
        // Actually simpler: keep the wall as a strip at x=0, y ∈ [-1,1], z ∈ [0,1].
        // Floor sits at z=0, x ∈ [-1,0], y ∈ [-1,1].
        // The shared crease is the line x=0, z=0, y ∈ [-1,1] — i.e.
        // vertices 1 (y=0), 3 (y=1), 5 (y=-1).
        //
        // Wall has 4 vertices: (0,-1,0)=5, (0,0,0)=1, (0,1,0)=3, (0,-1,1)=8,
        // (0,0,1)=6, (0,1,1)=7. Triangles: 5,1,8 / 1,6,8 / 1,3,6 / 3,7,6.
        // Outward normal (-y, since wall is at y=0 facing -y? Actually we want
        // a 90° crease — wall facing +x, normal +x). Reposition wall vertices
        // to x=0 means wall is the y/z plane; normal is ±x. Floor normal is
        // ±z. Dihedral 90°.
        let indices = vec![
            // Floor (-x half-plane), z=0, normal +z. Triangles must be CCW
            // when viewed from +z.
            0, 1, 3, 0, 3, 2, 4, 5, 1, 4, 1, 0,
            // Wall at x=0, normal +x. CCW when viewed from +x:
            // y goes: 5(-1)→1(0)→3(+1) along bottom; 8→6→7 along top.
            5, 8, 1, 8, 6, 1, 1, 6, 3, 6, 7, 3,
        ];
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("crease");
        // Reach into VertexClassifier so we use its threshold (45° default).
        let classifier = VertexClassifier::default();
        let t = feature_tangent(&mesh, VertexId(1), &classifier)
            .expect("vertex 1 has two feature edges along the crease (y direction)");
        assert!(
            t.x.abs() < 1e-3 && t.z.abs() < 1e-3,
            "feature tangent should be ±y, got {t:?}"
        );
        assert!(t.y.abs() > 0.95);
    }

    #[test]
    fn project_onto_tangent_keeps_along_component() {
        let disp = Vec3::new(2.0, 1.0, 0.5);
        let tangent = Vec3::X;
        let projected = project_onto_tangent(disp, tangent);
        assert!((projected - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn project_onto_zero_tangent_returns_zero() {
        let disp = Vec3::new(1.0, 2.0, 3.0);
        let projected = project_onto_tangent(disp, Vec3::ZERO);
        assert_eq!(projected, Vec3::ZERO);
    }
}
