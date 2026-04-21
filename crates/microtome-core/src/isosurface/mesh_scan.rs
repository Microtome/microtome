//! Scan-conversion of triangle meshes into a [`ScalarField`] for remeshing.
//!
//! Two parallel passes over the grid:
//!
//! 1. **Edge-crossings pass.** Each input triangle is recursively tested
//!    against octree cells (SAT triangle-cube culling); at the leaf level
//!    a watertight Möller–Trumbore test (Woop 2013, axis-aligned
//!    specialisation) records the first triangle crossing per cell edge.
//!    These produce Hermite data (surface point + face normal) used by
//!    the DC vertex solver.
//!
//! 2. **Corner-signs pass.** Selectable at construction time via
//!    [`SignMode`]:
//!    - [`SignMode::Gwn`] uses the **generalized winding number**
//!      (Jacobson 2013): for a consistently-oriented mesh, `w(P)`
//!      counts how many components contain `P`; `w ≥ 0.5` gives the
//!      **set union** of components, which is the correct inside test
//!      for dirty meshes with multiple intersecting/overlapping
//!      solids. Robust to self-intersections and small holes.
//!    - [`SignMode::FloodFill`] BFS-propagates a known-outside seed
//!      across the grid, flipping sign on each edge with a recorded
//!      triangle hit. Linear in grid size, no BVH build; correct on
//!      clean watertight input but vulnerable to the well-known
//!      flood-fill failure modes (sign leaks through holes /
//!      non-manifold features).
//!
//! 3. **Missing-crossing synthesis (lazy).** For any grid edge whose
//!    two corners differ in sign but no real triangle crossed it, the
//!    Hermite data is borrowed from the closest real hit on a
//!    *cousin* edge — one of the (up to) 10 grid edges incident on
//!    this edge's two endpoint vertices. This is the common case for
//!    watertight meshes: the M-T tie-break records a near-vertex
//!    crossing on exactly one incident edge, leaving its cousins as
//!    "missing" even though they all describe the same surface. Only
//!    when no real cousin exists (genuine hole or grid boundary) do
//!    we fall back to a midpoint position with a nearest-anywhere
//!    normal. Synthesis runs **on demand** inside
//!    [`ScannedMeshField::hermite`] and the result is cached, so we
//!    only pay for edges the DC octree actually queries.
//!
//! Sign-gen and synthesis are both lazy under [`SignMode::Gwn`]:
//! `from_mesh` builds the BVH eagerly but defers all per-corner GWN
//! queries to `index()` calls from the octree builder. Combined
//! with `has_surface_in`-based octree pruning (which short-circuits
//! homogeneous interior/exterior subtrees entirely), the per-grid
//! cost scales with surface area instead of bounding-box volume.
//!
//! The resulting [`ScannedMeshField`] implements [`ScalarField`] and plugs
//! directly into the existing dual-contouring pipeline via
//! `OctreeNode::build_with_scalar_field` — `index` answers corner signs,
//! `solve` answers leaf-edge hermite points, and `normal` returns the
//! face normal of the nearest stored intersection.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use glam::{IVec3, Vec3};

use crate::mesh::MeshData;

use super::indicators::{EDGE_MAP, PositionCode, code_to_pos, decode_cell};
use super::mesh_bvh::MeshBvh;
use super::polymender;
use super::scalar_field::ScalarField;

/// Selects how corner inside/outside flags are computed during
/// [`ScannedMeshField::from_mesh`].
///
/// Picked at runtime so the viewer (and tests) can A/B between the
/// three supported sign-generation paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignMode {
    /// Generalized winding number via [`MeshBvh`] (Jacobson 2013,
    /// Barill 2018). `w(P) >= 0.5` ⇒ inside. Robust to
    /// self-intersections, intersecting components, and small holes
    /// — none of these can flip propagation the way they can for
    /// flood-fill. Pays an O(N log N) BVH build plus an O(log N) GWN
    /// query per grid corner; for large meshes / fine grids this
    /// dominates the scan-conversion cost.
    Gwn,
    /// Edge-parity flood-fill from the grid's `(0,0,0)` corner
    /// (assumed outside). The sign flips whenever the BFS crosses an
    /// edge with a recorded triangle hit; otherwise it propagates.
    /// Linear in grid size, no BVH build. Correct for clean closed
    /// meshes; vulnerable to self-intersections, non-manifold
    /// boundaries, and holes (sign can leak through and invert a
    /// whole region).
    FloodFill,
    /// PolyMender (Ju 2004, "Robust Repair of Polygonal Models"):
    /// scan-convert, detect boundary cycles on the dual surface
    /// (primal cell faces with odd intersection-edge count), patch
    /// the cycles with synthetic intersection edges so `∂S = ∅`,
    /// then flood-fill signs on the augmented edge set. Closes
    /// holes in the input mesh instead of leaving GWN's smooth
    /// taper across them.
    Polymender,
}

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
    /// Real triangle-plane crossings recorded during scan-conversion.
    /// Patch synthesis (cousin lookup / midpoint fallback) writes into
    /// `patch_cache` instead of mutating this map, so cousin queries
    /// always see only authentic Hermite data.
    edges: HashMap<EdgeKey, EdgeHit>,
    /// Cells (at every depth from `size_code` down to 1) that any input
    /// triangle overlapped during scan-conversion. Read by
    /// [`Self::has_surface_in`] so the DC octree builder can short-
    /// circuit homogeneous interior/exterior regions instead of
    /// recursing all the way to leaf level just to discover there's
    /// nothing there.
    active_cells: HashSet<(PositionCode, i32)>,
    /// Per-corner inside/outside flag, encoded as `0 = unknown,
    /// 1 = outside, 2 = inside` so a single allocation serves both
    /// modes. For [`SignMode::FloodFill`] this is fully populated by
    /// BFS in `from_mesh`. For [`SignMode::Gwn`] it starts all-zero
    /// and is filled lazily on first `index()` query via the BVH;
    /// combined with the DC octree's `has_surface_in` pruning, the
    /// vast majority of slots are never computed.
    sign_cache: RefCell<Vec<u8>>,
    /// `Some` for [`SignMode::Gwn`] — used to compute corner signs
    /// on demand. `None` for [`SignMode::FloodFill`], where signs are
    /// pre-populated.
    bvh: Option<MeshBvh>,
    /// Lazily-synthesized Hermite data for sign-changing edges that
    /// have no real triangle crossing (M-T tie-break artefacts).
    /// Populated on first `hermite()` query that misses `edges`.
    patch_cache: RefCell<HashMap<EdgeKey, EdgeHit>>,
}

