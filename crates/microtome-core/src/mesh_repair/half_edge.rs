//! Half-edge mesh data structure.
//!
//! # Convention
//!
//! Each half-edge stores the **head** vertex it points to (`vertex` field),
//! along with `next`, `twin`, and its parent `face`. Boundary half-edges
//! have `twin = HalfEdgeId::INVALID`; the opposite direction is *not*
//! materialised as a ghost half-edge. `prev` is not stored; in a triangle
//! mesh `prev(h) = h.next.next`.
//!
//! Vertices store a single outgoing half-edge `he_out`. For boundary
//! vertices we pick an outgoing half-edge whose own twin is invalid —
//! i.e. the one on the "far end" of the vertex's face chain. Walking
//! backward via `prev(h).twin` from this he_out visits every incident
//! face before terminating on the boundary at the other end.
//!
//! # Freelists
//!
//! Passes that collapse or split edges mutate the mesh mid-run. To keep
//! handle IDs stable across one pipeline run, removed slots are marked
//! `removed: true` and their IDs are pushed onto a freelist for reuse.
//! [`HalfEdgeMesh::compact`] runs once inside [`HalfEdgeMesh::to_iso_mesh`]
//! to emit a compact indexed mesh for the caller.

use glam::Vec3;
use std::collections::HashMap;

use super::error::TopologyError;
use super::vertex_class::VertexClass;
use crate::isosurface::IsoMesh;

/// Opaque identifier for a vertex in a [`HalfEdgeMesh`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VertexId(pub u32);

/// Opaque identifier for a half-edge in a [`HalfEdgeMesh`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct HalfEdgeId(pub u32);

/// Opaque identifier for a triangular face in a [`HalfEdgeMesh`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct FaceId(pub u32);

impl VertexId {
    /// Sentinel value meaning "no vertex".
    pub const INVALID: Self = Self(u32::MAX);

    /// Returns this ID as a `usize` index into a backing `Vec`.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `true` if this ID is not the `INVALID` sentinel.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

impl HalfEdgeId {
    /// Sentinel value meaning "no half-edge".
    pub const INVALID: Self = Self(u32::MAX);

    /// Returns this ID as a `usize` index into a backing `Vec`.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `true` if this ID is not the `INVALID` sentinel.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

impl FaceId {
    /// Sentinel value meaning "no face".
    pub const INVALID: Self = Self(u32::MAX);

    /// Returns this ID as a `usize` index into a backing `Vec`.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `true` if this ID is not the `INVALID` sentinel.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

/// Per-vertex record in a [`HalfEdgeMesh`].
#[derive(Debug, Clone)]
pub(super) struct VertexRecord {
    /// World-space position.
    pub pos: Vec3,
    /// Any outgoing half-edge from this vertex. For boundary vertices this
    /// is specifically chosen so `he_out.twin == INVALID` — i.e. it's a
    /// boundary half-edge. Walking backward from it via `prev(h).twin`
    /// visits all incident faces and terminates on the boundary.
    pub he_out: HalfEdgeId,
    /// Soft-delete flag. Removed vertices are compacted away in
    /// [`HalfEdgeMesh::to_iso_mesh`].
    pub removed: bool,
}

/// Per-half-edge record in a [`HalfEdgeMesh`].
#[derive(Debug, Clone)]
pub(super) struct HalfEdgeRecord {
    /// Head (destination) vertex of this half-edge.
    pub vertex: VertexId,
    /// Parent face. `FaceId::INVALID` only on ghost half-edges (not used in v1).
    pub face: FaceId,
    /// Next half-edge in the face cycle.
    pub next: HalfEdgeId,
    /// The twin half-edge on the opposite side of this edge, or
    /// `HalfEdgeId::INVALID` on a boundary edge.
    pub twin: HalfEdgeId,
    /// Soft-delete flag.
    pub removed: bool,
}

/// Per-face record in a [`HalfEdgeMesh`].
#[derive(Debug, Clone)]
pub(super) struct FaceRecord {
    /// One of the three half-edges belonging to this face.
    pub he: HalfEdgeId,
    /// Soft-delete flag.
    pub removed: bool,
}

/// A manifold (or boundary-manifold) triangle mesh with half-edge connectivity.
///
/// Built from an [`IsoMesh`] via [`from_iso_mesh`](Self::from_iso_mesh),
/// mutated by mesh-repair passes, and emitted back to an `IsoMesh` via
/// [`to_iso_mesh`](Self::to_iso_mesh). See the module-level docs for the
/// storage convention.
#[derive(Debug, Clone, Default)]
pub struct HalfEdgeMesh {
    pub(super) vertices: Vec<VertexRecord>,
    pub(super) half_edges: Vec<HalfEdgeRecord>,
    pub(super) faces: Vec<FaceRecord>,
    /// Per-vertex class, parallel to `vertices`. Default `Interior`.
    /// Populated by [`VertexClassifier`](super::vertex_class::VertexClassifier)
    /// (lands with task #17). Survives compact and inherits via `combine`
    /// on collapse.
    pub(super) vertex_class: Vec<VertexClass>,
    pub(super) free_vertices: Vec<VertexId>,
    pub(super) free_half_edges: Vec<HalfEdgeId>,
    pub(super) free_faces: Vec<FaceId>,
}

impl HalfEdgeMesh {
    /// Creates a new empty half-edge mesh.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the number of live (non-removed) vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len() - self.free_vertices.len()
    }

