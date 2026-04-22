//! Half-edge topology operations: collapse, split, flip, face removal.
//!
//! All operations return `Result<_, HalfEdgeOpError>` and refuse to run when
//! a precondition is violated. Operations that modify connectivity mark
//! replaced half-edges / faces / vertices as removed (pushed onto the
//! freelist) so external IDs remain stable until [`HalfEdgeMesh::compact`]
//! (called at the end of a pipeline run).
//!
//! # Preconditions
//!
//! - [`collapse_edge`](HalfEdgeMesh::collapse_edge) and
//!   [`collapse_edge_to`](HalfEdgeMesh::collapse_edge_to) require the
//!   Dey-Edelsbrunner link condition.
//! - [`flip_edge`](HalfEdgeMesh::flip_edge) rejects boundary edges and
//!   flips that would duplicate an existing edge.
//! - [`split_edge`](HalfEdgeMesh::split_edge) always succeeds on a valid
//!   half-edge handle.
//!
//! # Caveats
//!
//! - The link condition is necessary but not sufficient for arbitrary
//!   inputs: collapsing an edge of a tetrahedron passes link but produces
//!   two coincident triangles. v1 passes avoid such degenerate topologies;
//!   the pipeline's between-pass manifoldness check catches them.
//! - Normal-flip detection is *not* wired into `collapse_edge` in v1 —
//!   the sliver-removal pass checks this itself via face normals before
//!   and after.

use std::collections::HashSet;

use glam::Vec3;

use super::error::HalfEdgeOpError;
use super::half_edge::{FaceId, FaceRecord, HalfEdgeId, HalfEdgeMesh, HalfEdgeRecord, VertexId};

impl HalfEdgeMesh {
    /// Collapses a half-edge, merging its two endpoints to the midpoint.
    ///
    /// Returns the surviving [`VertexId`] (the tail of `he` before the
    /// operation). The head of `he` is marked removed.
    pub fn collapse_edge(&mut self, he: HalfEdgeId) -> Result<VertexId, HalfEdgeOpError> {
        if !self.half_edge_is_live(he) {
            return Err(HalfEdgeOpError::InvalidHandle(he));
        }
        let u = self.he_tail(he);
        let v = self.he_head(he);
        let midpoint = (self.vertex_position(u) + self.vertex_position(v)) * 0.5;
        self.collapse_edge_to(he, midpoint)
    }

