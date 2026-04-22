//! Per-vertex classification used by feature-aware passes.
//!
//! Each vertex of a [`HalfEdgeMesh`](super::half_edge::HalfEdgeMesh) carries
//! a [`VertexClass`] that controls how passes treat it. Smoothing skips
//! `Fixed` vertices outright, slides `Boundary` and `Feature` vertices along
//! tangents only, and lets `Interior` vertices move freely. Simplification
//! refuses collapses that would merge vertices of incompatible class
//! (e.g. across two distinct boundary loops).
//!
//! v1 of the half-edge mesh stores all vertices as `Interior`. The
//! [`VertexClassifier`] populates the actual classes from mesh topology
//! plus an optional [`FeatureSet`](super::features::FeatureSet) — typically
//! invoked once after construction and again after any pass that mutates
//! connectivity (signalled via [`MeshRepairPass::reclassifies`](super::pass::MeshRepairPass::reclassifies)).

use glam::Vec3;

use super::features::FeatureSet;
use super::half_edge::{FaceId, HalfEdgeMesh, VertexId};

/// How a vertex should be treated by repair passes.
///
/// Higher discriminants take priority on conflict (see
/// [`combine`](Self::combine) and [`stronger`](Self::stronger)).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum VertexClass {
    /// Free interior vertex; smoothers and collapses move it without
    /// constraint.
    #[default]
    Interior,
    /// On a feature crease (dihedral angle exceeds threshold). Passes may
    /// move it tangentially along the crease but must not cross it.
    Feature,
    /// On a boundary loop. Passes preserve boundary topology — tangential
    /// motion only, collapses across boundary loops blocked.
    Boundary,
    /// User-pinned or artificial vertex (e.g. centroid added by hole fill).
    /// Passes treat as Dirichlet-fixed.
    Fixed,
}

impl VertexClass {
    /// Returns the stronger of two classes (Fixed > Boundary > Feature > Interior).
    pub fn stronger(a: Self, b: Self) -> Self {
        if a >= b { a } else { b }
    }

    /// Resolves the class of a vertex that's just been merged from `a` and `b`.
    /// Equivalent to [`stronger`](Self::stronger); kept as a named alias because
    /// callers typically reach for "what class does the merged vertex have?".
    pub fn combine(a: Self, b: Self) -> Self {
        Self::stronger(a, b)
    }
}

/// Populates the per-vertex class of a [`HalfEdgeMesh`] from mesh topology.
///
/// Boundary half-edges classify their endpoints as [`VertexClass::Boundary`];
/// edges where the two adjacent face normals diverge by more than
/// [`feature_dihedral_deg`](Self::feature_dihedral_deg) classify their
/// endpoints as [`VertexClass::Feature`]. The `pin_*` flags upgrade those
/// to [`VertexClass::Fixed`]. `user_pinned` vertices are always Fixed.
///
/// On conflict, the stronger class wins (see [`VertexClass::combine`]).
#[derive(Debug, Clone)]
pub struct VertexClassifier {
    /// Dihedral threshold in degrees: edges with `acos(n1·n2) > threshold`
    /// are flagged as feature creases. Default 45°.
    pub feature_dihedral_deg: f32,
    /// Promote `Boundary` classifications to `Fixed`. Default `true`.
    pub pin_boundary: bool,
    /// Promote `Feature` classifications to `Fixed`. Default `false` —
    /// features should slide along their crease, not freeze.
    pub pin_features: bool,
    /// Vertices that are unconditionally pinned to `Fixed` regardless of
    /// topology.
    pub user_pinned: Vec<VertexId>,
}

impl Default for VertexClassifier {
    fn default() -> Self {
        Self {
            feature_dihedral_deg: 45.0,
            pin_boundary: true,
            pin_features: false,
            user_pinned: Vec::new(),
        }
    }
}

impl VertexClassifier {
    /// Resets every live vertex to `Interior`, then walks topology to mark
    /// `Boundary` and `Feature` classes, then applies pins. No explicit
    /// feature set — purely topological.
    pub fn classify(&self, mesh: &mut HalfEdgeMesh) {
        self.classify_with(mesh, None);
    }