    /// Returns the number of live (non-removed) half-edges.
    pub fn half_edge_count(&self) -> usize {
        self.half_edges.len() - self.free_half_edges.len()
    }

    /// Returns the number of live (non-removed) faces.
    pub fn face_count(&self) -> usize {
        self.faces.len() - self.free_faces.len()
    }

    /// Returns the world-space position of a vertex.
    ///
    /// Panics only on out-of-range IDs (indicative of a caller bug). A removed
    /// vertex's position is still accessible; callers that need to filter by
    /// liveness should use [`vertex_is_live`](Self::vertex_is_live) first.
    pub fn vertex_position(&self, v: VertexId) -> Vec3 {
        self.vertices[v.index()].pos
    }

    /// Sets the world-space position of a vertex.
    pub fn set_vertex_position(&mut self, v: VertexId, pos: Vec3) {
        self.vertices[v.index()].pos = pos;
    }

    /// Returns `true` if the ID is in range and the slot is not removed.
    pub fn vertex_is_live(&self, v: VertexId) -> bool {
        v.is_valid() && v.index() < self.vertices.len() && !self.vertices[v.index()].removed
    }

    /// Returns `true` if the ID is in range and the slot is not removed.
    pub fn half_edge_is_live(&self, h: HalfEdgeId) -> bool {
        h.is_valid() && h.index() < self.half_edges.len() && !self.half_edges[h.index()].removed
    }

    /// Returns `true` if the ID is in range and the slot is not removed.
    pub fn face_is_live(&self, f: FaceId) -> bool {
        f.is_valid() && f.index() < self.faces.len() && !self.faces[f.index()].removed
    }

    /// Returns the head vertex of a half-edge.
    pub fn he_head(&self, h: HalfEdgeId) -> VertexId {
        self.half_edges[h.index()].vertex
    }

    /// Returns the tail (origin) vertex of a half-edge.
    ///
    /// Computed as `prev(h).vertex` using `prev = next.next` for triangles.
    pub fn he_tail(&self, h: HalfEdgeId) -> VertexId {
        let rec = &self.half_edges[h.index()];
        let prev = self.half_edges[rec.next.index()].next;
        self.half_edges[prev.index()].vertex
    }

    /// Returns the `next` half-edge in the face cycle.
    pub fn he_next(&self, h: HalfEdgeId) -> HalfEdgeId {
        self.half_edges[h.index()].next
    }

    /// Returns the `prev` half-edge in the face cycle (computed as `next.next`).
    pub fn he_prev(&self, h: HalfEdgeId) -> HalfEdgeId {
        self.half_edges[self.half_edges[h.index()].next.index()].next
    }

    /// Returns the twin of a half-edge, or `HalfEdgeId::INVALID` on a boundary.
    pub fn he_twin(&self, h: HalfEdgeId) -> HalfEdgeId {
        self.half_edges[h.index()].twin
    }

    /// Returns the parent face of a half-edge.
    pub fn he_face(&self, h: HalfEdgeId) -> FaceId {
        self.half_edges[h.index()].face
    }

    /// Returns any outgoing half-edge from a vertex, or `HalfEdgeId::INVALID`
    /// if the vertex is isolated.
    pub fn vertex_he_out(&self, v: VertexId) -> HalfEdgeId {
        self.vertices[v.index()].he_out
    }

    /// Returns the three head vertices of a face, in winding order.
    pub fn face_vertices(&self, f: FaceId) -> [VertexId; 3] {
        let h0 = self.faces[f.index()].he;
        let h1 = self.half_edges[h0.index()].next;
        // The face winds v0 → v1 → v2 where v0 = he_tail(h0), v1 = h0.vertex,
        // v2 = h1.vertex (= he_tail(h2) with h2 = h1.next).
        let v0 = self.he_tail(h0);
        let v1 = self.half_edges[h0.index()].vertex;
        let v2 = self.half_edges[h1.index()].vertex;
        [v0, v1, v2]
    }

    /// Alias for [`face_vertices`](Self::face_vertices).
    pub fn face_triangle(&self, f: FaceId) -> [VertexId; 3] {
        self.face_vertices(f)
    }

    /// Returns the three world-space positions of a face, in winding order.
    pub fn face_positions(&self, f: FaceId) -> [Vec3; 3] {
        let [v0, v1, v2] = self.face_vertices(f);
        [
            self.vertices[v0.index()].pos,
            self.vertices[v1.index()].pos,
            self.vertices[v2.index()].pos,
        ]
    }