#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub(crate) struct EdgeKey {
    /// Grid coordinate of the edge's lower endpoint.
    pub(crate) lower: PositionCode,
    /// Axis of the edge: 0 = +X, 1 = +Y, 2 = +Z.
    pub(crate) axis: u8,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct EdgeHit {
    /// World-space intersection point. For real edges this is the
    /// M-T crossing on the edge itself; for cousin-derived patches
    /// it is the cousin's stored position (typically near the shared
    /// grid vertex); for midpoint fallback patches it is the edge
    /// midpoint.
    pub(super) position: Vec3,
    /// World-space surface normal at `position`. For real edges this
    /// is the source triangle's face normal; patches inherit the
    /// cousin's normal (or the nearest real normal as a last resort).
    pub(super) normal: Vec3,
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
        sign_mode: SignMode,
    ) -> Self {
        let dims = IVec3::splat(size_code + 1);
        let total = (dims.x * dims.y * dims.z) as usize;
        let mut sign_storage = vec![0u8; total];
        let mut edges: HashMap<EdgeKey, EdgeHit> = HashMap::new();
        let mut active_cells: HashSet<(PositionCode, i32)> = HashSet::new();

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

            // First triangle to hit any given cell edge wins; subsequent
            // hits on the same edge (from this triangle via shared leaf
            // cells, or from other triangles at the same intersection
            // curve) are ignored. The crossing is used only for DC vertex
            // placement — corner *signs* come from GWN below, not from
            // edge-parity counting, so double-hits don't corrupt the
            // sign field the way they do for flood-fill pipelines.
            scan_triangle(
                v0,
                v1,
                v2,
                normal,
                min_code,
                size_code,
                unit_size,
                &mut edges,
                &mut active_cells,
            );
        }

        let bvh = match sign_mode {
            SignMode::Gwn => {
                // Build the BVH eagerly (one-shot cost) but defer all
                // GWN queries to `index()` so we only pay for corners
                // the DC octree actually visits — an order of magnitude
                // fewer than the full grid for typical sparse meshes.
                Some(MeshBvh::build(mesh))
            }
            SignMode::FloodFill => {
                // Flood-fill is inherently global (BFS from a known-
                // outside seed), so populate the cache eagerly.
                let key_set: HashSet<EdgeKey> = edges.keys().copied().collect();
                flood_fill_signs(&key_set, &mut sign_storage, min_code, dims);
                None
            }
            SignMode::Polymender => {
                // PolyMender §5.2 + §5.3: detect odd-parity cell
                // faces on the dual surface, solve `M x = b` over
                // GF(2) to find synthetic edges whose toggles zero
                // the parity. §5.4 (the same `flood_fill_signs` the
                // plain flood-fill path uses) then propagates a
                // consistent sign configuration over `S ⊕ P`.
                let real_keys: HashSet<EdgeKey> = edges.keys().copied().collect();
                let patch_keys = polymender::compute_patch_edges(&real_keys);
                // Symmetric difference `S ⊕ P`: anything in exactly
                // one of the two sets.
                let mut augmented: HashSet<EdgeKey> = real_keys;
                for e in &patch_keys {
                    if !augmented.insert(*e) {
                        augmented.remove(e);
                    }
                }
                flood_fill_signs(&augmented, &mut sign_storage, min_code, dims);
                // Seed Hermite entries for patch edges that are
                // genuinely synthetic. Position = edge midpoint;
                // normal = the cousin-averaged normal from nearby
                // real hits. The paper's §5.3 creates patch edges
                // "by intersecting h with the cell edges and
                // interpolating the normals associated with the end
                // points of h" — we approximate by averaging real
                // neighbour normals, which keeps the patch smoothly
                // continuous with the surrounding surface instead of
                // introducing axis-aligned flat plates.
                for edge in patch_keys {
                    if edges.contains_key(&edge) {
                        continue;
                    }
                    let position = edge_midpoint(edge, unit_size);
                    let normal = average_neighbour_normal(edge, &edges, unit_size);
                    edges.insert(edge, EdgeHit { position, normal });
                }
                None
            }
        };

        Self {
            unit_size,
            min_code,
            dims,
            edges,
            active_cells,
            sign_cache: RefCell::new(sign_storage),
            bvh,
            patch_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Returns `true` if the grid corner at `code` is inside the mesh.
    /// For [`SignMode::Gwn`] this lazily computes (and caches) the sign
    /// via the BVH; for [`SignMode::FloodFill`] the cache was eagerly
    /// populated and this is a Vec lookup.
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
        // Read-only fast path for already-known cells.
        {
            let cache = self.sign_cache.borrow();
            let cached = cache[idx];
            if cached != 0 {
                return cached == 2;
            }
        }
        // Lazy compute via BVH GWN; FloodFill's cache should never be
        // 0 here (the BFS visits every reachable corner from the seed).
        // If we land here in FloodFill mode it's an unreachable seed
        // (degenerate grid) — treat as outside, the safe default.
        let bvh = match &self.bvh {
            Some(b) => b,
            None => return false,
        };
        let world_pos = code_to_pos(code, self.unit_size);
        let inside = bvh.winding_number(world_pos) >= 0.5;
        self.sign_cache.borrow_mut()[idx] = if inside { 2 } else { 1 };
        inside
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

    fn has_surface_in(&self, min_code: PositionCode, cell_size: i32, _unit_size: f32) -> bool {
        // Cells the scan-conversion never visited contain no input
        // triangles, so their corners are necessarily homogeneous and
        // no descendant could produce a DC vertex. The octree builder
        // can prune the entire subtree without paying for a single
        // corner sign query.
        self.active_cells.contains(&(min_code, cell_size))
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

    fn hermite(&self, p1: Vec3, p2: Vec3) -> Option<(Vec3, Vec3)> {
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
            return Some(((p1 + p2) * 0.5, Vec3::Z));
        };
        let key = EdgeKey { lower, axis };

        // 1. Real M-T crossing.
        if let Some(hit) = self.edges.get(&key) {
            return Some((hit.position, normalize_or_z(hit.normal)));
        }
        // 2. Previously-synthesized patch.
        if let Some(hit) = self.patch_cache.borrow().get(&key) {
            return Some((hit.position, normalize_or_z(hit.normal)));
        }
        // 3. Synthesize: cousin first, midpoint as last resort. Cache
        //    so subsequent queries on the same edge are O(1).
        let patch =
            synthesize_from_cousins(key, &self.edges, self.unit_size).unwrap_or_else(|| {
                let position = edge_midpoint(key, self.unit_size);
                let normal = nearest_normal_in_field(self.edges.values(), position);
                EdgeHit { position, normal }
            });
        self.patch_cache.borrow_mut().insert(key, patch);
        Some((patch.position, normalize_or_z(patch.normal)))
    }

    fn normal(&self, p: Vec3) -> Vec3 {
        normalize_or_z(nearest_normal_in_field(self.edges.values(), p))
    }

    fn gradient_offset(&self) -> f32 {
        self.unit_size
    }
}