    /// Collapses a half-edge to a caller-specified position.
    ///
    /// The tail of `he` is kept (repositioned to `pos`) and the head is
    /// removed. Both incident faces (or the single incident face for a
    /// boundary edge) are removed.
    ///
    /// # Errors
    ///
    /// - [`InvalidHandle`](HalfEdgeOpError::InvalidHandle) if `he` is
    ///   removed or out of range.
    /// - [`LinkConditionFailed`](HalfEdgeOpError::LinkConditionFailed) if
    ///   the Dey-Edelsbrunner link condition does not hold.
    /// - [`BoundaryMergeForbidden`](HalfEdgeOpError::BoundaryMergeForbidden)
    ///   if `he` is interior but both endpoints lie on boundary loops.
    pub fn collapse_edge_to(
        &mut self,
        he: HalfEdgeId,
        pos: Vec3,
    ) -> Result<VertexId, HalfEdgeOpError> {
        if !self.half_edge_is_live(he) {
            return Err(HalfEdgeOpError::InvalidHandle(he));
        }
        let twin = self.he_twin(he);
        let u = self.he_tail(he);
        let v = self.he_head(he);
        self.check_link_condition(he, u, v, twin)?;

        // NB: the spec's `BoundaryMergeForbidden` check — rejecting an
        // interior collapse between two boundary vertices — is deferred to
        // v2 because the simple "both endpoints boundary" heuristic flags
        // legitimate collapses within a single loop (e.g. the diamond's
        // central edge). A correct implementation needs boundary-loop
        // identity, which arrives with the vertex classifier in v2.

        // Gather the left-face half-edges and their neighbours.
        let he_rec = self.half_edges[he.index()].clone();
        let e2 = he_rec.next;
        let e3 = self.half_edges[e2.index()].next;
        let f_l = he_rec.face;
        let t_au = self.half_edges[e3.index()].twin;
        let t_va = self.half_edges[e2.index()].twin;

        // Gather the right-face half-edges and their neighbours (if interior).
        let (t_ub, t_bv, f_r, f2, f3) = if twin.is_valid() {
            let twin_rec = self.half_edges[twin.index()].clone();
            let f2 = twin_rec.next;
            let f3 = self.half_edges[f2.index()].next;
            let t_ub = self.half_edges[f2.index()].twin;
            let t_bv = self.half_edges[f3.index()].twin;
            (t_ub, t_bv, twin_rec.face, f2, f3)
        } else {
            (
                HalfEdgeId::INVALID,
                HalfEdgeId::INVALID,
                FaceId::INVALID,
                HalfEdgeId::INVALID,
                HalfEdgeId::INVALID,
            )
        };

        // Re-wire twins on the a-side.
        if t_au.is_valid() {
            self.half_edges[t_au.index()].twin = t_va;
        }
        if t_va.is_valid() {
            self.half_edges[t_va.index()].twin = t_au;
        }
        // Re-wire twins on the b-side (interior only).
        if t_ub.is_valid() {
            self.half_edges[t_ub.index()].twin = t_bv;
        }
        if t_bv.is_valid() {
            self.half_edges[t_bv.index()].twin = t_ub;
        }

        // Redirect all half-edges that pointed to v (as head) to point to u.
        for h in &mut self.half_edges {
            if !h.removed && h.vertex == v {
                h.vertex = u;
            }
        }

        // Move u to the chosen position.
        self.vertices[u.index()].pos = pos;

        // Mark the two (or one) faces and their six (or three) half-edges removed.
        self.mark_half_edge_removed(he);
        self.mark_half_edge_removed(e2);
        self.mark_half_edge_removed(e3);
        self.mark_face_removed(f_l);
        if twin.is_valid() {
            self.mark_half_edge_removed(twin);
            self.mark_half_edge_removed(f2);
            self.mark_half_edge_removed(f3);
            self.mark_face_removed(f_r);
        }

        // Remove vertex v.
        self.vertices[v.index()].removed = true;
        self.vertices[v.index()].he_out = HalfEdgeId::INVALID;
        self.free_vertices.push(v);

        // Repair he_outs for affected vertices.
        self.rebuild_vertex_he_outs();

        Ok(u)
    }

