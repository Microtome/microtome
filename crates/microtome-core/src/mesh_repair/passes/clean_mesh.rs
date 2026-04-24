//! Pre-construction mesh cleanup: duplicate-face removal, orphan-vertex
//! removal, T-junction resolution, and (with a
//! [`ReprojectionTarget`](super::super::reprojection::ReprojectionTarget))
//! winding correction.
//!
//! T-junction resolution uses the closest-point [`TriangleBvh`](super::super::spatial::TriangleBvh)
//! to filter candidate vertices per triangle in O(V log F) average, then
//! fan-triangulates each affected triangle so every T-vertex becomes a
//! proper participating vertex.

use std::collections::{HashMap, HashSet};

use glam::Vec3;

use super::super::error::PassError;
use super::super::pass::{MeshRepairPass, PassOutcome, PassStage, PassWarningKind};
use super::super::spatial::{Aabb, TriangleBvh};
use crate::isosurface::IsoMesh;
use crate::mesh_repair::RepairContext;

/// Cheap mesh cleanup operations that don't need half-edge topology.
#[derive(Debug, Clone)]
pub struct CleanMesh {
    /// Drop duplicate triangles (same triangle as a sorted index tuple).
    pub remove_duplicate_faces: bool,
    /// Drop vertices not referenced by any surviving triangle, remap indices.
    pub remove_orphan_vertices: bool,
    /// Drop the surplus faces from any canonical edge shared by ≥3 faces.
    /// Keeps the first two encountered. The remaining edge becomes
    /// 2-manifold; downstream half-edge construction can proceed.
    /// Heuristic — picks faces by encounter order, not by area or normal
    /// consistency — but adequate for DC output where the third face is
    /// typically an M-T tie-break artifact.
    pub drop_non_manifold_faces: bool,
    /// Topologically propagate winding consistency: BFS over face adjacency
    /// (via shared canonical edges), flipping any face that traverses a
    /// shared edge in the same direction as its neighbour. Requires no
    /// target. Runs before [`fix_winding`](Self::fix_winding) so that the
    /// outward-normal check operates on a winding-consistent input.
    pub propagate_winding: bool,
    /// Flip face winding when `face_normal · target.normal(centroid) < 0`.
    /// Requires `ctx.target = Some(...)`; emits a Skipped warning otherwise.
    pub fix_winding: bool,
    /// Detect T-junctions (vertex strictly interior to another triangle's
    /// edge) and fan-triangulate the offending triangle so the T-vertex
    /// becomes a proper vertex.
    pub resolve_t_junctions: bool,
    /// Perpendicular-distance tolerance (world units) for T-junction
    /// detection. A vertex within this distance of an edge — and strictly
    /// between the endpoints — is treated as on the edge. Default `1e-4`.
    pub t_junction_tolerance: f32,
}

impl Default for CleanMesh {
    fn default() -> Self {
        Self {
            remove_duplicate_faces: true,
            remove_orphan_vertices: true,
            drop_non_manifold_faces: true,
            propagate_winding: true,
            fix_winding: true,
            resolve_t_junctions: true,
            t_junction_tolerance: 1e-4,
        }
    }
}

impl MeshRepairPass for CleanMesh {
    fn name(&self) -> &'static str {
        "clean_mesh"
    }

    fn stage(&self) -> PassStage {
        PassStage::PreConstruction
    }

    fn pre_construction(
        &self,
        mut iso: IsoMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<(IsoMesh, PassOutcome), PassError> {
        let mut outcome = PassOutcome::noop(self.name());

        if self.remove_duplicate_faces {
            let pre = iso.indices.len() / 3;
            iso = drop_duplicate_faces(iso);
            let post = iso.indices.len() / 3;
            outcome.stats.faces_removed += (pre - post) as u32;
        }

        if self.propagate_winding {
            let flips = propagate_winding(&mut iso);
            if flips > 0 {
                outcome.warn(
                    PassWarningKind::Clamped,
                    format!(
                        "propagate_winding flipped {flips} triangles for topological consistency"
                    ),
                );
            }
        }

        if self.fix_winding {
            if let Some(target) = ctx.target {
                let flips = fix_winding(&mut iso, target);
                if flips > 0 {
                    outcome.warn(
                        PassWarningKind::Clamped,
                        format!("flipped winding on {flips} triangles"),
                    );
                }
            } else {
                outcome.warn(
                    PassWarningKind::Skipped,
                    "fix_winding requires a ReprojectionTarget; skipped",
                );
            }
        }

        if self.resolve_t_junctions {
            let (next, splits) = split_t_junctions(iso, self.t_junction_tolerance);
            iso = next;
            if splits > 0 {
                outcome.warn(
                    PassWarningKind::Clamped,
                    format!("split {splits} T-junction triangles via fan triangulation"),
                );
            }
        }

        // Run last so any non-manifold edges created by t-junction
        // splitting (or already present on input) are surfaced before
        // half-edge construction.
        if self.drop_non_manifold_faces {
            let (next, dropped) = drop_non_manifold_faces(iso);
            iso = next;
            if dropped > 0 {
                outcome.stats.faces_removed += dropped;
                outcome.warn(
                    PassWarningKind::Clamped,
                    format!(
                        "dropped {dropped} surplus faces on canonical edges shared by ≥3 faces"
                    ),
                );
            }
        }

        if self.remove_orphan_vertices {
            let pre = iso.positions.len();
            iso = drop_orphan_vertices(iso);
            let post = iso.positions.len();
            // Reuse vertices_merged as the orphan-removal counter; semantically
            // close enough ("vertices that disappeared from the output").
            outcome.stats.vertices_merged += (pre - post) as u32;
        }

        Ok((iso, outcome))
    }
}