// ---------------------------------------------------------------------------
// Scan-conversion internals
// ---------------------------------------------------------------------------

/// World-space midpoint of a primal cell edge.
fn edge_midpoint(edge: EdgeKey, unit_size: f32) -> Vec3 {
    let lower_world = code_to_pos(edge.lower, unit_size);
    let mut offset = Vec3::ZERO;
    offset[edge.axis as usize] = 0.5 * unit_size;
    lower_world + offset
}

/// Returns the normal of the hit closest to `point`, or `Vec3::Z` if
/// the iterator is empty. Brute-force, used as the last-resort fallback
/// when a missing edge has no real cousin to inherit from.
fn nearest_normal_in_field<'a, I: IntoIterator<Item = &'a EdgeHit>>(
    candidates: I,
    point: Vec3,
) -> Vec3 {
    let mut best_d2 = f32::INFINITY;
    let mut best_n = Vec3::Z;
    for hit in candidates {
        let d2 = (hit.position - point).length_squared();
        if d2 < best_d2 {
            best_d2 = d2;
            best_n = hit.normal;
        }
    }
    best_n
}

/// Normalizes `n` if non-zero, else returns `Vec3::Z` as a safe default
/// (a zero normal would contribute no constraint to the QEF anyway).
fn normalize_or_z(n: Vec3) -> Vec3 {
    if n.length_squared() > 1e-12 {
        n.normalize()
    } else {
        Vec3::Z
    }
}