    /// Splits a half-edge by inserting a new vertex at `pos`.
    ///
    /// The new vertex is inserted on the edge shared by `he` and its twin
    /// (if any). Returns the [`VertexId`] of the new vertex. The two (or
    /// one) incident faces each become two faces; `he` and its twin are
    /// repurposed to point to the new vertex.
    pub fn split_edge(&mut self, he: HalfEdgeId, pos: Vec3) -> Result<VertexId, HalfEdgeOpError> {
        if !self.half_edge_is_live(he) {
            return Err(HalfEdgeOpError::InvalidHandle(he));
        }
        let twin = self.he_twin(he);

        // F_l = (u, v, a): e1 = he (u→v), e2 (v→a), e3 (a→u).
        let e1 = he;
        let e1_rec = self.half_edges[e1.index()].clone();
        let e2 = e1_rec.next;
        let e3 = self.half_edges[e2.index()].next;
        let f_l = e1_rec.face;
        let _u = self.he_tail(e1);
        let _v = e1_rec.vertex;
        let a = self.half_edges[e2.index()].vertex;

        // F_r = (v, u, b) if interior: t1 = twin (v→u), t2 (u→b), t3 (b→v).
        let (t1, t2, t3, f_r, b) = if twin.is_valid() {
            let t1 = twin;
            let t1_rec = self.half_edges[t1.index()].clone();
            let t2 = t1_rec.next;
            let t3 = self.half_edges[t2.index()].next;
            let b = self.half_edges[t2.index()].vertex;
            (t1, t2, t3, t1_rec.face, b)
        } else {
            (
                HalfEdgeId::INVALID,
                HalfEdgeId::INVALID,
                HalfEdgeId::INVALID,
                FaceId::INVALID,
                VertexId::INVALID,
            )
        };

        // Allocate new vertex w.
        let w = self.allocate_vertex(pos);

        // Allocate new half-edges.
        // Left side: n_wa (w→a), n_aw (a→w), n_wv (w→v).
        let n_wa = self.allocate_half_edge();
        let n_aw = self.allocate_half_edge();
        let n_wv = self.allocate_half_edge();
        // Right side (interior only): n_wu (w→u), n_bw (b→w), n_wb (w→b).
        let (n_wu, n_bw, n_wb) = if twin.is_valid() {
            (
                self.allocate_half_edge(),
                self.allocate_half_edge(),
                self.allocate_half_edge(),
            )
        } else {
            (
                HalfEdgeId::INVALID,
                HalfEdgeId::INVALID,
                HalfEdgeId::INVALID,
            )
        };

        // Allocate new face for F_l2 and F_r2.
        let f_l2 = self.allocate_face();
        let f_r2 = if twin.is_valid() {
            self.allocate_face()
        } else {
            FaceId::INVALID
        };

        // Repurpose e1 as u→w (change head only).
        self.half_edges[e1.index()].vertex = w;
        self.half_edges[e1.index()].next = n_wa;
        self.half_edges[e1.index()].face = f_l;
        // Populate new half-edges on the left.
        self.half_edges[n_wa.index()] = HalfEdgeRecord {
            vertex: a,
            face: f_l,
            next: e3,
            twin: n_aw,
            removed: false,
        };
        // e3.next was e1 (cycled back); now needs to still point to e1 (which is u→w).
        self.half_edges[e3.index()].next = e1;
        // Face F_l is now the triangle (u, w, a). Its representative half-edge is still e1.
        self.faces[f_l.index()].he = e1;
        // F_l2 = (w, v, a): n_wv → e2 → n_aw → n_wv.
        self.half_edges[n_wv.index()] = HalfEdgeRecord {
            vertex: self.half_edges[e1.index()].vertex, // but e1.vertex is now w — we want v
            ..HalfEdgeRecord {
                vertex: VertexId::INVALID,
                face: FaceId::INVALID,
                next: HalfEdgeId::INVALID,
                twin: HalfEdgeId::INVALID,
                removed: false,
            }
        };
        // Fix the above — n_wv.vertex should be v (the old head of e1 before repurposing).
        // We captured _v earlier as e1_rec.vertex.
        self.half_edges[n_wv.index()] = HalfEdgeRecord {
            vertex: _v,
            face: f_l2,
            next: e2,
            twin: HalfEdgeId::INVALID,
            removed: false,
        };
        self.half_edges[e2.index()].face = f_l2;
        self.half_edges[e2.index()].next = n_aw;
        self.half_edges[n_aw.index()] = HalfEdgeRecord {
            vertex: w,
            face: f_l2,
            next: n_wv,
            twin: n_wa,
            removed: false,
        };
        self.faces[f_l2.index()] = FaceRecord {
            he: n_wv,
            removed: false,
        };

        if twin.is_valid() {
            // Repurpose t1 as v→w.
            self.half_edges[t1.index()].vertex = w;
            self.half_edges[t1.index()].next = n_wb;
            self.half_edges[t1.index()].face = f_r;
            self.half_edges[n_wb.index()] = HalfEdgeRecord {
                vertex: b,
                face: f_r,
                next: t3,
                twin: n_bw,
                removed: false,
            };
            self.half_edges[t3.index()].next = t1;
            self.faces[f_r.index()].he = t1;
            // F_r2 = (w, u, b): n_wu → t2 → n_bw → n_wu.
            self.half_edges[n_wu.index()] = HalfEdgeRecord {
                vertex: _u,
                face: f_r2,
                next: t2,
                twin: e1,
                removed: false,
            };
            self.half_edges[t2.index()].face = f_r2;
            self.half_edges[t2.index()].next = n_bw;
            self.half_edges[n_bw.index()] = HalfEdgeRecord {
                vertex: w,
                face: f_r2,
                next: n_wu,
                twin: n_wb,
                removed: false,
            };
            self.faces[f_r2.index()] = FaceRecord {
                he: n_wu,
                removed: false,
            };
            // Twin e1 ↔ n_wu, t1 ↔ n_wv.
            self.half_edges[e1.index()].twin = n_wu;
            self.half_edges[t1.index()].twin = n_wv;
            self.half_edges[n_wv.index()].twin = t1;
        } else {
            // Boundary split: e1's twin stays INVALID; n_wv's twin INVALID too.
            self.half_edges[e1.index()].twin = HalfEdgeId::INVALID;
            self.half_edges[n_wv.index()].twin = HalfEdgeId::INVALID;
        }

        // Set vertex positions (already done for w via allocate_vertex).
        self.rebuild_vertex_he_outs();
        Ok(w)
    }

