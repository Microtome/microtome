//! PolyMender-style sign generation for a scan-converted mesh.
//!
//! Takes the set of intersection edges produced by
//! [`mesh_scan`](super::mesh_scan) and produces a consistent inside/outside
//! sign at every grid corner — even for meshes with holes, gaps, or
//! non-manifold boundaries, where a naive flood-fill would propagate signs
//! through the hole and misclassify interior corners.
//!
//! The algorithm (Ju 2004, §5):
//!
//! 1. **Detect odd faces.** A primal cell face is "odd" if 1 or 3 of its
//!    four bounding primal edges are intersection edges. These are where
//!    the dual surface `S` has a boundary.
//!
//! 2. **Extract boundary cycles.** The odd faces form an Eulerian graph
//!    (every incident primal cell has even valence); decompose into
//!    disjoint closed cycles `b_i`.
//!
//! 3. **Patch each cycle.** For each cycle `b_i`, compute a patch `P_i` of
//!    primal edges whose dual quads form a disk with boundary `b_i`. The
//!    patch is found by projecting the cycle onto a principal axis,
//!    running a minimum-cost DP triangulation (Garland-Heckbert), and
//!    lifting the triangulation back to primal edges.
//!
//! 4. **Flood-fill signs on `E ⊕ ⋃P_i`.** Because the extended edge set
//!    makes `∂(Ŝ) = ∅`, the BFS produces consistent signs.
//!
//! This module is pub(super); all its types stay internal to
//! [`isosurface`](super).

use std::collections::{HashMap, HashSet};

use super::indicators::PositionCode;
use super::mesh_scan::EdgeKey;

/// Ordered sequence of `FaceKey` edges forming a closed walk on the
/// boundary graph. A cycle has at least one face. The walk returns to its
/// starting cell: following each face to the other side traces out the
/// loop back to the origin.
#[allow(dead_code)]
pub(super) type BoundaryCycle = Vec<FaceKey>;

/// Identifies a primal cell face by its lower-corner grid code and the
/// axis perpendicular to it (0 = X, 1 = Y, 2 = Z).
///
/// The face occupies the 1×1 square spanning the two non-perpendicular
/// axes, with its minimum corner at `lower`. Faces are shared between two
/// cells; this canonical form picks the representation whose `lower` is
/// the lesser corner along the perpendicular axis.
#[derive(Debug, Hash, Eq, PartialEq, Clone, Copy)]
pub(super) struct FaceKey {
    pub(super) lower: PositionCode,
    pub(super) axis: u8,
}

/// Returns the 4 primal faces that contain the given primal edge as one
/// of their four bounding edges.
///
/// An edge is bounded by 4 faces: 2 perpendicular to each of the two axes
/// orthogonal to the edge's axis.
// `allow(dead_code)` until [`detect_odd_faces`] is wired into
// `ScannedMeshField::from_mesh` later in the sign-generation work.
#[allow(dead_code)]
fn faces_containing_edge(edge: EdgeKey) -> [FaceKey; 4] {
    let a = edge.axis as usize;
    let b1 = (a + 1) % 3;
    let b2 = (a + 2) % 3;

    let mut neg_b1 = edge.lower;
    neg_b1[b1] -= 1;
    let mut neg_b2 = edge.lower;
    neg_b2[b2] -= 1;

    [
        FaceKey {
            lower: edge.lower,
            axis: b1 as u8,
        },
        FaceKey {
            lower: neg_b2,
            axis: b1 as u8,
        },
        FaceKey {
            lower: edge.lower,
            axis: b2 as u8,
        },
        FaceKey {
            lower: neg_b1,
            axis: b2 as u8,
        },
    ]
}

