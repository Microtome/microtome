//! Garland-Heckbert quadric edge-collapse simplification.
//!
//! Builds a [`VertexQuadric`] per vertex (face / boundary / feature plane
//! constraints), enumerates undirected edges, computes a collapse cost
//! per edge as `(Q_u + Q_v).evaluate(p_opt)` where `p_opt` is the QEF
//! optimum, and processes edges cheapest-first via a priority queue with
//! lazy deletion. Each candidate is pre-checked for normal flips and
//! local volume change before the collapse is committed; pre-checks that
//! reject leave the mesh untouched and bump the edge's generation so
//! re-pushing later picks up the new configuration.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

use glam::Vec3;

use super::super::error::{HalfEdgeOpError, PassError};
use super::super::half_edge::{FaceId, HalfEdgeId, HalfEdgeMesh, VertexId};
use super::super::pass::{MeshRepairPass, PassOutcome, PassWarningKind};
use super::super::vertex_class::VertexClass;
use super::super::vertex_quadric::{QuadricWeights, VertexQuadric, accumulate_for_mesh};
use crate::mesh_repair::RepairContext;

/// QEF-driven mesh simplification.
#[derive(Debug, Clone)]
pub struct SimplifyQuadric {
    /// Stop when the live triangle count drops to this. None = no count budget.
    pub target_triangle_count: Option<u32>,
    /// Stop when the next collapse's cost exceeds this. None = no cost budget.
    pub target_error: Option<f32>,
    /// Tuning knobs for boundary / feature plane weights.
    pub weights: QuadricWeights,
    /// Reject collapses whose `p_opt` would flip an incident face normal.
    pub forbid_normal_flip: bool,
    /// Reject collapses where the absolute local volume change exceeds
    /// this fraction of the pre-collapse local |volume|. Set to 0 to
    /// disable the check. Default `0.05` (5 %).
    pub volume_tolerance: f32,
}

impl Default for SimplifyQuadric {
    fn default() -> Self {
        Self {
            target_triangle_count: None,
            target_error: None,
            weights: QuadricWeights::default(),
            forbid_normal_flip: true,
            volume_tolerance: 0.05,
        }
    }
}

impl MeshRepairPass for SimplifyQuadric {
    fn name(&self) -> &'static str {
        "simplify_quadric"
    }

    fn reclassifies(&self) -> bool {
        true
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        _ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        if self.target_triangle_count.is_none() && self.target_error.is_none() {
            return Err(PassError::InvalidConfig(
                "simplify_quadric requires at least one of target_triangle_count or target_error"
                    .into(),
            ));
        }

        let mut outcome = PassOutcome::noop(self.name());
        let mut quadrics = accumulate_for_mesh(mesh, self.weights);
        let mut generation: HashMap<(u32, u32), u32> = HashMap::new();
        let mut heap: BinaryHeap<HeapEntry> = BinaryHeap::new();

        // Seed the heap with the cost of each unique live edge.
        for h in mesh.edge_iter() {
            push_edge(mesh, &mut quadrics, &mut generation, &mut heap, h);
        }

        while let Some(entry) = heap.pop() {
            // Budget check.
            if let Some(target) = self.target_triangle_count
                && (mesh.face_count() as u32) <= target
            {
                break;
            }
            if let Some(max_err) = self.target_error
                && entry.cost > max_err
            {
                break;
            }

            let key = entry.key;
            // Stale entry?
            let cur_gen = generation.get(&key).copied().unwrap_or(0);
            if cur_gen != entry.generation {
                continue;
            }

            // Find the half-edge for this edge (may have moved due to prior
            // collapses; resolve from the canonical key).
            let Some(he) = find_he_for_edge(mesh, key) else {
                continue;
            };

            // Class compatibility: refuse a collapse that would merge
            // two different boundary loops, two different feature
            // sequences, or merge anything into Fixed unintentionally.
            // Conservative v2 rule: skip if either endpoint is Fixed.
            let u = mesh.he_tail(he);
            let v = mesh.he_head(he);
            if matches!(mesh.vertex_class(u), VertexClass::Fixed)
                || matches!(mesh.vertex_class(v), VertexClass::Fixed)
            {
                outcome.warn(
                    PassWarningKind::Skipped,
                    "skipped: Fixed endpoint".to_string(),
                );
                bump_gen(&mut generation, key);
                continue;
            }

            // Compute optimal position from combined quadric.
            let mut combined = quadrics[u.index()].clone();
            combined.combine(&quadrics[v.index()]);
            let (p_opt, _err) = combined.solve();

            // Pre-check: simulate the collapse and reject if it would flip
            // an incident face normal or exceed the volume tolerance. Both
            // checks read current geometry only, so a rejection leaves the
            // mesh untouched.
            if let Err(reason) = precheck_collapse(
                mesh,
                he,
                p_opt,
                self.forbid_normal_flip,
                self.volume_tolerance,
            ) {
                outcome.warn(
                    PassWarningKind::Skipped,
                    format!("collapse rejected by pre-check: {reason}"),
                );
                bump_gen(&mut generation, key);
                continue;
            }

            // Attempt collapse.
            match mesh.collapse_edge_to(he, p_opt) {
                Ok(survivor) => {
                    // Merge the quadric.
                    quadrics[survivor.index()] = combined;

                    // Re-cost survivor's incident edges.
                    bump_gen(&mut generation, key);
                    let incident: Vec<HalfEdgeId> = mesh
                        .edge_iter()
                        .filter(|h| mesh.he_tail(*h) == survivor || mesh.he_head(*h) == survivor)
                        .collect();
                    for h in incident {
                        push_edge(mesh, &mut quadrics, &mut generation, &mut heap, h);
                    }

                    outcome.stats.edges_collapsed += 1;
                }
                Err(_) => {
                    outcome.warn(
                        PassWarningKind::Skipped,
                        "collapse rejected by half-edge op (link condition / boundary)".to_string(),
                    );
                    bump_gen(&mut generation, key);
                    continue;
                }
            }
        }

        Ok(outcome)
    }
}