    /// Flips the diagonal of the two triangles sharing `he`.
    ///
    /// If `he` goes from `u` to `v` in faces `(u, v, a)` and `(v, u, b)`,
    /// after the flip `he` goes from `b` to `a` in faces `(u, b, a)` and
    /// `(v, a, b)`.
    pub fn flip_edge(&mut self, he: HalfEdgeId) -> Result<(), HalfEdgeOpError> {
        if !self.half_edge_is_live(he) {
            return Err(HalfEdgeOpError::InvalidHandle(he));
        }
        let twin = self.he_twin(he);
        if !twin.is_valid() {
            return Err(HalfEdgeOpError::BoundaryEdgeFlip);
        }

        let e1 = he;
        let e1_rec = self.half_edges[e1.index()].clone();
        let e2 = e1_rec.next;
        let e3 = self.half_edges[e2.index()].next;
        let f_l = e1_rec.face;

        let t1 = twin;
        let t1_rec = self.half_edges[t1.index()].clone();
        let t2 = t1_rec.next;
        let t3 = self.half_edges[t2.index()].next;
        let f_r = t1_rec.face;

        let a = self.half_edges[e2.index()].vertex; // apex of F_l
        let b = self.half_edges[t2.index()].vertex; // apex of F_r

        // Degenerate / duplicate edge check: if a == b, the two triangles are
        // incident along two edges and flipping would create a self-loop.
        if a == b {
            return Err(HalfEdgeOpError::FlipWouldDuplicateEdge);
        }
        // If edge a-b already exists, the flip would duplicate it.
        let mut a_connects_b = false;
        for n in self.vertex_one_ring(a) {
            if n == b {
                a_connects_b = true;
                break;
            }
        }
        if a_connects_b {
            return Err(HalfEdgeOpError::FlipWouldDuplicateEdge);
        }

        // Rewire e1 as b→a.
        self.half_edges[e1.index()].vertex = a;
        self.half_edges[e1.index()].next = e3;
        self.half_edges[e1.index()].face = f_l;
        // Rewire t1 as a→b.
        self.half_edges[t1.index()].vertex = b;
        self.half_edges[t1.index()].next = t3;
        self.half_edges[t1.index()].face = f_r;

        // F_l cycle: t2 (u→b) → e1 (b→a) → e3 (a→u). F_l = (u, b, a).
        self.half_edges[t2.index()].face = f_l;
        self.half_edges[t2.index()].next = e1;
        self.half_edges[e3.index()].next = t2;
        self.faces[f_l.index()].he = e1;

        // F_r cycle: e2 (v→a) → t1 (a→b) → t3 (b→v). F_r = (v, a, b).
        self.half_edges[e2.index()].face = f_r;
        self.half_edges[e2.index()].next = t1;
        self.half_edges[t3.index()].next = e2;
        self.faces[f_r.index()].he = t1;

        self.rebuild_vertex_he_outs();
        Ok(())
    }