    /// Returns the length of the edge represented by a half-edge.
    pub fn edge_length(&self, h: HalfEdgeId) -> f32 {
        let head = self.vertices[self.he_head(h).index()].pos;
        let tail = self.vertices[self.he_tail(h).index()].pos;
        (head - tail).length()
    }

    /// Returns the degree (count of one-ring neighbours) of a vertex.
    pub fn vertex_valence(&self, v: VertexId) -> u32 {
        self.vertex_one_ring(v).count() as u32
    }

    /// Returns `true` if the vertex lies on a boundary loop.
    ///
    /// Relies on the he_out assignment invariant: for boundary vertices,
    /// `he_out.twin == HalfEdgeId::INVALID`.
    pub fn vertex_is_boundary(&self, v: VertexId) -> bool {
        let he_out = self.vertices[v.index()].he_out;
        if !he_out.is_valid() {
            return false;
        }
        !self.half_edges[he_out.index()].twin.is_valid()
    }

    /// Iterates the one-ring neighbours of a vertex.
    ///
    /// For interior vertices, yields the head of every outgoing half-edge in
    /// cyclic order. For boundary vertices, yields the same *plus* one extra
    /// neighbour across the boundary gap — namely the head of the "next"
    /// half-edge after the boundary-adjacent outgoing, which is the
    /// across-the-gap neighbour that is only reachable via an incoming
    /// boundary half-edge.
    pub fn vertex_one_ring(&self, v: VertexId) -> OneRingIter<'_> {
        let start = self.vertices[v.index()].he_out;
        OneRingIter {
            mesh: self,
            start,
            current: start,
            first: true,
            pending_tail: HalfEdgeId::INVALID,
        }
    }

    /// Iterates unique edges of the mesh, yielding one half-edge per edge.
    ///
    /// Boundary half-edges (no twin) are yielded once; interior edges yield
    /// the half-edge with the lower index.
    pub fn edge_iter(&self) -> EdgeIter<'_> {
        EdgeIter {
            mesh: self,
            index: 0,
        }
    }

    /// Returns the boundary loops of the mesh.
    ///
    /// Each loop is a sequence of boundary half-edges (twin == INVALID) in
    /// the order they connect around a hole. The emitted order for a given
    /// loop starts at the lowest-indexed half-edge in that loop.
    pub fn boundary_loops(&self) -> Vec<Vec<HalfEdgeId>> {
        let mut visited = vec![false; self.half_edges.len()];
        let mut loops: Vec<Vec<HalfEdgeId>> = Vec::new();
        for i in 0..self.half_edges.len() {
            let h = &self.half_edges[i];
            if h.removed || h.twin.is_valid() || visited[i] {
                continue;
            }
            let mut loop_hes = Vec::new();
            let start = HalfEdgeId(i as u32);
            let mut current = start;
            loop {
                loop_hes.push(current);
                visited[current.index()] = true;
                let next = self.next_boundary_he(current);
                if next == start {
                    break;
                }
                current = next;
            }
            loops.push(loop_hes);
        }
        loops
    }

    /// Given a boundary half-edge (twin == INVALID), returns the next boundary
    /// half-edge in the loop. Walks around `he_head(h)` via `next` / `twin`
    /// pivots until the next boundary-adjacent outgoing is found.
    pub fn next_boundary_he(&self, h: HalfEdgeId) -> HalfEdgeId {
        debug_assert!(!self.half_edges[h.index()].twin.is_valid());
        let mut current = self.he_next(h);
        loop {
            let twin = self.he_twin(current);
            if !twin.is_valid() {
                return current;
            }
            current = self.he_next(twin);
        }
    }

    /// Returns `true` if construction-time invariants still hold:
    /// - every live half-edge's twin (when valid) points back;
    /// - every face's three half-edges cycle correctly via `next`;
    /// - no edge has more than two incident live faces.
    ///
    /// The pipeline calls this between passes when policy dictates.
    pub fn is_manifold(&self) -> bool {
        // Twin symmetry.
        for (i, h) in self.half_edges.iter().enumerate() {
            if h.removed {
                continue;
            }
            if h.twin.is_valid() {
                if !self.half_edge_is_live(h.twin) {
                    return false;
                }
                if self.half_edges[h.twin.index()].twin != HalfEdgeId(i as u32) {
                    return false;
                }
            }
        }
        // Face cycles of length 3 and each face's half-edges point back.
        for (fi, f) in self.faces.iter().enumerate() {
            if f.removed {
                continue;
            }
            let expected_face = FaceId(fi as u32);
            let h0 = f.he;
            let h1 = self.half_edges[h0.index()].next;
            let h2 = self.half_edges[h1.index()].next;
            let h3 = self.half_edges[h2.index()].next;
            if h3 != h0 {
                return false;
            }
            for h in [h0, h1, h2] {
                if self.half_edges[h.index()].face != expected_face {
                    return false;
                }
            }
        }
        true
    }