fn drop_duplicate_faces(mut iso: IsoMesh) -> IsoMesh {
    let mut seen: HashSet<(u32, u32, u32)> = HashSet::with_capacity(iso.indices.len() / 3);
    let mut kept: Vec<u32> = Vec::with_capacity(iso.indices.len());
    for tri in iso.indices.chunks_exact(3) {
        let mut sorted = [tri[0], tri[1], tri[2]];
        sorted.sort_unstable();
        let key = (sorted[0], sorted[1], sorted[2]);
        if seen.insert(key) {
            kept.extend_from_slice(tri);
        }
    }
    iso.indices = kept;
    iso
}

fn drop_orphan_vertices(iso: IsoMesh) -> IsoMesh {
    let mut referenced: Vec<bool> = vec![false; iso.positions.len()];
    for &i in &iso.indices {
        referenced[i as usize] = true;
    }
    if referenced.iter().all(|&r| r) {
        return iso;
    }
    let mut remap: Vec<u32> = vec![u32::MAX; iso.positions.len()];
    let mut new_positions: Vec<Vec3> = Vec::new();
    let mut new_normals: Vec<Vec3> = Vec::new();
    for (old, &keep) in referenced.iter().enumerate() {
        if keep {
            remap[old] = new_positions.len() as u32;
            new_positions.push(iso.positions[old]);
            if let Some(&n) = iso.normals.get(old) {
                new_normals.push(n);
            }
        }
    }
    let new_indices: Vec<u32> = iso.indices.iter().map(|&i| remap[i as usize]).collect();
    IsoMesh {
        positions: new_positions,
        normals: new_normals,
        indices: new_indices,
    }
}