/// Averages the normals of real hits within a local neighbourhood of
/// `edge`'s midpoint, weighted by inverse squared distance. Used to
/// seed Hermite data on PolyMender patch edges so the synthetic
/// surface blends smoothly into the surrounding hole rim rather than
/// introducing an axis-aligned plate. The weighting keeps the sum
/// bounded when hits lie exactly on the midpoint, and falls off
/// quickly past a cell's worth of distance so only local hits
/// dominate.
fn average_neighbour_normal(
    edge: EdgeKey,
    edges: &HashMap<EdgeKey, EdgeHit>,
    unit_size: f32,
) -> Vec3 {
    let midpoint = edge_midpoint(edge, unit_size);
    let radius_sq = (unit_size * 3.0).powi(2);
    let mut acc = Vec3::ZERO;
    let mut weight_sum = 0.0f32;
    for hit in edges.values() {
        let d2 = (hit.position - midpoint).length_squared();
        if d2 > radius_sq {
            continue;
        }
        let w = 1.0 / (d2 + 1e-6);
        acc += hit.normal * w;
        weight_sum += w;
    }
    if weight_sum > 0.0 {
        normalize_or_z(acc / weight_sum)
    } else {
        // No neighbours at all — fall back to the edge's own axis so
        // the QEF gets *some* constraint rather than a zero normal.
        let mut n = Vec3::ZERO;
        n[edge.axis as usize] = 1.0;
        n
    }
}

/// Returns the closest real hit on a "cousin" grid edge — one of the
/// (up to) 10 edges sharing an endpoint vertex with `edge`. Used to
/// repair watertight tie-break artefacts: when a triangle grazes a
/// shared grid vertex, exactly one incident edge wins the M-T crossing,
/// and the others' GWN sign-flipping endpoints turn into "missing"
/// edges.
///
/// Cousins are ranked by perpendicular distance from their hit to the
/// *line through the requested edge*, not by distance to the shared
/// endpoint vertex. The reason: at sharp features (e.g. gear-tooth
/// tips, where two flanks meet at a ridge close to a grid vertex)
/// cousins from each flank are equidistant from the shared vertex, and
/// vertex-distance ranking can pick a hit on the *wrong* flank — its
/// tangent plane then pulls the QEF off-feature, visibly chipping the
/// flank surface in the DC output. Perpendicular-to-edge ranking
/// naturally prefers hits whose surface piece actually crosses the
/// requested edge.
fn synthesize_from_cousins(
    edge: EdgeKey,
    edges: &HashMap<EdgeKey, EdgeHit>,
    unit_size: f32,
) -> Option<EdgeHit> {
    let mut axis_unit = IVec3::ZERO;
    axis_unit[edge.axis as usize] = 1;
    let v_lo = edge.lower;
    let v_hi = edge.lower + axis_unit;
    let v_lo_world = code_to_pos(v_lo, unit_size);
    let v_hi_world = code_to_pos(v_hi, unit_size);
    let edge_vec = v_hi_world - v_lo_world;
    let edge_len_sq = edge_vec.length_squared();

    let mut best_d2 = f32::INFINITY;
    let mut best: Option<EdgeHit> = None;

    for &(vertex_code, _) in &[(v_lo, v_lo_world), (v_hi, v_hi_world)] {
        for cousin_axis in 0..3u8 {
            for direction in [-1i32, 1] {
                // Skip the original edge itself (the only +axis-aligned
                // outgoing cousin at v_lo and the only −axis-aligned
                // incoming cousin at v_hi).
                if cousin_axis == edge.axis
                    && ((vertex_code == v_lo && direction == 1)
                        || (vertex_code == v_hi && direction == -1))
                {
                    continue;
                }
                let cousin_key = if direction == 1 {
                    EdgeKey {
                        lower: vertex_code,
                        axis: cousin_axis,
                    }
                } else {
                    let mut step = IVec3::ZERO;
                    step[cousin_axis as usize] = -1;
                    EdgeKey {
                        lower: vertex_code + step,
                        axis: cousin_axis,
                    }
                };
                let Some(hit) = edges.get(&cousin_key) else {
                    continue;
                };
                // Perpendicular distance from cousin hit to the line
                // through the requested edge. Hits on the same surface
                // piece our edge crosses sit close to that line; hits
                // from a neighbouring sharp feature sit further off.
                let to_hit = hit.position - v_lo_world;
                let along = if edge_len_sq > 0.0 {
                    to_hit.dot(edge_vec) / edge_len_sq
                } else {
                    0.0
                };
                let on_line = v_lo_world + edge_vec * along;
                let perp_d2 = (hit.position - on_line).length_squared();
                if perp_d2 < best_d2 {
                    best_d2 = perp_d2;
                    best = Some(*hit);
                }
            }
        }
    }

    best.map(|h| EdgeHit {
        position: h.position,
        normal: h.normal,
    })
}