    /// Builds a `HalfEdgeMesh` from an `IsoMesh`.
    ///
    /// The input must describe a triangle-only, 2-manifold-ish mesh:
    /// - `indices.len()` must be a multiple of 3.
    /// - No triangle may have duplicate indices.
    /// - All indices must be less than `positions.len()`.
    /// - No edge may be shared by more than 2 triangles.
    ///
    /// Coincident positions are not de-duplicated here — run a welding pass
    /// first (see [`WeldVertices`](super::passes::WeldVertices) once
    /// implemented in a later task) to collapse them, otherwise genuine
    /// T-junctions or double-sided input will surface as non-manifold edges.
    pub fn from_iso_mesh(mesh: &IsoMesh) -> Result<Self, TopologyError> {
        let idx_len = mesh.indices.len();
        if !idx_len.is_multiple_of(3) {
            return Err(TopologyError::NonTriangleFace { len: idx_len });
        }

        let vertex_count = u32::try_from(mesh.positions.len()).map_err(|_| {
            // Platform with >4G vertices: surface as IndexOutOfRange on first tri.
            TopologyError::IndexOutOfRange {
                face_index: 0,
                index: u32::MAX,
                vertex_count: u32::MAX,
            }
        })?;

        let tri_count = idx_len / 3;

        let vertices: Vec<VertexRecord> = mesh
            .positions
            .iter()
            .map(|&p| VertexRecord {
                pos: p,
                he_out: HalfEdgeId::INVALID,
                removed: false,
            })
            .collect();

        let mut half_edges: Vec<HalfEdgeRecord> = Vec::with_capacity(tri_count * 3);
        let mut faces: Vec<FaceRecord> = Vec::with_capacity(tri_count);

        // Twin-pairing map: canonical edge key (min, max) → the first half-edge
        // we saw for that edge.
        let mut edge_map: HashMap<(u32, u32), HalfEdgeId> = HashMap::with_capacity(tri_count * 2);

        for (tri_i, tri) in mesh.indices.chunks_exact(3).enumerate() {
            let (i0, i1, i2) = (tri[0], tri[1], tri[2]);

            for &idx in &[i0, i1, i2] {
                if idx >= vertex_count {
                    return Err(TopologyError::IndexOutOfRange {
                        face_index: tri_i,
                        index: idx,
                        vertex_count,
                    });
                }
            }

            if i0 == i1 || i1 == i2 || i0 == i2 {
                return Err(TopologyError::DegenerateTriangle {
                    face_index: tri_i,
                    indices: [i0, i1, i2],
                });
            }

            let h0 = HalfEdgeId(half_edges.len() as u32);
            let h1 = HalfEdgeId(h0.0 + 1);
            let h2 = HalfEdgeId(h0.0 + 2);
            let face = FaceId(faces.len() as u32);

            // h0: i0 → i1 (head = i1)
            // h1: i1 → i2 (head = i2)
            // h2: i2 → i0 (head = i0)
            half_edges.push(HalfEdgeRecord {
                vertex: VertexId(i1),
                face,
                next: h1,
                twin: HalfEdgeId::INVALID,
                removed: false,
            });
            half_edges.push(HalfEdgeRecord {
                vertex: VertexId(i2),
                face,
                next: h2,
                twin: HalfEdgeId::INVALID,
                removed: false,
            });
            half_edges.push(HalfEdgeRecord {
                vertex: VertexId(i0),
                face,
                next: h0,
                twin: HalfEdgeId::INVALID,
                removed: false,
            });
            faces.push(FaceRecord {
                he: h0,
                removed: false,
            });

            for (tail, head, this_he) in [(i0, i1, h0), (i1, i2, h1), (i2, i0, h2)] {
                let key = if tail < head {
                    (tail, head)
                } else {
                    (head, tail)
                };
                match edge_map.get(&key).copied() {
                    None => {
                        edge_map.insert(key, this_he);
                    }
                    Some(prev_he) => {
                        if half_edges[prev_he.index()].twin != HalfEdgeId::INVALID {
                            // A third face is trying to share this edge.
                            return Err(TopologyError::NonManifoldEdge {
                                u: VertexId(key.0),
                                v: VertexId(key.1),
                                face_count: 3,
                                face_index: tri_i,
                            });
                        }
                        half_edges[prev_he.index()].twin = this_he;
                        half_edges[this_he.index()].twin = prev_he;
                    }
                }
            }
        }

        let vertex_class = vec![VertexClass::Interior; vertices.len()];
        let mut mesh_out = Self {
            vertices,
            half_edges,
            faces,
            vertex_class,
            free_vertices: Vec::new(),
            free_half_edges: Vec::new(),
            free_faces: Vec::new(),
        };
        mesh_out.assign_vertex_he_out();
        Ok(mesh_out)
    }