/// Detects T-junctions (vertices strictly interior to another triangle's
/// edge, within `tolerance`) and fan-triangulates each affected triangle so
/// the T-vertex becomes a proper participating vertex. Returns the updated
/// mesh and the count of triangles that were split.
fn split_t_junctions(iso: IsoMesh, tolerance: f32) -> (IsoMesh, u32) {
    if iso.indices.is_empty() || tolerance <= 0.0 {
        return (iso, 0);
    }
    let triangles: Vec<[Vec3; 3]> = iso
        .indices
        .chunks_exact(3)
        .map(|tri| {
            [
                iso.positions[tri[0] as usize],
                iso.positions[tri[1] as usize],
                iso.positions[tri[2] as usize],
            ]
        })
        .collect();
    let Some(bvh) = TriangleBvh::build(&triangles) else {
        return (iso, 0);
    };
    let triangle_count = triangles.len();

    // Per-triangle, per-edge: list of (parameter t, vertex index) pairs that
    // sit on that edge. Edges are indexed 0=(i0→i1), 1=(i1→i2), 2=(i2→i0).
    let mut per_tri_per_edge: Vec<[Vec<(f32, u32)>; 3]> = (0..triangle_count)
        .map(|_| [Vec::new(), Vec::new(), Vec::new()])
        .collect();

    for v in 0..iso.positions.len() as u32 {
        let pv = iso.positions[v as usize];
        let query = Aabb {
            min: pv - Vec3::splat(tolerance),
            max: pv + Vec3::splat(tolerance),
        };
        bvh.visit_overlapping(query, |tri_idx| {
            let i0 = iso.indices[tri_idx * 3];
            let i1 = iso.indices[tri_idx * 3 + 1];
            let i2 = iso.indices[tri_idx * 3 + 2];
            if v == i0 || v == i1 || v == i2 {
                return;
            }
            let edges = [(0, i0, i1), (1, i1, i2), (2, i2, i0)];
            for (eidx, e_start, e_end) in edges {
                if let Some(t) = edge_parameter_if_on_segment(
                    pv,
                    iso.positions[e_start as usize],
                    iso.positions[e_end as usize],
                    tolerance,
                ) {
                    per_tri_per_edge[tri_idx][eidx].push((t, v));
                }
            }
        });
    }

    let mut new_indices: Vec<u32> = Vec::with_capacity(iso.indices.len());
    let mut split_count = 0u32;
    for (tri_idx, edges) in per_tri_per_edge.iter().enumerate().take(triangle_count) {
        let i0 = iso.indices[tri_idx * 3];
        let i1 = iso.indices[tri_idx * 3 + 1];
        let i2 = iso.indices[tri_idx * 3 + 2];
        let any_splits = edges.iter().any(|e| !e.is_empty());
        if !any_splits {
            new_indices.extend_from_slice(&[i0, i1, i2]);
            continue;
        }
        let mut polygon: Vec<u32> =
            Vec::with_capacity(3 + edges.iter().map(|e| e.len()).sum::<usize>());
        polygon.push(i0);
        push_sorted_edge_splits(&mut polygon, &edges[0]);
        polygon.push(i1);
        push_sorted_edge_splits(&mut polygon, &edges[1]);
        polygon.push(i2);
        push_sorted_edge_splits(&mut polygon, &edges[2]);
        // Fan triangulation from polygon[0]; preserves CCW winding because
        // polygon walks the original triangle's perimeter.
        for i in 1..polygon.len() - 1 {
            new_indices.extend_from_slice(&[polygon[0], polygon[i], polygon[i + 1]]);
        }
        split_count += 1;
    }

    (
        IsoMesh {
            positions: iso.positions,
            normals: iso.normals,
            indices: new_indices,
        },
        split_count,
    )
}

fn push_sorted_edge_splits(polygon: &mut Vec<u32>, splits: &[(f32, u32)]) {
    if splits.is_empty() {
        return;
    }
    let mut sorted = splits.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, v) in sorted {
        polygon.push(v);
    }
}

/// If `pv` lies within `tolerance` of the segment `(p_start, p_end)` and
/// strictly between the endpoints (parameter t in (0, 1)), returns t.
fn edge_parameter_if_on_segment(
    pv: Vec3,
    p_start: Vec3,
    p_end: Vec3,
    tolerance: f32,
) -> Option<f32> {
    let ab = p_end - p_start;
    let len_sq = ab.length_squared();
    if len_sq < 1e-20 {
        return None;
    }
    let t = (pv - p_start).dot(ab) / len_sq;
    if t <= 0.0 || t >= 1.0 {
        return None;
    }
    let foot = p_start + ab * t;
    if (pv - foot).length() < tolerance {
        Some(t)
    } else {
        None
    }
}

/// Drops the surplus faces from any canonical edge shared by ≥3 faces.
/// Keeps the first two faces encountered; subsequent faces on the same
/// non-manifold edge are dropped. Returns the updated mesh and the count
/// of faces removed.
fn drop_non_manifold_faces(iso: IsoMesh) -> (IsoMesh, u32) {
    let face_count = iso.indices.len() / 3;
    if face_count == 0 {
        return (iso, 0);
    }
    let mut edge_keepers: HashMap<(u32, u32), u32> = HashMap::new();
    let mut to_drop: HashSet<usize> = HashSet::new();
    for f in 0..face_count {
        if to_drop.contains(&f) {
            continue;
        }
        let i0 = iso.indices[f * 3];
        let i1 = iso.indices[f * 3 + 1];
        let i2 = iso.indices[f * 3 + 2];
        let mut would_break = false;
        for (a, b) in [(i0, i1), (i1, i2), (i2, i0)] {
            let key = if a < b { (a, b) } else { (b, a) };
            let count = *edge_keepers.get(&key).unwrap_or(&0);
            if count >= 2 {
                would_break = true;
                break;
            }
        }
        if would_break {
            to_drop.insert(f);
            continue;
        }
        for (a, b) in [(i0, i1), (i1, i2), (i2, i0)] {
            let key = if a < b { (a, b) } else { (b, a) };
            *edge_keepers.entry(key).or_insert(0) += 1;
        }
    }
    if to_drop.is_empty() {
        return (iso, 0);
    }
    let mut new_indices = Vec::with_capacity(iso.indices.len());
    for f in 0..face_count {
        if !to_drop.contains(&f) {
            new_indices.extend_from_slice(&iso.indices[f * 3..f * 3 + 3]);
        }
    }
    let removed = to_drop.len() as u32;
    (
        IsoMesh {
            positions: iso.positions,
            normals: iso.normals,
            indices: new_indices,
        },
        removed,
    )
}

