//! Scan-conversion of triangle meshes into a [`ScalarField`] for remeshing.
//!
//! Implements Phase 1 of the PolyMender pipeline [Ju 2004]: each input
//! triangle is recursively tested against octree cells (SAT triangle-cube
//! culling), and at the leaf level segment-triangle intersections produce
//! Hermite data (surface point + face normal) on every cell edge the
//! triangle crosses. After all triangles are processed, a BFS flood-fill
//! propagates inside/outside signs from the grid boundary (seeded as
//! outside) through the unit-length cell edges: edges marked as
//! intersections flip the sign, others keep it.
//!
//! The resulting [`ScannedMeshField`] implements [`ScalarField`] and plugs
//! directly into the existing dual-contouring pipeline via
//! `OctreeNode::build_with_scalar_field` — `index` answers corner signs,
//! `solve` answers leaf-edge hermite points, and `normal` returns the
//! face normal of the nearest stored intersection.
//!
//! # Limitations (Phase 1)
//!
//! Flood-fill sign propagation is only correct for watertight, consistently
//! oriented meshes. When the input has holes, gaps, or non-manifold
//! features, the "dual surface" of intersection edges has boundary cycles,
//! and the BFS produces incorrect signs through those holes. Phase 2 (the
//! paper's `detectProc` / `patchProc` / `signProc`) addresses this.

use std::collections::{HashMap, HashSet, VecDeque};

use glam::{IVec3, Vec3};

use crate::mesh::MeshData;

use super::indicators::{EDGE_MAP, PositionCode, code_to_pos, decode_cell};
use super::scalar_field::ScalarField;