    /// Same as [`classify`](Self::classify) but consults an optional
    /// [`FeatureSet`] for caller-supplied creases and pinned vertices. The
    /// declared creases mark their endpoints `Feature` *before* dihedral
    /// detection runs, and the pinned vertices are upgraded to `Fixed` at
    /// the end alongside `user_pinned`.
    pub fn classify_with(&self, mesh: &mut HalfEdgeMesh, features: Option<&FeatureSet>) {
        // 1. Reset live vertices to Interior.
        let n_verts = mesh.vertices.len();
        for vi in 0..n_verts {
            let vid = VertexId(vi as u32);
            if mesh.vertex_is_live(vid) {
                mesh.set_vertex_class(vid, VertexClass::Interior);
            }
        }

        // 1.5. Caller-declared creases come first; combine() means later
        // dihedral / boundary marks can only upgrade, never downgrade.
        if let Some(fs) = features {
            for (u, v) in fs.creases() {
                if mesh.vertex_is_live(u) {
                    set_at_least(mesh, u, VertexClass::Feature);
                }
                if mesh.vertex_is_live(v) {
                    set_at_least(mesh, v, VertexClass::Feature);
                }
            }
        }

        // 2. Feature edges (dihedral > threshold). Use acos(dot) directly:
        // 0 for flat junctions, π for fully folded — so a 45° threshold is a
        // 45° crease, regardless of which way the surface folds.
        // Collect feature endpoints first so the immutable borrow on
        // mesh.edge_iter ends before we mutate vertex_class.
        let threshold_rad = self.feature_dihedral_deg.to_radians();
        let mut feature_endpoints: Vec<VertexId> = Vec::new();
        for h in mesh.edge_iter() {
            let twin = mesh.he_twin(h);
            if !twin.is_valid() {
                continue; // boundary edge — handled in step 3
            }
            let n1 = face_normal(mesh, mesh.he_face(h));
            let n2 = face_normal(mesh, mesh.he_face(twin));
            let cos = n1.dot(n2).clamp(-1.0, 1.0);
            let angle = cos.acos();
            if angle > threshold_rad {
                feature_endpoints.push(mesh.he_tail(h));
                feature_endpoints.push(mesh.he_head(h));
            }
        }
        for v in feature_endpoints {
            set_at_least(mesh, v, VertexClass::Feature);
        }

        // 3. Boundary half-edges → mark both endpoints Boundary (overrides
        // Feature via combine()).
        for hi in 0..mesh.half_edges.len() {
            let h_rec = &mesh.half_edges[hi];
            if h_rec.removed || h_rec.twin.is_valid() {
                continue;
            }
            let h = super::half_edge::HalfEdgeId(hi as u32);
            let u = mesh.he_tail(h);
            let v = mesh.he_head(h);
            set_at_least(mesh, u, VertexClass::Boundary);
            set_at_least(mesh, v, VertexClass::Boundary);
        }

        // 4. Promote per pinning flags.
        if self.pin_boundary || self.pin_features {
            for vi in 0..n_verts {
                let vid = VertexId(vi as u32);
                if !mesh.vertex_is_live(vid) {
                    continue;
                }
                let cur = mesh.vertex_class(vid);
                let promoted = match cur {
                    VertexClass::Boundary if self.pin_boundary => VertexClass::Fixed,
                    VertexClass::Feature if self.pin_features => VertexClass::Fixed,
                    _ => cur,
                };
                if promoted != cur {
                    mesh.set_vertex_class(vid, promoted);
                }
            }
        }

        // 5. User pins always win — both classifier-level and feature-set-level.
        for &vid in &self.user_pinned {
            if mesh.vertex_is_live(vid) {
                mesh.set_vertex_class(vid, VertexClass::Fixed);
            }
        }
        if let Some(fs) = features {
            for vid in fs.pinned() {
                if mesh.vertex_is_live(vid) {
                    mesh.set_vertex_class(vid, VertexClass::Fixed);
                }
            }
        }
    }
}

fn face_normal(mesh: &HalfEdgeMesh, f: FaceId) -> Vec3 {
    let [p0, p1, p2] = mesh.face_positions(f);
    (p1 - p0).cross(p2 - p0).normalize_or_zero()
}