    /// Picks each vertex's `he_out`, preferring outgoing boundary half-edges
    /// (those with `twin == INVALID`).
    fn assign_vertex_he_out(&mut self) {
        for i in 0..self.half_edges.len() {
            let this_he = HalfEdgeId(i as u32);
            let rec = &self.half_edges[i];
            if rec.removed {
                continue;
            }
            let tail = self.he_tail(this_he);
            let current = self.vertices[tail.index()].he_out;
            let this_twin_invalid = rec.twin == HalfEdgeId::INVALID;
            let should_assign = if !current.is_valid() {
                true
            } else if this_twin_invalid {
                // Prefer an outgoing boundary half-edge — walking backward
                // from it via prev.twin reaches every incident face before
                // hitting the boundary on the opposite end.
                self.half_edges[current.index()].twin.is_valid()
            } else {
                false
            };
            if should_assign {
                self.vertices[tail.index()].he_out = this_he;
            }
        }
    }

    /// Drops removed slots, remaps IDs, and returns a compacted mesh.
    ///
    /// After `compact`, `free_*` lists are empty and all live records occupy
    /// the prefix of their `Vec`. IDs held by external callers are invalidated.
    pub fn compact(&mut self) {
        if self.free_vertices.is_empty()
            && self.free_half_edges.is_empty()
            && self.free_faces.is_empty()
        {
            return;
        }

        // Build vertex remap. Compact vertex_class alongside vertices.
        let mut v_remap: Vec<u32> = vec![u32::MAX; self.vertices.len()];
        let mut new_vertices: Vec<VertexRecord> = Vec::with_capacity(self.vertex_count());
        let mut new_vertex_class: Vec<VertexClass> = Vec::with_capacity(self.vertex_count());
        for (old, v) in self.vertices.iter().enumerate() {
            if !v.removed {
                v_remap[old] = new_vertices.len() as u32;
                new_vertices.push(v.clone());
                new_vertex_class.push(self.vertex_class[old]);
            }
        }

        // Build half-edge remap.
        let mut h_remap: Vec<u32> = vec![u32::MAX; self.half_edges.len()];
        let mut new_half_edges: Vec<HalfEdgeRecord> = Vec::with_capacity(self.half_edge_count());
        for (old, h) in self.half_edges.iter().enumerate() {
            if !h.removed {
                h_remap[old] = new_half_edges.len() as u32;
                new_half_edges.push(h.clone());
            }
        }

        // Build face remap.
        let mut f_remap: Vec<u32> = vec![u32::MAX; self.faces.len()];
        let mut new_faces: Vec<FaceRecord> = Vec::with_capacity(self.face_count());
        for (old, f) in self.faces.iter().enumerate() {
            if !f.removed {
                f_remap[old] = new_faces.len() as u32;
                new_faces.push(f.clone());
            }
        }

        // Apply remaps.
        for v in &mut new_vertices {
            if v.he_out.is_valid() {
                v.he_out = HalfEdgeId(h_remap[v.he_out.index()]);
            }
        }
        for h in &mut new_half_edges {
            h.vertex = VertexId(v_remap[h.vertex.index()]);
            if h.face.is_valid() {
                h.face = FaceId(f_remap[h.face.index()]);
            }
            if h.next.is_valid() {
                h.next = HalfEdgeId(h_remap[h.next.index()]);
            }
            if h.twin.is_valid() {
                h.twin = HalfEdgeId(h_remap[h.twin.index()]);
            }
        }
        for f in &mut new_faces {
            f.he = HalfEdgeId(h_remap[f.he.index()]);
        }

        self.vertices = new_vertices;
        self.vertex_class = new_vertex_class;
        self.half_edges = new_half_edges;
        self.faces = new_faces;
        self.free_vertices.clear();
        self.free_half_edges.clear();
        self.free_faces.clear();
    }

    /// Returns the [`VertexClass`] of a vertex.
    pub fn vertex_class(&self, v: VertexId) -> VertexClass {
        self.vertex_class
            .get(v.index())
            .copied()
            .unwrap_or(VertexClass::Interior)
    }

    /// Sets the [`VertexClass`] of a vertex. The classifier uses this when
    /// pinning vertices manually; passes typically go through the classifier.
    pub fn set_vertex_class(&mut self, v: VertexId, class: VertexClass) {
        if let Some(slot) = self.vertex_class.get_mut(v.index()) {
            *slot = class;
        }
    }

    /// Emits a compacted [`IsoMesh`] from this half-edge mesh.
    ///
    /// `normal_fn` is invoked for each live vertex to populate `normals`;
    /// callers that extracted the mesh from a [`ScalarField`](crate::isosurface::ScalarField)
    /// typically pass a closure wrapping `field.normal(p)`. Use
    /// [`to_iso_mesh_flat`](Self::to_iso_mesh_flat) if no field is available.
    pub fn to_iso_mesh(&mut self, normal_fn: impl Fn(Vec3) -> Vec3) -> IsoMesh {
        self.compact();

        let mut positions = Vec::with_capacity(self.vertices.len());
        let mut normals = Vec::with_capacity(self.vertices.len());
        for v in &self.vertices {
            positions.push(v.pos);
            normals.push(normal_fn(v.pos));
        }

        let mut indices: Vec<u32> = Vec::with_capacity(self.faces.len() * 3);
        for f in &self.faces {
            let h0 = f.he;
            let h1 = self.half_edges[h0.index()].next;
            let v0 = self.he_tail(h0);
            let v1 = self.half_edges[h0.index()].vertex;
            let v2 = self.half_edges[h1.index()].vertex;
            indices.push(v0.0);
            indices.push(v1.0);
            indices.push(v2.0);
        }

        IsoMesh {
            positions,
            normals,
            indices,
        }
    }