/// BFS-based corner sign computation. Seeds the grid `(0,0,0)` corner
/// as outside (the mesh is required to fit inside the scan region), then
/// 6-connected propagates: across an edge with a recorded triangle hit
/// the sign flips; otherwise it stays. O(grid corners). Linear, no BVH
/// build, no per-corner GWN. Correct on clean watertight input;
/// non-manifold or self-intersecting features can flip the sign of a
/// downstream region.
fn flood_fill_signs(
    edges: &HashSet<EdgeKey>,
    signs: &mut [u8],
    min_code: PositionCode,
    dims: IVec3,
) {
    let stride_y = dims.x;
    let stride_z = dims.x * dims.y;
    let total = (dims.x * dims.y * dims.z) as usize;
    let mut visited = vec![false; total];
    let mut queue: VecDeque<IVec3> = VecDeque::new();

    // Seed at relative (0,0,0). Sign starts `1 = outside`.
    signs[0] = 1;
    visited[0] = true;
    queue.push_back(IVec3::ZERO);

    while let Some(curr) = queue.pop_front() {
        let curr_idx = (curr.z * stride_z + curr.y * stride_y + curr.x) as usize;
        let curr_sign = signs[curr_idx];
        for axis in 0..3usize {
            for direction in [-1i32, 1] {
                let mut neighbor = curr;
                neighbor[axis] += direction;
                if neighbor.x < 0
                    || neighbor.y < 0
                    || neighbor.z < 0
                    || neighbor.x >= dims.x
                    || neighbor.y >= dims.y
                    || neighbor.z >= dims.z
                {
                    continue;
                }
                let neighbor_idx =
                    (neighbor.z * stride_z + neighbor.y * stride_y + neighbor.x) as usize;
                if visited[neighbor_idx] {
                    continue;
                }
                // The grid edge between `curr` and `neighbor` is keyed
                // by whichever endpoint is the lower along `axis`,
                // in *absolute* grid coordinates (same frame as
                // scan_triangle's inserts and polymender's patch
                // edges).
                let lower = if direction > 0 { curr } else { neighbor };
                let edge_key = EdgeKey {
                    lower: min_code + lower,
                    axis: axis as u8,
                };
                let crosses = edges.contains(&edge_key);
                // Toggle between 1 (outside) and 2 (inside) on each
                // crossed edge; carry the sign across non-crossed
                // edges. The 0 sentinel is reserved for "unknown" in
                // the GWN code path and never appears here.
                signs[neighbor_idx] = if crosses { 3 - curr_sign } else { curr_sign };
                visited[neighbor_idx] = true;
                queue.push_back(neighbor);
            }
        }
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
    active_cells: &mut HashSet<(PositionCode, i32)>,
) {
    let world_min = code_to_pos(cell_min, unit_size);
    let world_max = code_to_pos(cell_min + PositionCode::splat(cell_size), unit_size);

    if !triangle_overlaps_box(v0, v1, v2, world_min, world_max) {
        return;
    }

    // Record this cell at this depth so the DC octree builder can
    // skip homogeneous regions via `ScalarField::has_surface_in`.
    active_cells.insert((cell_min, cell_size));

    if cell_size == 1 {
        let corners = cube_corners(world_min, world_max);
        for (edge_idx, corner_pair) in EDGE_MAP.iter().enumerate() {
            let ci_lo = corner_pair[0];
            let ci_hi = corner_pair[1];
            let a = corners[ci_lo];
            let b = corners[ci_hi];
            let axis = (edge_idx / 4) as u8;
            let offset_lo = decode_cell(ci_lo);
            let key = EdgeKey {
                lower: cell_min + offset_lo,
                axis,
            };
            // First hit wins. Neighboring leaf cells share this primal
            // edge (up to 4×), and at mesh-intersection curves multiple
            // triangles may cross the same cell edge; either way we keep
            // one deterministic crossing for DC vertex placement. (With
            // GWN-based corner signs, duplicate crossings are not a
            // correctness problem — they were only an issue for the
            // earlier edge-parity + flood-fill pipeline.)
            if edges.contains_key(&key) {
                continue;
            }
            if let Some(t) = segment_triangle_intersection(a, b, axis, v0, v1, v2) {
                let hit_pos = a + (b - a) * t;
                edges.insert(
                    key,
                    EdgeHit {
                        position: hit_pos,
                        normal,
                    },
                );
            }
        }
        return;
    }

    let half = cell_size / 2;
    for i in 0..8 {
        let offset = decode_cell(i);
        let child_min = cell_min + offset * half;
        scan_triangle(
            v0,
            v1,
            v2,
            normal,
            child_min,
            half,
            unit_size,
            edges,
            active_cells,
        );
    }
}

/// Brute-force generalized winding number (Jacobson 2013). Kept as
/// ground truth for the GWN unit tests; the production scan uses
/// [`super::mesh_bvh::MeshBvh`] for a dramatic speedup on non-trivial
/// meshes.
#[cfg(test)]
fn generalized_winding_number(point: Vec3, mesh: &MeshData) -> f32 {
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
        // Point coincident with a mesh vertex — skip this triangle.
        // (Total winding stays well-defined via neighboring triangles;
        // the surface itself is not a well-posed place to query anyway.)
        if la < 1e-20 || lb < 1e-20 || lc < 1e-20 {
            continue;
        }
        let num = a.dot(b.cross(c));
        let denom = la * lb * lc + a.dot(b) * lc + b.dot(c) * la + c.dot(a) * lb;
        // 2 * atan2(num, denom) = signed solid angle [−2π, 2π]. Summed and
        // divided by 4π gives the winding number.
        accum += num.atan2(denom);
    }
    accum / std::f32::consts::TAU
}