fn set_at_least(mesh: &mut HalfEdgeMesh, v: VertexId, c: VertexClass) {
    let combined = VertexClass::combine(mesh.vertex_class(v), c);
    mesh.set_vertex_class(v, combined);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_interior() {
        assert_eq!(VertexClass::default(), VertexClass::Interior);
    }

    #[test]
    fn stronger_picks_higher_priority() {
        assert_eq!(
            VertexClass::stronger(VertexClass::Interior, VertexClass::Boundary),
            VertexClass::Boundary
        );
        assert_eq!(
            VertexClass::stronger(VertexClass::Boundary, VertexClass::Fixed),
            VertexClass::Fixed
        );
        assert_eq!(
            VertexClass::stronger(VertexClass::Feature, VertexClass::Boundary),
            VertexClass::Boundary
        );
        assert_eq!(
            VertexClass::stronger(VertexClass::Feature, VertexClass::Interior),
            VertexClass::Feature
        );
    }

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

    fn cube() -> HalfEdgeMesh {
        // Standard 8-vertex cube; six faces, each split into two triangles.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ];
        // CCW outward winding for all six faces.
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 1,  0, 3, 2, // -z bottom
            0, 1, 5,  0, 5, 4, // -y front
            1, 2, 6,  1, 6, 5, // +x right
            2, 3, 7,  2, 7, 6, // +y back
            3, 0, 4,  3, 4, 7, // -x left
            4, 5, 6,  4, 6, 7, // +z top
        ];
        HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("cube builds")
    }

    #[test]
    fn classify_marks_cube_corners_as_features() {
        let mut mesh = cube();
        let classifier = VertexClassifier::default();
        classifier.classify(&mut mesh);
        // Cube edges all have 90° dihedral → > 45°, so every cube vertex is a
        // feature. With pin_boundary=true (default), boundaries would override
        // — but a closed cube has no boundary, so all 8 corners stay Feature.
        for v in 0u32..8 {
            assert_eq!(mesh.vertex_class(VertexId(v)), VertexClass::Feature);
        }
    }

    #[test]
    fn classify_marks_open_cube_face_boundary_as_fixed() {
        // Cube with the +z face removed → 4 boundary vertices on the top.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ];
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 1,  0, 3, 2,
            0, 1, 5,  0, 5, 4,
            1, 2, 6,  1, 6, 5,
            2, 3, 7,  2, 7, 6,
            3, 0, 4,  3, 4, 7,
        ];
        let mut mesh =
            HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("open cube builds");
        let classifier = VertexClassifier::default();
        classifier.classify(&mut mesh);
        // Vertices 4..8 are on the +z boundary loop → Boundary, then Fixed
        // because pin_boundary defaults to true.
        for v in 4u32..8 {
            assert_eq!(mesh.vertex_class(VertexId(v)), VertexClass::Fixed);
        }
        // Vertices 0..4 are interior corners (cube edges meeting only
        // bottom/side faces) — they're feature corners.
        for v in 0u32..4 {
            assert_eq!(mesh.vertex_class(VertexId(v)), VertexClass::Feature);
        }
    }

    #[test]
    fn classify_user_pinned_overrides_topology() {
        let mut mesh = cube();
        let mut classifier = VertexClassifier::default();
        classifier.user_pinned = vec![VertexId(0)];
        classifier.classify(&mut mesh);
        assert_eq!(mesh.vertex_class(VertexId(0)), VertexClass::Fixed);
        assert_eq!(mesh.vertex_class(VertexId(1)), VertexClass::Feature);
    }

    #[test]
    fn classify_smooth_sphere_has_no_features() {
        // A coarse tetrahedron is the simplest closed surface, but its edges
        // have dihedral > 45° (acute corners). We just check that with a high
        // threshold (179°), nothing is flagged Feature.
        let iso = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        );
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso).expect("tet");
        let classifier = VertexClassifier {
            feature_dihedral_deg: 179.0,
            ..VertexClassifier::default()
        };
        classifier.classify(&mut mesh);
        for v in 0u32..4 {
            assert_eq!(mesh.vertex_class(VertexId(v)), VertexClass::Interior);
        }
    }

    #[test]
    fn featureset_creases_promote_interior_vertices() {
        // Build a closed surface (tetrahedron-derived) and use a high
        // dihedral threshold so nothing else qualifies as Feature. The
        // FeatureSet then provides the only source of Feature classification.
        // Use the tetrahedron — its edges have dihedral well below 179°.
        let iso = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        );
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso).expect("tet");
        let mut fs = FeatureSet::new();
        fs.add_crease(VertexId(0), VertexId(1));
        let classifier = VertexClassifier {
            feature_dihedral_deg: 179.0,
            pin_boundary: false,
            ..VertexClassifier::default()
        };
        classifier.classify_with(&mut mesh, Some(&fs));
        // Tetrahedron is closed — no boundary promotions to override.
        // 0 and 1 are on the declared crease.
        assert_eq!(mesh.vertex_class(VertexId(0)), VertexClass::Feature);
        assert_eq!(mesh.vertex_class(VertexId(1)), VertexClass::Feature);
        // 2 and 3 are not on the crease and dihedral is below threshold.
        assert_eq!(mesh.vertex_class(VertexId(2)), VertexClass::Interior);
        assert_eq!(mesh.vertex_class(VertexId(3)), VertexClass::Interior);
    }

    #[test]
    fn featureset_pinned_vertices_become_fixed() {
        let mut mesh = cube();
        let mut fs = FeatureSet::new();
        fs.pin(VertexId(3));
        let classifier = VertexClassifier {
            pin_boundary: false,
            pin_features: false,
            ..VertexClassifier::default()
        };
        classifier.classify_with(&mut mesh, Some(&fs));
        assert_eq!(mesh.vertex_class(VertexId(3)), VertexClass::Fixed);
    }

    #[test]
    fn classify_pin_features_promotes_to_fixed() {
        let mut mesh = cube();
        let classifier = VertexClassifier {
            pin_features: true,
            ..VertexClassifier::default()
        };
        classifier.classify(&mut mesh);
        for v in 0u32..8 {
            assert_eq!(mesh.vertex_class(VertexId(v)), VertexClass::Fixed);
        }
    }

    #[test]
    fn combine_is_alias_for_stronger() {
        for a in [
            VertexClass::Interior,
            VertexClass::Feature,
            VertexClass::Boundary,
            VertexClass::Fixed,
        ] {
            for b in [
                VertexClass::Interior,
                VertexClass::Feature,
                VertexClass::Boundary,
                VertexClass::Fixed,
            ] {
                assert_eq!(VertexClass::combine(a, b), VertexClass::stronger(a, b));
            }
        }
    }
}