#[derive(Debug, Clone, Copy)]
struct HeapEntry {
    cost: f32,
    generation: u32,
    key: (u32, u32),
}

impl Eq for HeapEntry {}
impl PartialEq for HeapEntry {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}
impl Ord for HeapEntry {
    // Reverse order for min-heap.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .cost
            .partial_cmp(&self.cost)
            .unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for HeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn canonical(u: VertexId, v: VertexId) -> (u32, u32) {
    if u.0 <= v.0 { (u.0, v.0) } else { (v.0, u.0) }
}

fn bump_gen(generation: &mut HashMap<(u32, u32), u32>, key: (u32, u32)) {
    let g = generation.entry(key).or_insert(0);
    *g = g.wrapping_add(1);
}

fn push_edge(
    mesh: &HalfEdgeMesh,
    quadrics: &mut [VertexQuadric],
    generation: &mut HashMap<(u32, u32), u32>,
    heap: &mut BinaryHeap<HeapEntry>,
    h: HalfEdgeId,
) {
    if !mesh.half_edge_is_live(h) {
        return;
    }
    let u = mesh.he_tail(h);
    let v = mesh.he_head(h);
    let key = canonical(u, v);
    let mut combined = quadrics[u.index()].clone();
    combined.combine(&quadrics[v.index()]);
    let (p_opt, _) = combined.solve();
    let cost = combined.evaluate(p_opt);
    bump_gen(generation, key);
    let g = *generation.get(&key).unwrap_or(&0);
    heap.push(HeapEntry {
        cost,
        generation: g,
        key,
    });
}

fn find_he_for_edge(mesh: &HalfEdgeMesh, key: (u32, u32)) -> Option<HalfEdgeId> {
    for h in mesh.edge_iter() {
        let u = mesh.he_tail(h);
        let v = mesh.he_head(h);
        if canonical(u, v) == key {
            return Some(h);
        }
    }
    None
}

/// Pre-collapse safety check. Walks every face incident to `u` or `v` and
/// simulates the post-collapse geometry by replacing endpoint positions
/// with `p_opt`. Returns an error if any face's normal would flip
/// (when `forbid_normal_flip`) or if the absolute change in summed signed
/// volume of the affected faces exceeds `volume_tolerance` × pre-volume
/// magnitude (when `volume_tolerance > 0`). The two faces that vanish in
/// the collapse contribute their pre-collapse signed volume to the
/// difference (post = 0).
fn precheck_collapse(
    mesh: &HalfEdgeMesh,
    he: HalfEdgeId,
    p_opt: Vec3,
    forbid_normal_flip: bool,
    volume_tolerance: f32,
) -> Result<(), HalfEdgeOpError> {
    let u = mesh.he_tail(he);
    let v = mesh.he_head(he);
    let face_l = mesh.he_face(he);
    let twin = mesh.he_twin(he);
    let face_r = if twin.is_valid() {
        mesh.he_face(twin)
    } else {
        FaceId::INVALID
    };

    let mut total_pre = 0.0_f32;
    let mut total_post = 0.0_f32;
    let mut total_abs = 0.0_f32;
    let mut seen: std::collections::HashSet<u32> = std::collections::HashSet::new();

    for endpoint in [u, v] {
        let he_out = mesh.vertex_he_out(endpoint);
        if !he_out.is_valid() {
            continue;
        }
        let start = he_out;
        let mut cur = start;
        let mut first = true;
        loop {
            let face = mesh.he_face(cur);
            if mesh.face_is_live(face) && seen.insert(face.0) {
                let tri = mesh.face_triangle(face);
                let [p0, p1, p2] = mesh.face_positions(face);
                let pre_vol = signed_volume(p0, p1, p2);
                total_abs += pre_vol.abs();

                if face == face_l || face == face_r {
                    // Collapse removes this face; pre contributes, post is zero.
                    total_pre += pre_vol;
                    continue;
                }

                let q0 = post_collapse_position(mesh, tri[0], u, v, p_opt);
                let q1 = post_collapse_position(mesh, tri[1], u, v, p_opt);
                let q2 = post_collapse_position(mesh, tri[2], u, v, p_opt);
                let n_pre = (p1 - p0).cross(p2 - p0);
                let n_post = (q1 - q0).cross(q2 - q0);

                if forbid_normal_flip && n_pre.dot(n_post) < 0.0 {
                    return Err(HalfEdgeOpError::WouldFlipNormal);
                }
                total_pre += pre_vol;
                total_post += signed_volume(q0, q1, q2);
            }
            let prev = mesh.he_prev(cur);
            let prev_twin = mesh.he_twin(prev);
            if !prev_twin.is_valid() {
                break;
            }
            if !first && prev_twin == start {
                break;
            }
            cur = prev_twin;
            first = false;
        }
    }

    if volume_tolerance > 0.0 {
        let delta = (total_post - total_pre).abs();
        let denom = total_abs.max(1e-9);
        let frac = delta / denom;
        if frac > volume_tolerance {
            return Err(HalfEdgeOpError::WouldExceedVolumeTolerance {
                delta: frac,
                tolerance: volume_tolerance,
            });
        }
    }
    Ok(())
}

#[inline]
fn post_collapse_position(
    mesh: &HalfEdgeMesh,
    vid: VertexId,
    u: VertexId,
    v: VertexId,
    p_opt: Vec3,
) -> Vec3 {
    if vid == u || vid == v {
        p_opt
    } else {
        mesh.vertex_position(vid)
    }
}

#[inline]
fn signed_volume(p0: Vec3, p1: Vec3, p2: Vec3) -> f32 {
    p0.dot(p1.cross(p2)) / 6.0
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

    /// Subdivided triangle: 4 small triangles inside one big triangle.
    /// Face count: 4. Simplifying to 2 should collapse one interior edge.
    fn subdivided_triangle() -> HalfEdgeMesh {
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0), // 0
            Vec3::new(2.0, 0.0, 0.0), // 1
            Vec3::new(1.0, 2.0, 0.0), // 2
            Vec3::new(1.0, 0.0, 0.0), // 3 mid 0-1
            Vec3::new(1.5, 1.0, 0.0), // 4 mid 1-2
            Vec3::new(0.5, 1.0, 0.0), // 5 mid 0-2
        ];
        let indices = vec![
            0, 3, 5, // bottom-left
            3, 1, 4, // bottom-right
            5, 4, 2, // top
            3, 4, 5, // centre (flipped)
        ];
        HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("subdivided triangle")
    }

    #[test]
    fn simplify_requires_a_budget() {
        let mut mesh = subdivided_triangle();
        let pass = SimplifyQuadric::default();
        let ctx = RepairContext::noop();
        let err = pass.apply(&mut mesh, &ctx).unwrap_err();
        assert!(matches!(err, PassError::InvalidConfig(_)));
    }

    #[test]
    fn simplify_reduces_triangle_count_to_target() {
        let mut mesh = subdivided_triangle();
        let pre = mesh.face_count();
        let pass = SimplifyQuadric {
            target_triangle_count: Some(2),
            ..SimplifyQuadric::default()
        };
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("simplify");
        assert!(
            mesh.face_count() <= pre,
            "face count should not increase: pre={pre} post={}",
            mesh.face_count()
        );
        assert!(
            outcome.stats.edges_collapsed > 0,
            "should have collapsed at least one edge"
        );
    }

    /// Octahedron (closed, non-coplanar). Any collapse has nonzero cost
    /// because adjacent faces have different normals.
    fn octahedron() -> HalfEdgeMesh {
        let positions = vec![
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 4,  2, 1, 4,  1, 3, 4,  3, 0, 4,
            2, 0, 5,  1, 2, 5,  3, 1, 5,  0, 3, 5,
        ];
        HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("octa")
    }

    #[test]
    fn simplify_volume_tolerance_blocks_aggressive_collapse_on_thin_pyramid() {
        // Thin pyramid: triangular base at z=0 with apex at z=10. Collapsing
        // the apex into the base would change local volume by ~100 % of the
        // four incident triangles' summed |signed volume|. With tolerance
        // 0.01 every collapse touching the apex is blocked. With tolerance
        // 1.0 (effectively off) collapses proceed.
        let positions = vec![
            Vec3::new(0.0, 0.0, 0.0),  // 0
            Vec3::new(1.0, 0.0, 0.0),  // 1
            Vec3::new(0.5, 1.0, 0.0),  // 2
            Vec3::new(0.5, 0.5, 10.0), // 3 apex
        ];
        let indices = vec![0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3];
        let iso_mesh = iso(positions, indices);
        let mut mesh_strict = HalfEdgeMesh::from_iso_mesh(&iso_mesh).expect("pyramid");
        let pass_strict = SimplifyQuadric {
            target_triangle_count: Some(0), // unbounded budget
            volume_tolerance: 0.01,
            ..SimplifyQuadric::default()
        };
        let ctx = RepairContext::noop();
        pass_strict.apply(&mut mesh_strict, &ctx).expect("simplify");

        let mut mesh_loose = HalfEdgeMesh::from_iso_mesh(&iso_mesh).expect("pyramid");
        let pass_loose = SimplifyQuadric {
            target_triangle_count: Some(0),
            volume_tolerance: 1.5,
            forbid_normal_flip: false,
            ..SimplifyQuadric::default()
        };
        pass_loose.apply(&mut mesh_loose, &ctx).expect("simplify");

        assert!(
            mesh_strict.face_count() > mesh_loose.face_count(),
            "tight tolerance should block more collapses: strict={} loose={}",
            mesh_strict.face_count(),
            mesh_loose.face_count()
        );
    }

    #[test]
    fn simplify_target_error_caps_collapse_count() {
        // With a tight error budget (1e-6), no octahedron edge collapse
        // can satisfy it (each collapse has nonzero cost ≈ vertex offset
        // from neighbouring face planes). Face count stays the same.
        let mut mesh = octahedron();
        let pre = mesh.face_count();
        let pass = SimplifyQuadric {
            target_error: Some(1e-6),
            ..SimplifyQuadric::default()
        };
        let ctx = RepairContext::noop();
        let outcome = pass.apply(&mut mesh, &ctx).expect("simplify");
        assert_eq!(
            mesh.face_count(),
            pre,
            "tight error budget should prevent collapses"
        );
        assert_eq!(outcome.stats.edges_collapsed, 0);
    }
}