/// Scalar field derived from a triangle mesh via PolyMender-style scan
/// conversion.
///
/// Construct via [`ScannedMeshField::from_mesh`]; pass by reference into the
/// existing dual-contouring pipeline the same way any other `ScalarField`
/// primitive is used.
pub struct ScannedMeshField {
    /// World units per grid unit (shared with the DC pipeline using this field).
    unit_size: f32,
    /// Grid coordinate of the minimum corner of the scan-conversion region.
    min_code: PositionCode,
    /// Number of grid corners per dimension (always `size_code + 1`).
    dims: IVec3,
    /// Per-corner inside/outside flag, laid out as `z * dims.x * dims.y + y * dims.x + x`.
    /// `true` = inside the mesh, `false` = outside.
    signs: Vec<bool>,
    /// Sparse hermite data for each unit-length cell edge the mesh crosses.
    edges: HashMap<EdgeKey, EdgeHit>,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub(super) struct EdgeKey {
    /// Grid coordinate of the edge's lower endpoint.
    pub(super) lower: PositionCode,
    /// Axis of the edge: 0 = +X, 1 = +Y, 2 = +Z.
    pub(super) axis: u8,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EdgeHit {
    /// World-space intersection point on the edge. For real intersection
    /// edges this is the triangle-plane crossing; for patch edges it is
    /// the edge midpoint (a synthesized location, see `is_patch`).
    pub(super) position: Vec3,
    /// World-space surface normal at the intersection. For real edges
    /// this is the source triangle's face normal; for patch edges it is
    /// a weighted blend of nearby real normals.
    pub(super) normal: Vec3,
    /// `true` iff this edge was synthesized by PolyMender sign-generation
    /// patching (not produced by scan-conversion from the input mesh).
    /// Consumers that prefer real data can filter on this.
    pub(super) is_patch: bool,
}

impl ScannedMeshField {
    /// Scan-converts a triangle mesh into a `ScalarField` over the grid
    /// rooted at `min_code` with extent `size_code` grid units.
    ///
    /// The mesh should lie entirely within the grid; triangles crossing the
    /// boundary are clipped implicitly (leaves outside the root are pruned).
    /// Memory use is `O((size_code + 1)³)` bytes for the sign array plus
    /// `O(#intersection_edges)` for the sparse hermite map.
    pub fn from_mesh(
        mesh: &MeshData,
        min_code: PositionCode,
        size_code: i32,
        unit_size: f32,
    ) -> Self {
        let dims = IVec3::splat(size_code + 1);
        let total = (dims.x * dims.y * dims.z) as usize;
        let mut signs = vec![false; total];
        let mut edges: HashMap<EdgeKey, EdgeHit> = HashMap::new();

        let tri_count = mesh.indices.len() / 3;
        for t in 0..tri_count {
            let i0 = mesh.indices[t * 3] as usize;
            let i1 = mesh.indices[t * 3 + 1] as usize;
            let i2 = mesh.indices[t * 3 + 2] as usize;
            let v0 = Vec3::from(mesh.vertices[i0].position);
            let v1 = Vec3::from(mesh.vertices[i1].position);
            let v2 = Vec3::from(mesh.vertices[i2].position);

            let normal_raw = (v1 - v0).cross(v2 - v0);
            if normal_raw.length_squared() < 1e-20 {
                continue;
            }
            let normal = normal_raw.normalize();

            scan_triangle(
                v0, v1, v2, normal, min_code, size_code, unit_size, &mut edges,
            );
        }

        // Phase 2 sign generation (paper §5): patch the dual surface so
        // the extended intersection-edge set `Ê = E ∪ patch_edges` has
        // empty boundary. Without this step, a mesh with holes produces
        // a dual surface with boundary cycles and the flood-fill below
        // yields inconsistent signs through the holes.
        patch_dual_surface(&mut edges, unit_size);

        flood_fill_signs(&edges, &mut signs, min_code, dims);

        Self {
            unit_size,
            min_code,
            dims,
            signs,
            edges,
        }
    }

    /// Returns `true` if the grid corner at `code` is inside the mesh.
    fn sign_at(&self, code: PositionCode) -> bool {
        let rel = code - self.min_code;
        if rel.x < 0
            || rel.y < 0
            || rel.z < 0
            || rel.x >= self.dims.x
            || rel.y >= self.dims.y
            || rel.z >= self.dims.z
        {
            return false;
        }
        let idx = (rel.z * self.dims.x * self.dims.y + rel.y * self.dims.x + rel.x) as usize;
        self.signs[idx]
    }

    /// Number of intersection edges recorded during scan-conversion
    /// (exposed for tests and diagnostics).
    #[cfg(test)]
    fn intersection_count(&self) -> usize {
        self.edges.len()
    }
}

impl ScalarField for ScannedMeshField {
    fn value(&self, p: Vec3) -> f32 {
        let code = PositionCode::new(
            (p.x / self.unit_size).round() as i32,
            (p.y / self.unit_size).round() as i32,
            (p.z / self.unit_size).round() as i32,
        );
        if self.sign_at(code) { -1.0 } else { 1.0 }
    }

    fn index(&self, code: PositionCode, _unit_size: f32) -> f32 {
        if self.sign_at(code) { -1.0 } else { 1.0 }
    }

    fn solve(&self, p1: Vec3, p2: Vec3) -> Option<Vec3> {
        let c1 = PositionCode::new(
            (p1.x / self.unit_size).round() as i32,
            (p1.y / self.unit_size).round() as i32,
            (p1.z / self.unit_size).round() as i32,
        );
        let c2 = PositionCode::new(
            (p2.x / self.unit_size).round() as i32,
            (p2.y / self.unit_size).round() as i32,
            (p2.z / self.unit_size).round() as i32,
        );
        let delta = c2 - c1;

        let (lower, axis) = if delta.x > 0 {
            (c1, 0u8)
        } else if delta.x < 0 {
            (c2, 0u8)
        } else if delta.y > 0 {
            (c1, 1u8)
        } else if delta.y < 0 {
            (c2, 1u8)
        } else if delta.z > 0 {
            (c1, 2u8)
        } else if delta.z < 0 {
            (c2, 2u8)
        } else {
            return Some((p1 + p2) * 0.5);
        };

        if let Some(hit) = self.edges.get(&EdgeKey { lower, axis }) {
            Some(hit.position)
        } else {
            Some((p1 + p2) * 0.5)
        }
    }

    fn normal(&self, p: Vec3) -> Vec3 {
        let mut best_d2 = f32::INFINITY;
        let mut best_n = Vec3::Z;
        for hit in self.edges.values() {
            let d2 = (hit.position - p).length_squared();
            if d2 < best_d2 {
                best_d2 = d2;
                best_n = hit.normal;
            }
        }
        if best_n.length_squared() > 1e-12 {
            best_n.normalize()
        } else {
            Vec3::Z
        }
    }

    fn gradient_offset(&self) -> f32 {
        self.unit_size
    }
}

// ---------------------------------------------------------------------------
// Scan-conversion internals
// ---------------------------------------------------------------------------

/// Recursive octree descent for one triangle. At each level we SAT-cull
/// children that the triangle cannot touch; at the leaf level (cell size
/// 1 grid unit) we test the 12 cell edges and record any intersection in
/// `edges`.
/// Mutates `edges` so the extended set `Ê = E ⊖ (⊖ᵢ Pᵢ)` has empty
/// boundary on the dual surface. Paper §5: detect odd faces → extract
/// cycles → patch each cycle → combine via symmetric difference.
///
/// Crucially this is a *symmetric* difference, not a union: two
/// per-cycle patches that share a primal edge cancel that edge. Using
/// union here would leave residual odd faces when boundary cycles are
/// close enough that their patches overlap (common on dirty meshes
/// with multiple holes).
///
/// Patch edges that don't collide with a real intersection edge get
/// synthesized Hermite data: position at the edge midpoint, normal
/// from the nearest real intersection edge at construction time.
fn patch_dual_surface(edges: &mut HashMap<EdgeKey, EdgeHit>, unit_size: f32) {
    let odd_faces = super::sign_gen::detect_odd_faces(edges);
    if odd_faces.is_empty() {
        return;
    }
    let cycles = super::sign_gen::extract_boundary_cycles(&odd_faces);

    // Combined patch set via per-edge parity (symmetric difference).
    let mut combined_patch: HashSet<EdgeKey> = HashSet::new();
    for cycle in cycles {
        let patch_edges = super::sign_gen::patch_cycle(&cycle);
        for pe in patch_edges {
            if !combined_patch.insert(pe) {
                combined_patch.remove(&pe);
            }
        }
    }

    // Snapshot real edges so nearest-normal lookup isn't polluted by
    // in-progress patch insertions (nor by upcoming removals).
    let real_snapshot: Vec<(Vec3, Vec3)> = edges
        .values()
        .filter(|h| !h.is_patch)
        .map(|h| (h.position, h.normal))
        .collect();

    // Apply Ê = E ⊖ combined_patch.
    for patch_edge in combined_patch {
        if edges.remove(&patch_edge).is_some() {
            // Was already in E (or a previous patch, which shouldn't
            // happen post-dedup); XOR removes it.
            continue;
        }
        let midpoint = edge_midpoint(patch_edge, unit_size);
        let normal = nearest_normal(&real_snapshot, midpoint);
        edges.insert(
            patch_edge,
            EdgeHit {
                position: midpoint,
                normal,
                is_patch: true,
            },
        );
    }
}

/// World-space midpoint of a primal cell edge.
fn edge_midpoint(edge: EdgeKey, unit_size: f32) -> Vec3 {
    let lower_world = code_to_pos(edge.lower, unit_size);
    let mut offset = Vec3::ZERO;
    offset[edge.axis as usize] = 0.5 * unit_size;
    lower_world + offset
}

/// Brute-force nearest-neighbor search over a (position, normal)
/// snapshot. Falls back to +Z if the snapshot is empty.
fn nearest_normal(candidates: &[(Vec3, Vec3)], point: Vec3) -> Vec3 {
    let mut best_d2 = f32::INFINITY;
    let mut best_n = Vec3::Z;
    for &(pos, n) in candidates {
        let d2 = (pos - point).length_squared();
        if d2 < best_d2 {
            best_d2 = d2;
            best_n = n;
        }
    }
    if best_n.length_squared() > 1e-12 {
        best_n.normalize()
    } else {
        Vec3::Z
    }
}

#[allow(clippy::too_many_arguments)]
fn scan_triangle(
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
    normal: Vec3,
    cell_min: PositionCode,
    cell_size: i32,
    unit_size: f32,
    edges: &mut HashMap<EdgeKey, EdgeHit>,
) {
    let world_min = code_to_pos(cell_min, unit_size);
    let world_max = code_to_pos(cell_min + PositionCode::splat(cell_size), unit_size);

    if !triangle_overlaps_box(v0, v1, v2, world_min, world_max) {
        return;
    }

    if cell_size == 1 {
        let corners = cube_corners(world_min, world_max);
        for (edge_idx, corner_pair) in EDGE_MAP.iter().enumerate() {
            let ci_lo = corner_pair[0];
            let ci_hi = corner_pair[1];
            let a = corners[ci_lo];
            let b = corners[ci_hi];
            if let Some(t) = segment_triangle_intersection(a, b, v0, v1, v2) {
                let hit_pos = a + (b - a) * t;
                let offset_lo = decode_cell(ci_lo);
                let lower_code = cell_min + offset_lo;
                let axis = (edge_idx / 4) as u8;
                edges
                    .entry(EdgeKey {
                        lower: lower_code,
                        axis,
                    })
                    .or_insert(EdgeHit {
                        position: hit_pos,
                        normal,
                        is_patch: false,
                    });
            }
        }
        return;
    }

    let half = cell_size / 2;
    for i in 0..8 {
        let offset = decode_cell(i);
        let child_min = cell_min + offset * half;
        scan_triangle(v0, v1, v2, normal, child_min, half, unit_size, edges);
    }
}

/// Returns the 8 world-space corners of an axis-aligned box in the same
/// order as `decode_cell(i)` (i.e. `x*4 + y*2 + z`).
fn cube_corners(box_min: Vec3, box_max: Vec3) -> [Vec3; 8] {
    [
        Vec3::new(box_min.x, box_min.y, box_min.z),
        Vec3::new(box_min.x, box_min.y, box_max.z),
        Vec3::new(box_min.x, box_max.y, box_min.z),
        Vec3::new(box_min.x, box_max.y, box_max.z),
        Vec3::new(box_max.x, box_min.y, box_min.z),
        Vec3::new(box_max.x, box_min.y, box_max.z),
        Vec3::new(box_max.x, box_max.y, box_min.z),
        Vec3::new(box_max.x, box_max.y, box_max.z),
    ]
}

/// Separating Axis Theorem test for triangle-box overlap.
///
/// Checks 13 potential separating axes:
/// - 3 box face normals (X, Y, Z),
/// - 1 triangle face normal,
/// - 9 cross products of box edges × triangle edges.
///
/// Returns `true` if none of them separates the triangle from the box.
///
/// Uses f32 arithmetic; a small epsilon skips degenerate cross products.
/// False positives are harmless (we just test more edges at leaf level);
/// false negatives could miss intersections.
fn triangle_overlaps_box(v0: Vec3, v1: Vec3, v2: Vec3, box_min: Vec3, box_max: Vec3) -> bool {
    let center = (box_min + box_max) * 0.5;
    let half = (box_max - box_min) * 0.5;

    let t0 = v0 - center;
    let t1 = v1 - center;
    let t2 = v2 - center;

    for a in 0..3 {
        let pmin = t0[a].min(t1[a]).min(t2[a]);
        let pmax = t0[a].max(t1[a]).max(t2[a]);
        if pmin > half[a] || pmax < -half[a] {
            return false;
        }
    }

    let tri_edges = [t1 - t0, t2 - t1, t0 - t2];
    let box_axes = [Vec3::X, Vec3::Y, Vec3::Z];
    for ba in &box_axes {
        for te in &tri_edges {
            let axis = ba.cross(*te);
            if axis.length_squared() < 1e-12 {
                continue;
            }
            let p0 = t0.dot(axis);
            let p1 = t1.dot(axis);
            let p2 = t2.dot(axis);
            let r = half.x * axis.x.abs() + half.y * axis.y.abs() + half.z * axis.z.abs();
            let pmin = p0.min(p1).min(p2);
            let pmax = p0.max(p1).max(p2);
            if pmin > r || pmax < -r {
                return false;
            }
        }
    }

    let tri_normal = tri_edges[0].cross(tri_edges[1]);
    if tri_normal.length_squared() > 1e-12 {
        let d = t0.dot(tri_normal);
        let r =
            half.x * tri_normal.x.abs() + half.y * tri_normal.y.abs() + half.z * tri_normal.z.abs();
        if d.abs() > r {
            return false;
        }
    }

    true
}

/// Möller–Trumbore ray-triangle intersection, parameterised on a finite
/// segment `[a, b]`. Returns `Some(t)` where `t ∈ [0, 1]` is the segment
/// parameter of the hit, or `None` if they don't intersect.
fn segment_triangle_intersection(a: Vec3, b: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    const EPS: f32 = 1e-7;
    let dir = b - a;
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = dir.cross(edge2);
    let det = edge1.dot(h);
    if det.abs() < EPS {
        return None;
    }
    let inv_det = 1.0 / det;
    let s = a - v0;
    let u = inv_det * s.dot(h);
    if !(-EPS..=1.0 + EPS).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = inv_det * dir.dot(q);
    if v < -EPS || u + v > 1.0 + EPS {
        return None;
    }
    let t = inv_det * edge2.dot(q);
    if (-EPS..=1.0 + EPS).contains(&t) {
        Some(t.clamp(0.0, 1.0))
    } else {
        None
    }
}

/// Propagates inside/outside signs across the full leaf-grid via BFS,
/// starting from the 8 root-cell corners (seeded as outside).
///
/// An edge key present in `edges` flips the sign when traversed; all other
/// unit edges keep the sign the same. For a watertight, consistently
/// oriented mesh the resulting sign field has a sign change exactly on
/// each intersection edge, which is what DC needs to extract the surface.
fn flood_fill_signs(
    edges: &HashMap<EdgeKey, EdgeHit>,
    signs: &mut [bool],
    min_code: PositionCode,
    dims: IVec3,
) {
    let total = (dims.x * dims.y * dims.z) as usize;
    let mut visited = vec![false; total];
    let mut queue: VecDeque<PositionCode> = VecDeque::new();

    let max_code = min_code + PositionCode::new(dims.x - 1, dims.y - 1, dims.z - 1);

    let root_corners = [
        PositionCode::new(min_code.x, min_code.y, min_code.z),
        PositionCode::new(min_code.x, min_code.y, max_code.z),
        PositionCode::new(min_code.x, max_code.y, min_code.z),
        PositionCode::new(min_code.x, max_code.y, max_code.z),
        PositionCode::new(max_code.x, min_code.y, min_code.z),
        PositionCode::new(max_code.x, min_code.y, max_code.z),
        PositionCode::new(max_code.x, max_code.y, min_code.z),
        PositionCode::new(max_code.x, max_code.y, max_code.z),
    ];

    for c in root_corners {
        let idx = linear_index(c, min_code, dims);
        signs[idx] = false;
        visited[idx] = true;
        queue.push_back(c);
    }

    while let Some(c) = queue.pop_front() {
        let idx_c = linear_index(c, min_code, dims);
        let sign_c = signs[idx_c];
        for axis in 0..3usize {
            for dir in [-1i32, 1] {
                let mut delta = IVec3::ZERO;
                delta[axis] = dir;
                let neighbor = c + delta;
                let rel = neighbor - min_code;
                if rel.x < 0
                    || rel.y < 0
                    || rel.z < 0
                    || rel.x >= dims.x
                    || rel.y >= dims.y
                    || rel.z >= dims.z
                {
                    continue;
                }
                let n_idx = linear_index(neighbor, min_code, dims);
                if visited[n_idx] {
                    continue;
                }
                let lower = if dir > 0 { c } else { neighbor };
                let edge_key = EdgeKey {
                    lower,
                    axis: axis as u8,
                };
                let crosses = edges.contains_key(&edge_key);
                let sign_n = if crosses { !sign_c } else { sign_c };
                signs[n_idx] = sign_n;
                visited[n_idx] = true;
                queue.push_back(neighbor);
            }
        }
    }
}

fn linear_index(code: PositionCode, min_code: PositionCode, dims: IVec3) -> usize {
    let rel = code - min_code;
    (rel.z * dims.x * dims.y + rel.y * dims.x + rel.x) as usize
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use crate::isosurface::OctreeNode;
    use crate::mesh::{BoundingBox, MeshData, MeshVertex};

    fn make_cube_mesh(min: f32, max: f32) -> MeshData {
        build_cube_mesh(min, max, true)
    }

    /// Builds a cube (closed, 12 triangles) or an open-topped box (no
    /// +Z face, 10 triangles) in `[min, max]³`.
    fn build_cube_mesh(min: f32, max: f32, closed_top: bool) -> MeshData {
        let p = |x: f32, y: f32, z: f32| [x, y, z];
        let c000 = p(min, min, min);
        let c100 = p(max, min, min);
        let c010 = p(min, max, min);
        let c110 = p(max, max, min);
        let c001 = p(min, min, max);
        let c101 = p(max, min, max);
        let c011 = p(min, max, max);
        let c111 = p(max, max, max);

        // 12 (closed) or 10 (open-top) triangles, CCW from outside.
        let mut faces: Vec<([f32; 3], [f32; 3], [f32; 3])> = vec![
            // -Z face
            (c000, c110, c010),
            (c000, c100, c110),
            // -Y face
            (c000, c001, c101),
            (c000, c101, c100),
            // +Y face
            (c010, c110, c111),
            (c010, c111, c011),
            // -X face
            (c000, c011, c001),
            (c000, c010, c011),
            // +X face
            (c100, c101, c111),
            (c100, c111, c110),
        ];
        if closed_top {
            // +Z face
            faces.push((c001, c011, c111));
            faces.push((c001, c111, c101));
        }

        let mut vertices: Vec<MeshVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for (i, (a, b, c)) in faces.iter().enumerate() {
            let av = Vec3::from(*a);
            let bv = Vec3::from(*b);
            let cv = Vec3::from(*c);
            let n = (bv - av).cross(cv - av).normalize_or_zero();
            let normal = [n.x, n.y, n.z];
            let base = (i * 3) as u32;
            vertices.push(MeshVertex {
                position: *a,
                normal,
            });
            vertices.push(MeshVertex {
                position: *b,
                normal,
            });
            vertices.push(MeshVertex {
                position: *c,
                normal,
            });
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
        }

        let bbox = BoundingBox {
            min: Vec3::splat(min),
            max: Vec3::splat(max),
        };
        let volume = ((max - min) as f64).powi(3);
        MeshData {
            vertices,
            indices,
            bbox,
            volume,
        }
    }

    #[test]
    fn triangle_inside_box() {
        assert!(triangle_overlaps_box(
            Vec3::new(0.1, 0.1, 0.5),
            Vec3::new(0.9, 0.1, 0.5),
            Vec3::new(0.5, 0.9, 0.5),
            Vec3::ZERO,
            Vec3::ONE,
        ));
    }

    #[test]
    fn triangle_outside_box() {
        assert!(!triangle_overlaps_box(
            Vec3::new(2.0, 2.0, 2.0),
            Vec3::new(3.0, 2.0, 2.0),
            Vec3::new(2.5, 3.0, 2.0),
            Vec3::ZERO,
            Vec3::ONE,
        ));
    }

    #[test]
    fn triangle_piercing_box() {
        assert!(triangle_overlaps_box(
            Vec3::new(-1.0, 0.5, 0.5),
            Vec3::new(2.0, 0.5, 0.5),
            Vec3::new(0.5, 2.0, 0.5),
            Vec3::ZERO,
            Vec3::ONE,
        ));
    }

    #[test]
    fn segment_crosses_triangle_at_midpoint() {
        let t = segment_triangle_intersection(
            Vec3::new(0.25, 0.25, -1.0),
            Vec3::new(0.25, 0.25, 1.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!(t.is_some());
        let tv = t.unwrap();
        assert!((tv - 0.5).abs() < 1e-5);
    }

    #[test]
    fn segment_misses_triangle() {
        let t = segment_triangle_intersection(
            Vec3::new(2.0, 2.0, -1.0),
            Vec3::new(2.0, 2.0, 1.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!(t.is_none());
    }

    #[test]
    fn scan_cube_flood_fill_signs() {
        // Cube [0.123, 0.877]³ in a depth-5 grid over [0,1]³.
        let mesh = make_cube_mesh(0.123, 0.877);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1); // 16
        let unit_size = 1.0 / size_code as f32; // 0.0625
        let min_code = IVec3::ZERO;

        let field = ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size);
        assert!(field.intersection_count() > 0);

        // Corner of the grid — outside.
        assert!(!field.sign_at(IVec3::new(0, 0, 0)));
        // Center of the grid (world (0.5, 0.5, 0.5)) — inside.
        assert!(field.sign_at(IVec3::new(8, 8, 8)));
        // Far corner of the grid — outside.
        assert!(!field.sign_at(IVec3::new(16, 16, 16)));
    }

    #[test]
    fn open_top_cube_patched_interior_is_inside() {
        // Five-sided box (missing +Z face) at [0.15, 0.85]³ in a
        // depth-5 grid. Without Phase 2 sign generation, a naive BFS
        // would propagate "outside" through the missing top face and
        // mark the interior as outside. With PolyMender patching, the
        // boundary cycle around the hole is closed and interior corners
        // get the correct "inside" sign.
        let mesh = build_cube_mesh(0.15, 0.85, false);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1); // 16
        let unit_size = 1.0 / size_code as f32; // 0.0625
        let min_code = IVec3::ZERO;

        let field = ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size);

        // Interior corner near the open face: world (0.5, 0.5, 0.8125),
        // which is inside the bbox [0.15, 0.85]³. Without patching this
        // would be classified as outside (BFS sees a clear path from
        // the grid boundary through the hole).
        let near_top_interior = IVec3::new(8, 8, 13);
        assert!(
            field.sign_at(near_top_interior),
            "interior near open face must be inside after patching"
        );

        // Center of the box — should also be inside.
        assert!(
            field.sign_at(IVec3::new(8, 8, 8)),
            "box center must be inside"
        );

        // Grid corners outside the bbox — still outside.
        assert!(!field.sign_at(IVec3::new(0, 0, 0)));
        assert!(!field.sign_at(IVec3::new(16, 16, 16)));
    }

    #[test]
    fn open_top_cube_round_trip_through_dc_yields_repaired_mesh() {
        // End-to-end: scan-convert a five-sided box, let Phase 2 close
        // the boundary cycle, run the full DC pipeline, and verify the
        // extracted mesh is non-empty and covers the input bbox.
        // With Phase 1 alone the DC would produce an empty (or inverted)
        // mesh because the sign field through the hole is wrong.
        let mesh = build_cube_mesh(0.15, 0.85, false);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1);
        let unit_size = 1.0 / size_code as f32;
        let min_code = IVec3::ZERO;

        let field = ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size);

        let octree = OctreeNode::build_with_scalar_field(min_code, depth, &field, false, unit_size);
        let mut octree = octree.expect("Phase 2 patched box should produce a non-empty octree");
        OctreeNode::simplify(&mut octree, 0.0);
        let result = OctreeNode::extract_mesh(&mut octree, &field, unit_size);

        assert!(
            result.triangle_count() > 0,
            "repaired five-sided box should produce triangles"
        );

        // Bbox of the reconstruction should roughly match the input cube
        // (approximate because the patched top face is synthesized near
        // the original opening).
        let mut bb_min = Vec3::splat(f32::MAX);
        let mut bb_max = Vec3::splat(f32::MIN);
        for p in &result.positions {
            bb_min = bb_min.min(*p);
            bb_max = bb_max.max(*p);
        }
        let tol = unit_size * 3.0;
        assert!(
            (bb_min.x - 0.15).abs() < tol && (bb_min.y - 0.15).abs() < tol,
            "bbox min.xy close to 0.15, got {bb_min:?}"
        );
        assert!(
            (bb_max.x - 0.85).abs() < tol && (bb_max.y - 0.85).abs() < tol,
            "bbox max.xy close to 0.85, got {bb_max:?}"
        );
    }