/// Propagates winding consistency across the mesh by BFS over face
/// adjacency. Two faces sharing a canonical edge {u, v} should traverse it
/// in opposite directions; if they don't, one of them is flipped. Returns
/// the number of triangles whose vertex order was reversed.
///
/// Behaviour on multi-component meshes: each connected component is BFS-ed
/// independently, anchored on its lowest-indexed face (which keeps its
/// original winding). Non-orientable surfaces would loop indefinitely in
/// theory; we cap at one BFS pass per face to guarantee termination on
/// pathological inputs.
fn propagate_winding(iso: &mut IsoMesh) -> u32 {
    let face_count = iso.indices.len() / 3;
    if face_count <= 1 {
        return 0;
    }

    // Build canonical-edge → list of (face_idx, directed_a, directed_b)
    // entries. directed_a/directed_b are the actual indices in iso.indices
    // for the directed edge (so we can detect direction).
    type EdgeRef = (usize, u32, u32);
    let mut edge_to_faces: HashMap<(u32, u32), Vec<EdgeRef>> =
        HashMap::with_capacity(face_count * 2);
    for f in 0..face_count {
        let i0 = iso.indices[f * 3];
        let i1 = iso.indices[f * 3 + 1];
        let i2 = iso.indices[f * 3 + 2];
        for (a, b) in [(i0, i1), (i1, i2), (i2, i0)] {
            let key = if a < b { (a, b) } else { (b, a) };
            edge_to_faces.entry(key).or_default().push((f, a, b));
        }
    }

    let mut visited = vec![false; face_count];
    let mut flips = 0u32;
    let mut queue: std::collections::VecDeque<usize> = std::collections::VecDeque::new();

    for seed in 0..face_count {
        if visited[seed] {
            continue;
        }
        visited[seed] = true;
        queue.push_back(seed);
        while let Some(f) = queue.pop_front() {
            let (i0, i1, i2) = (
                iso.indices[f * 3],
                iso.indices[f * 3 + 1],
                iso.indices[f * 3 + 2],
            );
            for (a, b) in [(i0, i1), (i1, i2), (i2, i0)] {
                let key = if a < b { (a, b) } else { (b, a) };
                let Some(neighbours) = edge_to_faces.get(&key) else {
                    continue;
                };
                for &(g, ga, gb) in neighbours {
                    if g == f || visited[g] {
                        continue;
                    }
                    visited[g] = true;
                    // f traverses edge as a→b. If g traverses it as ga→gb
                    // and (ga, gb) == (a, b), they go same direction — flip g.
                    if (ga, gb) == (a, b) {
                        // Flip g by swapping its second and third indices.
                        // Walk from i0 perspective: (i0,i1,i2) → (i0,i2,i1).
                        iso.indices.swap(g * 3 + 1, g * 3 + 2);
                        flips += 1;

                        // The flip changed g's directed edges in
                        // edge_to_faces. We don't need to update the map
                        // because BFS only consults f's edges (the seed
                        // for this neighbour), and g is now winding-
                        // consistent with f. Subsequent BFS hops from g
                        // will read iso.indices fresh.
                    }
                    queue.push_back(g);
                }
            }
        }
    }

    flips
}