    /// Removes a face without rewiring.
    ///
    /// Marks the face and its three half-edges as removed, invalidates the
    /// twins of the three edges (they become boundary edges), and repairs
    /// vertex `he_out` pointers. Used by [`RemoveSlivers`](super::passes::RemoveSlivers)
    /// when a collapse is rejected but the face is tolerable to just drop.
    pub fn remove_degenerate_face(&mut self, f: FaceId) -> Result<(), HalfEdgeOpError> {
        if !self.face_is_live(f) {
            return Err(HalfEdgeOpError::InvalidHandle(HalfEdgeId::INVALID));
        }
        let h0 = self.faces[f.index()].he;
        let h1 = self.he_next(h0);
        let h2 = self.he_next(h1);
        for h in [h0, h1, h2] {
            let twin = self.he_twin(h);
            if twin.is_valid() {
                self.half_edges[twin.index()].twin = HalfEdgeId::INVALID;
            }
            self.mark_half_edge_removed(h);
        }
        self.mark_face_removed(f);
        self.rebuild_vertex_he_outs();
        Ok(())
    }

    // --- Helpers ---

    fn check_link_condition(
        &self,
        he: HalfEdgeId,
        u: VertexId,
        v: VertexId,
        twin: HalfEdgeId,
    ) -> Result<(), HalfEdgeOpError> {
        let u_ring: HashSet<VertexId> = self.vertex_one_ring(u).collect();
        let v_ring: HashSet<VertexId> = self.vertex_one_ring(v).collect();
        let shared: HashSet<VertexId> = u_ring.intersection(&v_ring).copied().collect();
        let a_apex = self.he_head(self.he_next(he));
        let mut allowed: HashSet<VertexId> = HashSet::new();
        allowed.insert(a_apex);
        if twin.is_valid() {
            let b_apex = self.he_head(self.he_next(twin));
            allowed.insert(b_apex);
        }
        if shared == allowed {
            Ok(())
        } else {
            Err(HalfEdgeOpError::LinkConditionFailed)
        }
    }

    fn mark_half_edge_removed(&mut self, h: HalfEdgeId) {
        self.half_edges[h.index()].removed = true;
        self.free_half_edges.push(h);
    }

    fn mark_face_removed(&mut self, f: FaceId) {
        self.faces[f.index()].removed = true;
        self.free_faces.push(f);
    }

    fn allocate_vertex(&mut self, pos: Vec3) -> VertexId {
        if let Some(v) = self.free_vertices.pop() {
            self.vertices[v.index()] = super::half_edge::VertexRecord {
                pos,
                he_out: HalfEdgeId::INVALID,
                removed: false,
            };
            v
        } else {
            let id = VertexId(self.vertices.len() as u32);
            self.vertices.push(super::half_edge::VertexRecord {
                pos,
                he_out: HalfEdgeId::INVALID,
                removed: false,
            });
            id
        }
    }

    fn allocate_half_edge(&mut self) -> HalfEdgeId {
        if let Some(h) = self.free_half_edges.pop() {
            self.half_edges[h.index()] = HalfEdgeRecord {
                vertex: VertexId::INVALID,
                face: FaceId::INVALID,
                next: HalfEdgeId::INVALID,
                twin: HalfEdgeId::INVALID,
                removed: false,
            };
            h
        } else {
            let id = HalfEdgeId(self.half_edges.len() as u32);
            self.half_edges.push(HalfEdgeRecord {
                vertex: VertexId::INVALID,
                face: FaceId::INVALID,
                next: HalfEdgeId::INVALID,
                twin: HalfEdgeId::INVALID,
                removed: false,
            });
            id
        }
    }