    #[test]
    fn scan_cube_round_trip_through_dc() {
        let mesh = make_cube_mesh(0.123, 0.877);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1);
        let unit_size = 1.0 / size_code as f32;
        let min_code = IVec3::ZERO;

        let field = ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size);

        let octree = OctreeNode::build_with_scalar_field(min_code, depth, &field, false, unit_size);
        assert!(octree.is_some());
        let mut octree = octree.unwrap();
        OctreeNode::simplify(&mut octree, 0.0);
        let result = OctreeNode::extract_mesh(&mut octree, &field, unit_size);

        assert!(result.triangle_count() > 0);

        // Reconstructed bbox should be close to the input cube.
        let mut bb_min = Vec3::splat(f32::MAX);
        let mut bb_max = Vec3::splat(f32::MIN);
        for p in &result.positions {
            bb_min = bb_min.min(*p);
            bb_max = bb_max.max(*p);
        }
        let tol = unit_size * 2.0;
        assert!((bb_min.x - 0.123).abs() < tol, "bb_min.x = {}", bb_min.x);
        assert!((bb_min.y - 0.123).abs() < tol, "bb_min.y = {}", bb_min.y);
        assert!((bb_min.z - 0.123).abs() < tol, "bb_min.z = {}", bb_min.z);
        assert!((bb_max.x - 0.877).abs() < tol, "bb_max.x = {}", bb_max.x);
        assert!((bb_max.y - 0.877).abs() < tol, "bb_max.y = {}", bb_max.y);
        assert!((bb_max.z - 0.877).abs() < tol, "bb_max.z = {}", bb_max.z);
    }
}