// `synthesize_missing_crossings` (the eager full-grid synthesis pass)
// has been removed. Patch synthesis is now performed on-demand by
// [`ScannedMeshField::hermite`] when an edge query lands on a grid
// edge with no recorded triangle crossing — typically the M-T tie-
// break artefact described above. Doing it lazily means we only pay
// for edges the DC octree actually queries (a small fraction of the
// grid for sparse meshes), and we no longer need a separate
// `is_patch` flag on `EdgeHit` to distinguish real from synthesised
// data — patches live in `patch_cache`, real hits in `edges`.

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

/// Watertight segment-triangle intersection for axis-aligned rays
/// (Woop 2013, "Watertight Ray/Triangle Intersection"; specialised for
/// an axis-aligned `+axis` direction so no shear is required).
///
/// The segment runs from `a` to `b` along the positive direction of
/// `axis` (0=X, 1=Y, 2=Z). Returns `Some(t)` where `t ∈ [0, 1]` is the
/// segment parameter of the hit, or `None` if they don't intersect.
///
/// Watertight guarantees:
/// - Two triangles sharing an edge, ray on that edge → **exactly one**
///   triangle reports a hit.
/// - Multiple triangles meeting at a vertex, ray on that vertex →
///   **exactly one** triangle reports a hit.
///
/// These guarantees make parity-based crossing counts (XOR tracking)
/// robust on face-diagonal seams and other shared-primitive cases.
fn segment_triangle_intersection(
    a: Vec3,
    b: Vec3,
    axis: u8,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
) -> Option<f32> {
    // Woop's cyclic (kx, ky, kz) convention: with direction along +kz and
    // a *positive* dominant component, no kx/ky swap is needed. Our edges
    // always run lo→hi along +axis (see `EDGE_MAP` ordering), so skip.
    let (kx, ky, kz) = match axis {
        0 => (1usize, 2usize, 0usize),
        1 => (2usize, 0usize, 1usize),
        2 => (0usize, 1usize, 2usize),
        _ => return None,
    };

    let seg_len = b[kz] - a[kz];
    if seg_len <= 0.0 {
        return None;
    }

    // Project relative to the segment origin. No shear: the ray is
    // already aligned with +kz, so the shear coefficients are (0, 0).
    let ax = v0[kx] - a[kx];
    let ay = v0[ky] - a[ky];
    let bx = v1[kx] - a[kx];
    let by = v1[ky] - a[ky];
    let cx = v2[kx] - a[kx];
    let cy = v2[ky] - a[ky];

    // Scaled 2D edge functions — twice the signed area of each sub-triangle
    // formed with the (projected) ray origin.
    let mut u = cx * by - cy * bx;
    let mut v = ax * cy - ay * cx;
    let mut w = bx * ay - by * ax;

    // f64 fallback when any coefficient is exactly zero — avoids
    // catastrophic cancellation rounding a near-zero value to the
    // wrong sign.
    if u == 0.0 || v == 0.0 || w == 0.0 {
        let u64 = (cx as f64) * (by as f64) - (cy as f64) * (bx as f64);
        let v64 = (ax as f64) * (cy as f64) - (ay as f64) * (cx as f64);
        let w64 = (bx as f64) * (ay as f64) - (by as f64) * (ax as f64);
        u = u64 as f32;
        v = v64 as f32;
        w = w64 as f32;

        // Still exactly zero ⇒ ray lies on that edge (in 2D). Apply a
        // canonical orientation rule: a 2D edge direction `dx>0` (or
        // `dx==0 && dy>0`) grants ownership to this triangle. An
        // adjacent triangle sees the shared edge with reversed vertex
        // order, giving the opposite dx/dy and failing the rule — so
        // exactly one side accepts.
        if u == 0.0 && !owns_edge_2d(bx, by, cx, cy) {
            return None;
        }
        if v == 0.0 && !owns_edge_2d(cx, cy, ax, ay) {
            return None;
        }
        if w == 0.0 && !owns_edge_2d(ax, ay, bx, by) {
            return None;
        }
    }

    // Consistent signs required (all ≥0 or all ≤0). Zero coefficients
    // that survived the tie-break are benign here.
    if (u < 0.0 || v < 0.0 || w < 0.0) && (u > 0.0 || v > 0.0 || w > 0.0) {
        return None;
    }

    let det = u + v + w;
    if det == 0.0 {
        return None;
    }

    // Interpolate the kz coordinate of the hit from the unnormalised
    // barycentrics (U, V, W). Dividing by the segment length yields a
    // parameter in [0, 1] when the hit is within the segment.
    let az = v0[kz] - a[kz];
    let bz = v1[kz] - a[kz];
    let cz = v2[kz] - a[kz];
    let t_raw = u * az + v * bz + w * cz;
    let t = t_raw / (det * seg_len);

    if !(0.0..=1.0).contains(&t) {
        return None;
    }

    Some(t)
}