/// Enumerates the primal faces incident to an odd number of intersection
/// edges — the boundary of the dual surface `S`.
///
/// Runs in O(|edges|): each edge touches 4 faces, and the histogram over
/// face incidence is built in one pass.
// `allow(dead_code)` until this is called from `ScannedMeshField::from_mesh`
// in a later commit of the sign-generation work.
#[allow(dead_code)]
pub(super) fn detect_odd_faces<V>(edges: &HashMap<EdgeKey, V>) -> HashSet<FaceKey> {
    let mut counts: HashMap<FaceKey, u32> = HashMap::with_capacity(edges.len() * 4);
    for edge_key in edges.keys() {
        for face in faces_containing_edge(*edge_key) {
            *counts.entry(face).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter_map(|(face, count)| if count % 2 == 1 { Some(face) } else { None })
        .collect()
}

/// Returns the two primal cells that share the given face. The face's
/// `lower` corner is on the upper cell's side; the lower cell sits one
/// unit back along the face's perpendicular axis.
#[allow(dead_code)]
fn cells_adjacent_to_face(face: FaceKey) -> [PositionCode; 2] {
    let mut lower = face.lower;
    lower[face.axis as usize] -= 1;
    [lower, face.lower]
}

/// Decomposes the odd-face set into disjoint simple closed cycles.
///
/// Nodes of ∂(S) are primal cells; edges are the odd faces. Every cell
/// has even valence in ∂(S) (paper §5, Euler argument), so we can always
/// partition the edges into simple cycles.
///
/// Algorithm: Hierholzer-style walk that emits a simple cycle each time
/// the walk revisits a node in its current path. Per the paper this
/// yields edge-disjoint cycles `b_i` — the inputs to [patch computation].
#[allow(dead_code)]
pub(super) fn extract_boundary_cycles(odd_faces: &HashSet<FaceKey>) -> Vec<BoundaryCycle> {
    let mut adjacency: HashMap<PositionCode, Vec<FaceKey>> = HashMap::new();
    for &face in odd_faces {
        let [c0, c1] = cells_adjacent_to_face(face);
        adjacency.entry(c0).or_default().push(face);
        adjacency.entry(c1).or_default().push(face);
    }

    let mut used: HashSet<FaceKey> = HashSet::with_capacity(odd_faces.len());
    let mut cycles: Vec<BoundaryCycle> = Vec::new();

    while let Some(start) = find_unstarted(&adjacency, &used) {
        walk_from(&adjacency, &mut used, start, &mut cycles);
    }

    cycles
}

/// Returns any cell that still has an unused incident odd face.
fn find_unstarted(
    adjacency: &HashMap<PositionCode, Vec<FaceKey>>,
    used: &HashSet<FaceKey>,
) -> Option<PositionCode> {
    adjacency.iter().find_map(|(cell, faces)| {
        if faces.iter().any(|f| !used.contains(f)) {
            Some(*cell)
        } else {
            None
        }
    })
}

/// Walks from `start` along unused odd faces, emitting a simple cycle
/// every time the walk returns to a node it already visited. Continues
/// until no unused edges are reachable from the current walk tip.
fn walk_from(
    adjacency: &HashMap<PositionCode, Vec<FaceKey>>,
    used: &mut HashSet<FaceKey>,
    start: PositionCode,
    cycles: &mut Vec<BoundaryCycle>,
) {
    let mut path_nodes: Vec<PositionCode> = vec![start];
    let mut path_edges: Vec<FaceKey> = Vec::new();
    let mut in_path: HashMap<PositionCode, usize> = HashMap::new();
    in_path.insert(start, 0);

    let mut current = start;
    loop {
        let Some(face) = next_unused_edge(adjacency, used, current) else {
            break;
        };
        used.insert(face);
        path_edges.push(face);

        let [c0, c1] = cells_adjacent_to_face(face);
        let next = if c0 == current { c1 } else { c0 };

        if let Some(&idx) = in_path.get(&next) {
            // Closing a simple cycle: everything from position `idx` onward
            // forms a loop (node at idx → ... → current → face → node at idx).
            let cycle: Vec<FaceKey> = path_edges.drain(idx..).collect();
            cycles.push(cycle);
            for popped in path_nodes.drain(idx + 1..) {
                in_path.remove(&popped);
            }
            current = next;
        } else {
            in_path.insert(next, path_nodes.len());
            path_nodes.push(next);
            current = next;
        }
    }
}

/// Returns any unused face incident to `cell` in the adjacency graph.
fn next_unused_edge(
    adjacency: &HashMap<PositionCode, Vec<FaceKey>>,
    used: &HashSet<FaceKey>,
    cell: PositionCode,
) -> Option<FaceKey> {
    adjacency
        .get(&cell)?
        .iter()
        .find(|f| !used.contains(f))
        .copied()
}

/// Returns the 4 primal edges that bound the given primal face.
///
/// Used by later stages (cycle extraction, patching) to navigate between
/// faces and their adjacent edges.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn edges_bounding_face(face: FaceKey) -> [EdgeKey; 4] {
    let a = face.axis as usize;
    let b1 = (a + 1) % 3;
    let b2 = (a + 2) % 3;

    let mut c_b2 = face.lower;
    c_b2[b2] += 1;
    let mut c_b1 = face.lower;
    c_b1[b1] += 1;

    [
        EdgeKey {
            lower: face.lower,
            axis: b1 as u8,
        },
        EdgeKey {
            lower: c_b2,
            axis: b1 as u8,
        },
        EdgeKey {
            lower: face.lower,
            axis: b2 as u8,
        },
        EdgeKey {
            lower: c_b1,
            axis: b2 as u8,
        },
    ]
}

#[cfg(test)]
mod tests {
    use glam::IVec3;

    use super::*;

    /// Minimal `EdgeHit`-substitute for the `detect_odd_faces` generic —
    /// the function only needs the edge *keys*, so the value type is
    /// irrelevant to its behavior.
    fn edge_set(edges: &[EdgeKey]) -> HashMap<EdgeKey, ()> {
        edges.iter().map(|&e| (e, ())).collect()
    }

    fn edge(lx: i32, ly: i32, lz: i32, axis: u8) -> EdgeKey {
        EdgeKey {
            lower: IVec3::new(lx, ly, lz),
            axis,
        }
    }

    #[test]
    fn empty_edge_set_has_no_odd_faces() {
        let edges = edge_set(&[]);
        let odd = detect_odd_faces(&edges);
        assert!(odd.is_empty());
    }

    #[test]
    fn single_edge_makes_four_odd_faces() {
        // A single intersection edge touches 4 faces, each with count 1.
        let edges = edge_set(&[edge(0, 0, 0, 0)]);
        let odd = detect_odd_faces(&edges);
        assert_eq!(odd.len(), 4);
    }

    #[test]
    fn face_with_all_four_edges_is_not_odd() {
        // The 4 primal edges bounding face ((0,0,0), axis=X) are the 4
        // Y- and Z-edges of the YZ square at x=0. Put all 4 in E; the
        // face itself has count 4 → even → not odd.
        let face = FaceKey {
            lower: IVec3::ZERO,
            axis: 0,
        };
        let bounding = edges_bounding_face(face);
        let edges = edge_set(&bounding);
        let odd = detect_odd_faces(&edges);
        assert!(
            !odd.contains(&face),
            "face with 4 bounding intersection edges should be even"
        );
    }

    #[test]
    fn faces_containing_edge_round_trip() {
        // For each of the 4 faces returned by faces_containing_edge(e),
        // e must appear in edges_bounding_face(f).
        let e = edge(3, 5, 7, 1);
        for face in faces_containing_edge(e) {
            let bounding = edges_bounding_face(face);
            assert!(
                bounding.contains(&e),
                "edge {:?} not in bounding set of face {:?}: {:?}",
                e,
                face,
                bounding
            );
        }
    }

    #[test]
    fn faces_containing_edge_are_distinct() {
        let e = edge(0, 0, 0, 2);
        let faces = faces_containing_edge(e);
        let unique: HashSet<_> = faces.iter().copied().collect();
        assert_eq!(unique.len(), 4);
    }

    // -----------------------------------------------------------------
    // Cycle extraction tests
    // -----------------------------------------------------------------

    /// Attempts to walk `cycle` starting at `start`. Returns `true` iff
    /// every face in the walk is adjacent to the current cell and the
    /// walk returns to `start`.
    fn cycle_walks_back_to(cycle: &[FaceKey], start: PositionCode) -> bool {
        let mut current = start;
        for &face in cycle {
            let [a, b] = cells_adjacent_to_face(face);
            if a == current {
                current = b;
            } else if b == current {
                current = a;
            } else {
                return false;
            }
        }
        current == start
    }

    /// Asserts that `cycle` closes starting at either cell of its first face.
    fn assert_is_closed_cycle(cycle: &[FaceKey]) {
        assert!(!cycle.is_empty());
        let [c0, c1] = cells_adjacent_to_face(cycle[0]);
        assert!(
            cycle_walks_back_to(cycle, c0) || cycle_walks_back_to(cycle, c1),
            "cycle does not close from either cell of its first face: {cycle:?}"
        );
    }

    #[test]
    fn empty_odd_faces_yields_no_cycles() {
        let cycles = extract_boundary_cycles(&HashSet::new());
        assert!(cycles.is_empty());
    }

    #[test]
    fn single_square_cycle() {
        // 4 faces around a single primal edge form a cycle in ∂(S).
        // Take edge (origin, axis=X): the 4 faces containing it all share
        // axis ≠ X and tile the yz-ring around the edge.
        let edge = EdgeKey {
            lower: IVec3::ZERO,
            axis: 0,
        };
        let faces: HashSet<FaceKey> = faces_containing_edge(edge).iter().copied().collect();
        assert_eq!(faces.len(), 4);

        let cycles = extract_boundary_cycles(&faces);
        assert_eq!(cycles.len(), 1, "expected one cycle, got {cycles:?}");
        assert_eq!(cycles[0].len(), 4);
        assert_is_closed_cycle(&cycles[0]);

        // Every face is used exactly once.
        let used: HashSet<FaceKey> = cycles[0].iter().copied().collect();
        assert_eq!(used, faces);
    }

    #[test]
    fn two_disjoint_cycles() {
        let e1 = EdgeKey {
            lower: IVec3::new(0, 0, 0),
            axis: 0,
        };
        let e2 = EdgeKey {
            lower: IVec3::new(10, 10, 10),
            axis: 0,
        };
        let mut faces: HashSet<FaceKey> = HashSet::new();
        faces.extend(faces_containing_edge(e1));
        faces.extend(faces_containing_edge(e2));

        let cycles = extract_boundary_cycles(&faces);
        assert_eq!(cycles.len(), 2);
        for cycle in &cycles {
            assert_eq!(cycle.len(), 4);
            assert_is_closed_cycle(cycle);
        }
        // All faces accounted for with no overlap.
        let used: HashSet<FaceKey> = cycles.iter().flat_map(|c| c.iter().copied()).collect();
        assert_eq!(used, faces);
    }

    #[test]
    fn every_face_used_exactly_once() {
        // Three disjoint ring-cycles around three parallel X-edges.
        let edges = [
            EdgeKey {
                lower: IVec3::new(0, 0, 0),
                axis: 0,
            },
            EdgeKey {
                lower: IVec3::new(20, 0, 0),
                axis: 0,
            },
            EdgeKey {
                lower: IVec3::new(0, 20, 0),
                axis: 0,
            },
        ];
        let mut faces: HashSet<FaceKey> = HashSet::new();
        for &e in &edges {
            faces.extend(faces_containing_edge(e));
        }

        let cycles = extract_boundary_cycles(&faces);
        assert_eq!(cycles.len(), 3);
        let mut used = Vec::new();
        for cycle in &cycles {
            assert_is_closed_cycle(cycle);
            used.extend(cycle.iter().copied());
        }
        let used_set: HashSet<FaceKey> = used.iter().copied().collect();
        assert_eq!(used_set.len(), used.len(), "same face appears twice");
        assert_eq!(used_set, faces);
    }

    #[test]
    fn odd_faces_respect_parity() {
        // 3 edges bounding the same face → that face has count 3 (odd).
        let face = FaceKey {
            lower: IVec3::ZERO,
            axis: 2, // Z-perpendicular
        };
        let bounding = edges_bounding_face(face);
        let edges = edge_set(&bounding[..3]);
        let odd = detect_odd_faces(&edges);
        assert!(
            odd.contains(&face),
            "face with 3 bounding edges should be odd"
        );
    }
}