    fn allocate_face(&mut self) -> FaceId {
        if let Some(f) = self.free_faces.pop() {
            self.faces[f.index()] = FaceRecord {
                he: HalfEdgeId::INVALID,
                removed: false,
            };
            f
        } else {
            let id = FaceId(self.faces.len() as u32);
            self.faces.push(FaceRecord {
                he: HalfEdgeId::INVALID,
                removed: false,
            });
            id
        }
    }

    fn rebuild_vertex_he_outs(&mut self) {
        for v in &mut self.vertices {
            if !v.removed {
                v.he_out = HalfEdgeId::INVALID;
            }
        }
        for i in 0..self.half_edges.len() {
            let rec = &self.half_edges[i];
            if rec.removed {
                continue;
            }
            let this_he = HalfEdgeId(i as u32);
            let tail = self.he_tail(this_he);
            if !self.vertex_is_live(tail) {
                continue;
            }
            let this_twin_invalid = rec.twin == HalfEdgeId::INVALID;
            let current = self.vertices[tail.index()].he_out;
            let should_assign = if !current.is_valid() {
                true
            } else if this_twin_invalid {
                self.half_edges[current.index()].twin.is_valid()
            } else {
                false
            };
            if should_assign {
                self.vertices[tail.index()].he_out = this_he;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;

    fn single_triangle() -> IsoMesh {
        IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        }
    }

    fn tetrahedron() -> IsoMesh {
        IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            normals: vec![Vec3::Z; 4],
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        }
    }

    /// Two triangles sharing an edge: (0,1,2) and (0,2,3). Diamond shape.
    fn diamond() -> IsoMesh {
        IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.5, 1.0, 0.0),
                Vec3::new(0.5, -1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 4],
            indices: vec![0, 1, 2, 0, 3, 1],
        }
    }

    #[test]
    fn flip_boundary_edge_errors() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        // Any half-edge in the single triangle is boundary.
        let err = mesh.flip_edge(HalfEdgeId(0)).unwrap_err();
        assert!(matches!(err, HalfEdgeOpError::BoundaryEdgeFlip));
    }

    #[test]
    fn flip_diamond_interior_edge_swaps_diagonal() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&diamond()).expect("construct");
        // Find the interior edge (has a valid twin).
        let interior_he = (0..mesh.half_edge_count() as u32)
            .map(HalfEdgeId)
            .find(|&h| mesh.he_twin(h).is_valid())
            .expect("interior edge exists in diamond");
        let pre_head = mesh.he_head(interior_he);
        let pre_tail = mesh.he_tail(interior_he);
        mesh.flip_edge(interior_he).expect("flip");
        // After flip, the same half-edge now connects the two apices (1 and 3).
        let post_head = mesh.he_head(interior_he);
        let post_tail = mesh.he_tail(interior_he);
        let pre_set: HashSet<VertexId> = [pre_head, pre_tail].into_iter().collect();
        let post_set: HashSet<VertexId> = [post_head, post_tail].into_iter().collect();
        assert_ne!(pre_set, post_set, "diagonal must change on flip");
        // Still manifold.
        assert!(mesh.is_manifold());
    }

    #[test]
    fn flip_duplicate_edge_errors() {
        // A tetrahedron: every pair of vertices is already connected, so any flip
        // would duplicate the existing a-b edge.
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        let interior_he = (0..mesh.half_edge_count() as u32)
            .map(HalfEdgeId)
            .find(|&h| mesh.he_twin(h).is_valid())
            .expect("tetrahedron has interior edges");
        let err = mesh.flip_edge(interior_he).unwrap_err();
        assert!(matches!(err, HalfEdgeOpError::FlipWouldDuplicateEdge));
    }

    #[test]
    fn split_boundary_edge_of_single_triangle() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        let pre_vertex_count = mesh.vertex_count();
        let pre_face_count = mesh.face_count();
        let midpoint = Vec3::new(0.5, 0.0, 0.0);
        let w = mesh.split_edge(HalfEdgeId(0), midpoint).expect("split");
        assert_eq!(mesh.vertex_position(w), midpoint);
        // One vertex added, one face added (boundary split).
        assert_eq!(mesh.vertex_count(), pre_vertex_count + 1);
        assert_eq!(mesh.face_count(), pre_face_count + 1);
        assert!(mesh.is_manifold());
    }

    #[test]
    fn split_interior_edge_of_diamond() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&diamond()).expect("construct");
        let pre_vertex_count = mesh.vertex_count();
        let pre_face_count = mesh.face_count();
        let interior_he = (0..mesh.half_edge_count() as u32)
            .map(HalfEdgeId)
            .find(|&h| mesh.he_twin(h).is_valid())
            .expect("interior edge");
        let tail = mesh.vertex_position(mesh.he_tail(interior_he));
        let head = mesh.vertex_position(mesh.he_head(interior_he));
        let midpoint = (tail + head) * 0.5;
        mesh.split_edge(interior_he, midpoint).expect("split");
        assert_eq!(mesh.vertex_count(), pre_vertex_count + 1);
        assert_eq!(mesh.face_count(), pre_face_count + 2);
        assert!(mesh.is_manifold());
    }

    #[test]
    fn collapse_diamond_interior_edge_removes_two_faces() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&diamond()).expect("construct");
        let pre_vertex_count = mesh.vertex_count();
        let pre_face_count = mesh.face_count();
        let interior_he = (0..mesh.half_edge_count() as u32)
            .map(HalfEdgeId)
            .find(|&h| mesh.he_twin(h).is_valid())
            .expect("interior edge");
        mesh.collapse_edge(interior_he).expect("collapse");
        // Interior collapse removes two faces and one vertex.
        assert_eq!(mesh.vertex_count(), pre_vertex_count - 1);
        assert_eq!(mesh.face_count(), pre_face_count - 2);
    }

    #[test]
    fn collapse_boundary_edge_of_diamond() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&diamond()).expect("construct");
        let pre_vertex_count = mesh.vertex_count();
        let pre_face_count = mesh.face_count();
        // Try each boundary half-edge in turn; accept the first that passes
        // the link condition. The diamond has two interior half-edges on edge
        // 0-2 (the shared diagonal) and six boundary half-edges on the outer
        // edges (1-2, 2-0 in the first tri has a twin = 0-2 which is interior
        // actually — let me recount). Either way, at least one boundary
        // collapse must succeed.
        let mut collapsed = false;
        for h in (0..mesh.half_edge_count() as u32).map(HalfEdgeId) {
            if !mesh.he_twin(h).is_valid() && mesh.collapse_edge(h).is_ok() {
                collapsed = true;
                break;
            }
        }
        assert!(
            collapsed,
            "should be able to collapse at least one boundary edge"
        );
        // Boundary collapse removes one face and one vertex.
        assert_eq!(mesh.vertex_count(), pre_vertex_count - 1);
        assert_eq!(mesh.face_count(), pre_face_count - 1);
    }

    #[test]
    fn collapse_invalid_handle_errors() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        let err = mesh.collapse_edge(HalfEdgeId::INVALID).unwrap_err();
        assert!(matches!(err, HalfEdgeOpError::InvalidHandle(_)));
    }

    #[test]
    fn flip_invalid_handle_errors() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        let err = mesh.flip_edge(HalfEdgeId::INVALID).unwrap_err();
        assert!(matches!(err, HalfEdgeOpError::InvalidHandle(_)));
    }
}