/// Canonical edge-ownership tie-break for the on-edge (U/V/W == 0) case.
/// Two triangles sharing an edge see opposite 2D projected directions,
/// so exactly one satisfies `dx > 0 || (dx == 0 && dy > 0)`.
fn owns_edge_2d(ex: f32, ey: f32, fx: f32, fy: f32) -> bool {
    let dx = fx - ex;
    let dy = fy - ey;
    dx > 0.0 || (dx == 0.0 && dy > 0.0)
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

        // 12 (closed) or 10 (open-top) triangles, CCW from outside —
        // outward-facing normals, matching the STL / OBJ convention.
        let mut faces: Vec<([f32; 3], [f32; 3], [f32; 3])> = vec![
            // -Z face (outward −Z)
            (c000, c010, c110),
            (c000, c110, c100),
            // -Y face (outward −Y)
            (c000, c101, c001),
            (c000, c100, c101),
            // +Y face (outward +Y)
            (c010, c111, c110),
            (c010, c011, c111),
            // -X face (outward −X)
            (c000, c001, c011),
            (c000, c011, c010),
            // +X face (outward +X)
            (c100, c111, c101),
            (c100, c110, c111),
        ];
        if closed_top {
            // +Z face (outward +Z)
            faces.push((c001, c111, c011));
            faces.push((c001, c101, c111));
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

    /// Merges two mesh parts (vertex+index) into a single soup. Useful
    /// for building dirty test fixtures: intersecting solids, nested
    /// boxes, etc.
    fn merge_meshes(mut a: MeshData, b: MeshData) -> MeshData {
        let offset = a.vertices.len() as u32;
        a.vertices.extend(b.vertices);
        a.indices.extend(b.indices.into_iter().map(|i| i + offset));
        a.bbox = BoundingBox {
            min: a.bbox.min.min(b.bbox.min),
            max: a.bbox.max.max(b.bbox.max),
        };
        a.volume += b.volume;
        a
    }

    #[test]
    fn gwn_inside_closed_cube_is_one() {
        // A closed consistently-oriented cube — GWN at the center should
        // be ≈ 1 (inside), at a far corner ≈ 0 (outside).
        let mesh = make_cube_mesh(0.1, 0.9);
        let w_inside = generalized_winding_number(Vec3::new(0.5, 0.5, 0.5), &mesh);
        let w_outside = generalized_winding_number(Vec3::new(2.0, 2.0, 2.0), &mesh);
        assert!(
            (w_inside - 1.0).abs() < 1e-3,
            "GWN inside cube ≈ 1; got {w_inside}"
        );
        assert!(
            w_outside.abs() < 1e-3,
            "GWN outside cube ≈ 0; got {w_outside}"
        );
    }

    #[test]
    fn gwn_overlap_region_is_two() {
        // Two overlapping cubes. At a point inside BOTH, winding ≈ 2.
        // At a point inside only one, winding ≈ 1. Outside both, ≈ 0.
        // The 0.5 threshold correctly classifies all "in at least one"
        // points as inside — the semantic needed for dirty meshes with
        // intersecting components.
        let a = make_cube_mesh(0.0, 0.6);
        let b = make_cube_mesh(0.4, 1.0);
        let mesh = merge_meshes(a, b);
        let w_overlap = generalized_winding_number(Vec3::new(0.5, 0.5, 0.5), &mesh);
        let w_only_a = generalized_winding_number(Vec3::new(0.2, 0.2, 0.2), &mesh);
        let w_only_b = generalized_winding_number(Vec3::new(0.8, 0.8, 0.8), &mesh);
        let w_outside = generalized_winding_number(Vec3::new(-1.0, -1.0, -1.0), &mesh);
        assert!(
            (w_overlap - 2.0).abs() < 1e-2,
            "overlap GWN ≈ 2; got {w_overlap}"
        );
        assert!(
            (w_only_a - 1.0).abs() < 1e-2,
            "only-A GWN ≈ 1; got {w_only_a}"
        );
        assert!(
            (w_only_b - 1.0).abs() < 1e-2,
            "only-B GWN ≈ 1; got {w_only_b}"
        );
        assert!(w_outside.abs() < 1e-2, "outside GWN ≈ 0; got {w_outside}");
    }

    #[test]
    fn overlapping_cubes_union_through_dc() {
        // Scan two overlapping cubes into a ScalarField and run DC. With
        // the old edge-parity pipeline the overlap region would come out
        // as "outside" (XOR) or the double-counted front faces would
        // produce extra geometry (or_insert). GWN makes the overlap
        // region inside, so the DC output is the clean union surface.
        let a = make_cube_mesh(0.15, 0.65);
        let b = make_cube_mesh(0.4, 0.9);
        let mesh = merge_meshes(a, b);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1);
        let unit_size = 1.0 / size_code as f32;
        let min_code = IVec3::ZERO;

        let field =
            ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size, SignMode::Gwn);

        // Center of the overlap region — must be inside.
        assert!(
            field.sign_at(IVec3::new(8, 8, 8)),
            "overlap center must be inside"
        );
        // Corner of cube A only — must be inside.
        assert!(
            field.sign_at(IVec3::new(4, 4, 4)),
            "cube-A-only point must be inside"
        );
        // Corner of cube B only — must be inside.
        assert!(
            field.sign_at(IVec3::new(13, 13, 13)),
            "cube-B-only point must be inside"
        );
        // Outside both — must be outside.
        assert!(
            !field.sign_at(IVec3::new(0, 0, 0)),
            "grid corner must be outside"
        );

        let octree = OctreeNode::build_with_scalar_field(min_code, depth, &field, false, unit_size);
        let Some(mut octree) = octree else {
            panic!("overlapping cubes should produce a non-empty octree");
        };
        OctreeNode::simplify(&mut octree, 0.0);
        let result = OctreeNode::extract_mesh(&mut octree, &field, unit_size);
        assert!(
            result.triangle_count() > 0,
            "DC output must be non-empty for overlapping cubes"
        );
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
            2,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let Some(tv) = t else {
            panic!("interior hit should register");
        };
        assert!((tv - 0.5).abs() < 1e-5);
    }

    #[test]
    fn segment_misses_triangle() {
        let t = segment_triangle_intersection(
            Vec3::new(2.0, 2.0, -1.0),
            Vec3::new(2.0, 2.0, 1.0),
            2,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!(t.is_none());
    }

    #[test]
    fn watertight_shared_diagonal_exactly_one_hit() {
        // Unit quad in the z=0 plane, split by the diagonal
        // (0,0,0)–(1,1,0). Ray along +Z through (0.5, 0.5) hits the
        // shared diagonal. Exactly one of the two triangles must
        // register — otherwise scan-conversion double-counts.
        let a = Vec3::new(0.5, 0.5, -1.0);
        let b = Vec3::new(0.5, 0.5, 1.0);
        let t1 = segment_triangle_intersection(
            a,
            b,
            2,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let t2 = segment_triangle_intersection(
            a,
            b,
            2,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
        );
        assert_ne!(
            t1.is_some(),
            t2.is_some(),
            "exactly one of the two triangles must own the shared diagonal; got t1={t1:?}, t2={t2:?}"
        );
    }

    #[test]
    fn watertight_shared_vertex_exactly_one_hit() {
        // Four triangles fanning around the center vertex (0.5, 0.5, 0).
        // A +Z ray through the center hits the shared vertex; the
        // watertight rule must pick exactly one of the four.
        let a = Vec3::new(0.5, 0.5, -1.0);
        let b = Vec3::new(0.5, 0.5, 1.0);
        let center = Vec3::new(0.5, 0.5, 0.0);
        let tris = [
            (Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), center),
            (Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), center),
            (Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0), center),
            (Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.0, 0.0, 0.0), center),
        ];
        let hit_count = tris
            .iter()
            .filter(|(v0, v1, v2)| segment_triangle_intersection(a, b, 2, *v0, *v1, *v2).is_some())
            .count();
        assert_eq!(
            hit_count, 1,
            "exactly one of the four fan triangles must own the shared vertex"
        );
    }

    #[test]
    fn watertight_ray_across_axis_misses_edge_on_plane() {
        // Ray travels along +X, not +Z, but the triangle lies in the
        // z=0 plane. The ray origin is on the triangle plane at (0.1, 0.5, 0),
        // heading to (0.9, 0.5, 0). The segment stays inside the triangle;
        // since it lies in the plane the watertight test should reject
        // (det==0, ray parallel to triangle).
        let a = Vec3::new(0.1, 0.5, 0.0);
        let b = Vec3::new(0.9, 0.5, 0.0);
        let t = segment_triangle_intersection(
            a,
            b,
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.5, 1.0, 0.0),
        );
        assert!(
            t.is_none(),
            "coplanar ray must not count as a crossing; got {t:?}"
        );
    }

    #[test]
    fn scan_cube_flood_fill_signs() {
        // Cube [0.123, 0.877]³ in a depth-5 grid over [0,1]³.
        let mesh = make_cube_mesh(0.123, 0.877);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1); // 16
        let unit_size = 1.0 / size_code as f32; // 0.0625
        let min_code = IVec3::ZERO;

        let field =
            ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size, SignMode::Gwn);
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

        let field =
            ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size, SignMode::Gwn);

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

        let field =
            ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size, SignMode::Gwn);

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
    fn active_cells_prune_homogeneous_regions() {
        // Small cube (≈12% of grid extent) centred in a depth-6 grid.
        // The vast majority of octree cells are entirely interior or
        // entirely exterior; `active_cells` should hold only the cells
        // the surface actually touched.
        let mesh = make_cube_mesh(0.45, 0.55);
        let depth = 6;
        let size_code = 1_i32 << (depth - 1);
        let unit_size = 1.0 / size_code as f32;
        let min_code = IVec3::ZERO;

        let field =
            ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size, SignMode::Gwn);

        // Total node count of a fully-built octree at this depth.
        let mut total_cells = 0usize;
        for i in 0..depth {
            let n = (size_code >> i) as usize;
            total_cells += n * n * n;
        }
        let active = field.active_cells.len();
        // Mesh surface is 6 thin slabs ≈ 5 × 5 leaf cells each.
        // Active set must be at least an order of magnitude smaller
        // than the full octree — the whole point of the optimisation.
        assert!(
            active * 8 < total_cells,
            "expected pruning ratio > 8×; active={active}, total={total_cells}"
        );
    }

    #[test]
    fn scan_cube_round_trip_through_dc() {
        let mesh = make_cube_mesh(0.123, 0.877);
        let depth = 5;
        let size_code = 1_i32 << (depth - 1);
        let unit_size = 1.0 / size_code as f32;
        let min_code = IVec3::ZERO;

        let field =
            ScannedMeshField::from_mesh(&mesh, min_code, size_code, unit_size, SignMode::Gwn);

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