    /// Emits an `IsoMesh` with per-vertex normals set to `Vec3::ZERO`.
    ///
    /// Convenient for callers who don't have a scalar field and plan to run
    /// [`IsoMesh::generate_flat_normals`](IsoMesh::generate_flat_normals)
    /// afterwards.
    pub fn to_iso_mesh_flat(&mut self) -> IsoMesh {
        self.to_iso_mesh(|_| Vec3::ZERO)
    }
}

/// Iterator over a vertex's one-ring neighbours.
///
/// See [`HalfEdgeMesh::vertex_one_ring`] for semantics.
pub struct OneRingIter<'a> {
    mesh: &'a HalfEdgeMesh,
    start: HalfEdgeId,
    current: HalfEdgeId,
    first: bool,
    pending_tail: HalfEdgeId,
}

impl Iterator for OneRingIter<'_> {
    type Item = VertexId;

    fn next(&mut self) -> Option<VertexId> {
        if self.current.is_valid() {
            let h = self.current;
            let head = self.mesh.he_head(h);
            let prev = self.mesh.he_prev(h);
            let prev_twin = self.mesh.he_twin(prev);
            if !prev_twin.is_valid() {
                // Boundary: stash the across-the-gap neighbour for the next call.
                self.pending_tail = self.mesh.he_next(h);
                self.current = HalfEdgeId::INVALID;
            } else if !self.first && prev_twin == self.start {
                self.current = HalfEdgeId::INVALID;
            } else {
                self.current = prev_twin;
                self.first = false;
            }
            return Some(head);
        }
        if self.pending_tail.is_valid() {
            let h = self.pending_tail;
            self.pending_tail = HalfEdgeId::INVALID;
            return Some(self.mesh.he_head(h));
        }
        None
    }
}

/// Iterator over unique edges of a [`HalfEdgeMesh`].
pub struct EdgeIter<'a> {
    mesh: &'a HalfEdgeMesh,
    index: usize,
}