fn fix_winding(
    iso: &mut IsoMesh,
    target: &dyn super::super::reprojection::ReprojectionTarget,
) -> u32 {
    let mut flips: u32 = 0;
    let positions = iso.positions.clone();
    for tri in iso.indices.chunks_exact_mut(3) {
        let p0 = positions[tri[0] as usize];
        let p1 = positions[tri[1] as usize];
        let p2 = positions[tri[2] as usize];
        let face_normal = (p1 - p0).cross(p2 - p0);
        if face_normal == Vec3::ZERO {
            continue;
        }
        let centroid = (p0 + p1 + p2) / 3.0;
        let target_normal = target.normal(centroid);
        if face_normal.dot(target_normal) < 0.0 {
            tri.swap(1, 2);
            flips += 1;
        }
    }
    flips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::Sphere;
    use crate::mesh_repair::reprojection::ScalarFieldTarget;

    fn iso(positions: Vec<Vec3>, indices: Vec<u32>) -> IsoMesh {
        let n = positions.len();
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; n],
            indices,
        }
    }

    #[test]
    fn clean_drops_duplicate_face() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2, 1, 2, 0],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert_eq!(out.indices.len(), 3);
        assert_eq!(outcome.stats.faces_removed, 1);
    }

    #[test]
    fn clean_removes_orphan_vertex() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(99.0, 99.0, 99.0), // orphan
            ],
            vec![0, 1, 2],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert_eq!(out.positions.len(), 3);
        assert_eq!(outcome.stats.vertices_merged, 1);
    }

    #[test]
    fn clean_fix_winding_flips_inverted_triangle() {
        // Sphere centered at origin; inward-facing triangle at +x outside.
        // Set up: triangle near (1,0,0) wound clockwise as viewed from +x
        // (i.e. its normal points -x). The target's outward normal is +x;
        // we expect a flip.
        let input = iso(
            vec![
                Vec3::new(2.0, -1.0, -1.0),
                Vec3::new(2.0, 1.0, -1.0),
                Vec3::new(2.0, 0.0, 1.0),
            ],
            // Order chosen so cross((p1-p0), (p2-p0)) points -x.
            vec![0, 2, 1],
        );
        let sphere = Sphere::with_center(1.0, Vec3::ZERO);
        let target = ScalarFieldTarget::new(&sphere);
        let pass = CleanMesh::default();
        let nf = |_p: Vec3| Vec3::ZERO;
        let ctx = RepairContext::new(&nf).with_target(&target);
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        // After flip, indices should be (0, 1, 2) — winding reversed.
        assert_eq!(&out.indices[..3], &[0, 1, 2]);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| matches!(w.kind, PassWarningKind::Clamped))
        );
    }

    #[test]
    fn clean_fix_winding_skipped_without_target() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (_out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| matches!(w.kind, PassWarningKind::Skipped))
        );
    }

    #[test]
    fn clean_splits_simple_t_junction() {
        // Triangle (0,1,2) with vertex 3 at midpoint of edge (1,2). The
        // T-junction handler should fan the offending triangle into two
        // triangles, both containing vertex 3.
        //
        // Layout (z=0):
        //   2 (0,2)
        //   |\
        //   | \
        //   3  \  <- midpoint of edge (1,2)
        //   |   \
        //   |    \
        //   1 (2,0)
        //   |
        //   0 (0,0)
        //
        // Edge being split: (1, 2). Result: (0,1,3) + (0,3,2).
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0), // 0
                Vec3::new(2.0, 0.0, 0.0), // 1
                Vec3::new(0.0, 2.0, 0.0), // 2
                Vec3::new(1.0, 1.0, 0.0), // 3 — midpoint of (1,2)
            ],
            // Two disjoint triangles, the second exists only to keep vertex 3
            // referenced and avoid the orphan-removal pass dropping it.
            // Orient the first so vertex 3 falls on edge (1,2).
            vec![0, 1, 2, /* keep-alive: */ 3, 1, 2],
        );
        let pass = CleanMesh {
            // Don't dedup — the second triangle has the same sorted index
            // tuple as the keep-alive use of vertex 3.
            remove_duplicate_faces: false,
            // Skip winding correction (no target).
            fix_winding: false,
            ..CleanMesh::default()
        };
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        // Expect a fan triangulation: original (0,1,2) replaced by 2
        // triangles, plus the keep-alive triangle still split through 3 -
        // so 4 total.
        let split_warning = outcome
            .warnings
            .iter()
            .any(|w| matches!(w.kind, PassWarningKind::Clamped));
        assert!(split_warning, "should warn about the split");
        assert!(
            out.indices.len() / 3 >= 3,
            "should have at least 3 triangles after split; got {}",
            out.indices.len() / 3
        );
        // Vertex 3 must be referenced by the new index buffer.
        assert!(out.indices.contains(&3));
    }

    #[test]
    fn clean_no_t_junction_preserves_mesh() {
        // Two adjacent triangles, no T-junction. Should be untouched by the
        // T-junction pass.
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            ],
            vec![0, 1, 2, 1, 3, 2],
        );
        let pass = CleanMesh {
            fix_winding: false,
            ..CleanMesh::default()
        };
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert_eq!(out.indices.len() / 3, 2, "no splits should occur");
        assert!(
            !outcome
                .warnings
                .iter()
                .any(|w| w.message.contains("T-junction"))
        );
    }

    #[test]
    fn clean_no_op_on_already_clean_mesh() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input.clone(), &ctx).expect("clean");
        assert_eq!(out.positions.len(), input.positions.len());
        assert_eq!(out.indices.len(), input.indices.len());
        assert_eq!(outcome.stats.faces_removed, 0);
        assert_eq!(outcome.stats.vertices_merged, 0);
    }
}