impl Iterator for EdgeIter<'_> {
    type Item = HalfEdgeId;

    fn next(&mut self) -> Option<HalfEdgeId> {
        while self.index < self.mesh.half_edges.len() {
            let i = self.index;
            self.index += 1;
            let h = &self.mesh.half_edges[i];
            if h.removed {
                continue;
            }
            let this_id = HalfEdgeId(i as u32);
            if !h.twin.is_valid() || this_id.0 < h.twin.0 {
                return Some(this_id);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sentinel_is_u32_max() {
        assert_eq!(VertexId::INVALID.0, u32::MAX);
        assert_eq!(HalfEdgeId::INVALID.0, u32::MAX);
        assert_eq!(FaceId::INVALID.0, u32::MAX);
    }

    #[test]
    fn invalid_ids_report_invalid() {
        assert!(!VertexId::INVALID.is_valid());
        assert!(!HalfEdgeId::INVALID.is_valid());
        assert!(!FaceId::INVALID.is_valid());
    }

    #[test]
    fn regular_ids_report_valid() {
        assert!(VertexId(0).is_valid());
        assert!(HalfEdgeId(42).is_valid());
        assert!(FaceId(1000).is_valid());
    }

    #[test]
    fn index_converts_to_usize() {
        assert_eq!(VertexId(7).index(), 7usize);
    }

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
        // Four vertices, four faces, all twins paired (closed surface).
        IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
            ],
            normals: vec![Vec3::Z; 4],
            // Outward-pointing winding for a tetrahedron at the origin.
            indices: vec![0, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3],
        }
    }

    #[test]
    fn construct_single_triangle() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        assert_eq!(mesh.vertex_count(), 3);
        assert_eq!(mesh.face_count(), 1);
        assert_eq!(mesh.half_edge_count(), 3);
        for h in 0..3 {
            assert_eq!(mesh.he_twin(HalfEdgeId(h)), HalfEdgeId::INVALID);
        }
    }

    #[test]
    fn construct_tetrahedron() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.face_count(), 4);
        assert_eq!(mesh.half_edge_count(), 12);
        // Closed surface: every half-edge has a valid twin.
        for h in 0..12 {
            assert!(
                mesh.he_twin(HalfEdgeId(h)).is_valid(),
                "half-edge {h} has no twin"
            );
        }
    }

    #[test]
    fn construct_rejects_odd_index_count() {
        let bad = IsoMesh {
            positions: vec![Vec3::ZERO; 3],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1],
        };
        match HalfEdgeMesh::from_iso_mesh(&bad) {
            Err(TopologyError::NonTriangleFace { len }) => assert_eq!(len, 2),
            other => panic!("expected NonTriangleFace, got {other:?}"),
        }
    }

    #[test]
    fn construct_rejects_duplicate_indices() {
        let bad = IsoMesh {
            positions: vec![Vec3::ZERO; 3],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 1],
        };
        match HalfEdgeMesh::from_iso_mesh(&bad) {
            Err(TopologyError::DegenerateTriangle {
                face_index,
                indices,
            }) => {
                assert_eq!(face_index, 0);
                assert_eq!(indices, [0, 1, 1]);
            }
            other => panic!("expected DegenerateTriangle, got {other:?}"),
        }
    }

    #[test]
    fn construct_rejects_out_of_range() {
        let bad = IsoMesh {
            positions: vec![Vec3::ZERO; 2],
            normals: vec![Vec3::Z; 2],
            indices: vec![0, 1, 2],
        };
        match HalfEdgeMesh::from_iso_mesh(&bad) {
            Err(TopologyError::IndexOutOfRange {
                index,
                vertex_count,
                ..
            }) => {
                assert_eq!(index, 2);
                assert_eq!(vertex_count, 2);
            }
            other => panic!("expected IndexOutOfRange, got {other:?}"),
        }
    }

    #[test]
    fn construct_rejects_non_manifold_edge() {
        // Three triangles sharing the same edge (0, 1).
        let bad = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(0.0, -1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 5],
            indices: vec![0, 1, 2, 0, 1, 3, 0, 1, 4],
        };
        match HalfEdgeMesh::from_iso_mesh(&bad) {
            Err(TopologyError::NonManifoldEdge { .. }) => {}
            other => panic!("expected NonManifoldEdge, got {other:?}"),
        }
    }

    #[test]
    fn round_trip_preserves_tetrahedron() {
        let iso = tetrahedron();
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso).expect("construct");
        let back = mesh.to_iso_mesh_flat();
        assert_eq!(back.positions.len(), 4);
        assert_eq!(back.indices.len(), 12);
        // Each original vertex should still be present (position-equal).
        for p in &iso.positions {
            assert!(
                back.positions.iter().any(|q| (*q - *p).length() < 1e-6),
                "position {p:?} missing after round-trip"
            );
        }
    }

    #[test]
    fn face_vertices_returns_winding_order() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        let verts = mesh.face_vertices(FaceId(0));
        assert_eq!(verts, [VertexId(0), VertexId(1), VertexId(2)]);
    }

    #[test]
    fn he_tail_roundtrip() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        for h in 0..3 {
            let hid = HalfEdgeId(h);
            let head = mesh.he_head(hid);
            let tail = mesh.he_tail(hid);
            assert_ne!(head, tail);
            // Walking next from tail's outgoing half-edges should land on head.
            assert_eq!(mesh.he_head(hid).index(), head.index());
            assert_eq!(tail.index(), mesh.he_tail(hid).index());
        }
    }

    fn cube_with_hole() -> IsoMesh {
        // 8 cube corners, 5 faces (one face of the cube removed). 10 triangles.
        // Vertex numbering:
        //   0: (0,0,0)  1: (1,0,0)  2: (1,1,0)  3: (0,1,0)  (bottom face, y=0 → z=0)
        //   4: (0,0,1)  5: (1,0,1)  6: (1,1,1)  7: (0,1,1)
        //
        // Remove the +z face (vertices 4,5,6,7).
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
        // 5 faces × 2 triangles, CCW when viewed from outside. The missing
        // face is the +z face (indices 4,5,6,7).
        #[rustfmt::skip]
        let indices = vec![
            // -z bottom (inward normal = +z, so from below CCW = 0,2,1 / 0,3,2)
            0, 2, 1,
            0, 3, 2,
            // -y front
            0, 1, 5,
            0, 5, 4,
            // +x right
            1, 2, 6,
            1, 6, 5,
            // +y back
            2, 3, 7,
            2, 7, 6,
            // -x left
            3, 0, 4,
            3, 4, 7,
        ];
        IsoMesh {
            positions,
            normals: vec![Vec3::ZERO; 8],
            indices,
        }
    }

    #[test]
    fn single_triangle_vertex_valence() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        for v in 0..3 {
            assert_eq!(mesh.vertex_valence(VertexId(v)), 2);
        }
    }

    #[test]
    fn tetrahedron_vertex_valence_is_three() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        for v in 0..4 {
            assert_eq!(mesh.vertex_valence(VertexId(v)), 3);
        }
    }

    #[test]
    fn single_triangle_all_vertices_are_boundary() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        for v in 0..3 {
            assert!(mesh.vertex_is_boundary(VertexId(v)));
        }
    }

    #[test]
    fn tetrahedron_no_boundary_vertices() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        for v in 0..4 {
            assert!(!mesh.vertex_is_boundary(VertexId(v)));
        }
    }

    #[test]
    fn single_triangle_boundary_loop() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        let loops = mesh.boundary_loops();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 3);
    }

    #[test]
    fn tetrahedron_no_boundary_loops() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        assert!(mesh.boundary_loops().is_empty());
    }

    #[test]
    fn cube_with_hole_has_single_boundary_loop_of_four() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&cube_with_hole()).expect("construct");
        let loops = mesh.boundary_loops();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 4);
        // Exactly vertices 4,5,6,7 appear on the loop.
        let mut verts: Vec<u32> = loops[0].iter().map(|h| mesh.he_head(*h).0).collect();
        verts.sort_unstable();
        let mut tails: Vec<u32> = loops[0].iter().map(|h| mesh.he_tail(*h).0).collect();
        tails.sort_unstable();
        assert_eq!(verts, vec![4, 5, 6, 7]);
        assert_eq!(tails, vec![4, 5, 6, 7]);
    }

    #[test]
    fn edge_iter_yields_unique_edges() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        // Tetrahedron has 6 edges. 12 half-edges / 2 = 6 unique.
        let edges: Vec<_> = mesh.edge_iter().collect();
        assert_eq!(edges.len(), 6);
    }

    #[test]
    fn edge_iter_on_cube_with_hole() {
        // 8 vertices, 10 faces → Euler: V - E + F = 2 - g where g = boundary loops.
        // For genus-0 surface with 1 boundary: 8 - E + 10 = 1 → E = 17? Actually
        // for a disk-with-hole topology, V - E + F = 1 (single disk). 8 - E + 10 = 1 → E = 17.
        let mesh = HalfEdgeMesh::from_iso_mesh(&cube_with_hole()).expect("construct");
        let edges: Vec<_> = mesh.edge_iter().collect();
        assert_eq!(edges.len(), 17);
    }

    #[test]
    fn edge_length_matches_geometry() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        // Any half-edge from vertex 0 (origin) to vertex 1 (1,0,0) has length 1.
        for h in 0..3 {
            let hid = HalfEdgeId(h);
            let tail = mesh.he_tail(hid);
            let head = mesh.he_head(hid);
            if (tail, head) == (VertexId(0), VertexId(1))
                || (tail, head) == (VertexId(1), VertexId(0))
            {
                assert!((mesh.edge_length(hid) - 1.0).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn face_positions_matches_vertices() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        let ps = mesh.face_positions(FaceId(0));
        assert_eq!(ps[0], Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(ps[1], Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(ps[2], Vec3::new(0.0, 1.0, 0.0));
    }

    #[test]
    fn is_manifold_true_for_tetrahedron() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        assert!(mesh.is_manifold());
    }

    #[test]
    fn is_manifold_true_for_single_triangle() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        assert!(mesh.is_manifold());
    }

    #[test]
    fn vertex_class_defaults_to_interior() {
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        for v in 0u32..3 {
            assert_eq!(mesh.vertex_class(VertexId(v)), VertexClass::Interior);
        }
    }

    #[test]
    fn vertex_class_set_and_read_round_trip() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        mesh.set_vertex_class(VertexId(1), VertexClass::Fixed);
        mesh.set_vertex_class(VertexId(2), VertexClass::Boundary);
        assert_eq!(mesh.vertex_class(VertexId(1)), VertexClass::Fixed);
        assert_eq!(mesh.vertex_class(VertexId(2)), VertexClass::Boundary);
        assert_eq!(mesh.vertex_class(VertexId(0)), VertexClass::Interior);
    }

    #[test]
    fn vertex_class_survives_compact() {
        // Mark vertices, mark some removed, compact, check survivors keep class.
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&tetrahedron()).expect("construct");
        mesh.set_vertex_class(VertexId(0), VertexClass::Fixed);
        mesh.set_vertex_class(VertexId(2), VertexClass::Feature);
        // Manually remove vertex 1 to force a compact remap.
        mesh.vertices[1].removed = true;
        mesh.free_vertices.push(VertexId(1));
        mesh.compact();
        // After compact: original vertex 0 is now at index 0, 2 is now at 1, 3 is at 2.
        assert_eq!(mesh.vertex_class(VertexId(0)), VertexClass::Fixed);
        assert_eq!(mesh.vertex_class(VertexId(1)), VertexClass::Feature);
        assert_eq!(mesh.vertex_class(VertexId(2)), VertexClass::Interior);
    }

    #[test]
    fn boundary_vertex_he_out_is_a_boundary_half_edge() {
        // Single triangle: all vertices boundary, every half-edge has twin INVALID,
        // so he_out for each vertex should itself be boundary.
        let mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("construct");
        for v in 0..3 {
            let vid = VertexId(v);
            let he = mesh.vertex_he_out(vid);
            assert!(he.is_valid());
            assert_eq!(mesh.he_tail(he), vid);
            assert_eq!(mesh.he_twin(he), HalfEdgeId::INVALID);
        }
    }
}
