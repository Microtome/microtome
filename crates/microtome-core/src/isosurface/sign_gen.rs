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

use glam::IVec3;

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

/// A splitting plane for the recursive patch construction (paper §5.3):
/// a primal axis-aligned plane that cuts through cycle `b` at exactly
/// two faces `e1`, `e2` whose perpendicular axes equal the plane's
/// normal — the "orthogonal edges" in figure 7 of the paper.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub(super) struct SplittingPlane {
    /// Normal axis of the plane (0 = X, 1 = Y, 2 = Z).
    pub(super) axis: u8,
    /// Integer position of the plane along its normal axis.
    pub(super) position: i32,
    /// The two dual edges (primal faces in `b`) that lie on the plane.
    pub(super) e1: FaceKey,
    pub(super) e2: FaceKey,
}

/// Builds the band of primal edges `Q` connecting `e1` and `e2` on the
/// splitting plane. Q's dual quads form a 2D strip in the plane from
/// the face-center of `e1` to the face-center of `e2`.
///
/// Paper §5.3: "Q as the quads dual to the cell edges on the primal
/// grid that cross `h`." We interpret this as primal edges along the
/// plane's normal axis, at integer (b1, b2) positions visited by a
/// 4-connected path between the two face centers.
///
/// For the initial implementation the path is L-shaped (run along b1
/// first, then b2). This is a valid 4-connected path between integer
/// endpoints; a straight line tracer can replace it without changing
/// the rest of the pipeline.
#[allow(dead_code)]
pub(super) fn build_band(plane: SplittingPlane) -> Vec<EdgeKey> {
    let a_s = plane.axis as usize;
    let b1 = (a_s + 1) % 3;
    let b2 = (a_s + 2) % 3;

    let start = (plane.e1.lower[b1], plane.e1.lower[b2]);
    let end = (plane.e2.lower[b1], plane.e2.lower[b2]);
    let cells_2d = l_shape_path(start, end);

    let mut q = Vec::with_capacity(cells_2d.len());
    for (i, j) in cells_2d {
        let mut lower = IVec3::ZERO;
        // Sit the band one cell "below" the plane (c[a_S] = L - 1). This
        // is arbitrary — either side is a valid placement; picking one
        // consistently matters for the symmetric-difference cycle math
        // that composes patches in the caller.
        lower[a_s] = plane.position - 1;
        lower[b1] = i;
        lower[b2] = j;
        q.push(EdgeKey {
            lower,
            axis: plane.axis,
        });
    }
    q
}

/// 4-connected L-shaped path between integer 2D points: walks along the
/// b1 axis first, then b2. Visits every grid cell along the way,
/// including endpoints.
fn l_shape_path(start: (i32, i32), end: (i32, i32)) -> Vec<(i32, i32)> {
    let mut cells = vec![start];
    let mut current = start;

    let sx = (end.0 - current.0).signum();
    while current.0 != end.0 {
        current.0 += sx;
        cells.push(current);
    }
    let sy = (end.1 - current.1).signum();
    while current.1 != end.1 {
        current.1 += sy;
        cells.push(current);
    }
    cells
}

/// Finds a splitting plane that intersects cycle `b` at exactly two
/// orthogonal dual edges. Returns `None` if no such plane exists — for
/// simple non-empty cycles the paper guarantees one does.
///
/// The search iterates axes and positions and returns the first hit.
/// Cycles with multi-wind behavior (more than 2 faces of the same axis
/// at the same position) still produce valid planes elsewhere.
#[allow(dead_code)]
pub(super) fn pick_splitting_plane(b: &[FaceKey]) -> Option<SplittingPlane> {
    for axis in 0u8..3 {
        let mut by_position: HashMap<i32, Vec<FaceKey>> = HashMap::new();
        for &face in b {
            if face.axis == axis {
                by_position
                    .entry(face.lower[axis as usize])
                    .or_default()
                    .push(face);
            }
        }
        for (position, faces) in by_position {
            if faces.len() == 2 {
                return Some(SplittingPlane {
                    axis,
                    position,
                    e1: faces[0],
                    e2: faces[1],
                });
            }
        }
    }
    None
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

    // -----------------------------------------------------------------
    // Splitting plane tests
    // -----------------------------------------------------------------

    #[test]
    fn splitting_plane_exists_for_square_cycle() {
        // The 4 faces around a single X-axis edge form a cycle. Both
        // axis=1 (Y) and axis=2 (Z) have 2 faces at position 0, so a
        // valid splitting plane exists.
        let edge = EdgeKey {
            lower: IVec3::ZERO,
            axis: 0,
        };
        let faces: HashSet<FaceKey> = faces_containing_edge(edge).iter().copied().collect();
        let cycles = extract_boundary_cycles(&faces);
        assert_eq!(cycles.len(), 1);

        let plane = pick_splitting_plane(&cycles[0]).expect("splitting plane must exist");

        // axis X (0) has no faces in this cycle; only Y (1) and Z (2) are valid.
        assert!(plane.axis == 1 || plane.axis == 2);
        assert_eq!(plane.position, 0);
        assert_ne!(plane.e1, plane.e2);
        assert_eq!(plane.e1.axis, plane.axis);
        assert_eq!(plane.e2.axis, plane.axis);
        assert_eq!(plane.e1.lower[plane.axis as usize], plane.position);
        assert_eq!(plane.e2.lower[plane.axis as usize], plane.position);
    }

    #[test]
    fn splitting_plane_none_for_empty_cycle() {
        assert!(pick_splitting_plane(&[]).is_none());
    }

    #[test]
    fn splitting_plane_picks_correct_axis_for_planar_cycle() {
        // Build a larger rectangular cycle lying in the Z=5 plane.
        // Outline: 4 cells on the Z=5 plane forming a 2x2 block.
        //
        // Cells at Z=5: (0,0,5), (1,0,5), (0,1,5), (1,1,5)
        // Faces between them on the Z=5 plane: X-perp at x=1 between
        // (0,*,5)-(1,*,5), and Y-perp at y=1 between (*,0,5)-(*,1,5).
        //
        // ∂S faces for a cycle around the 2x2 block: 8 exterior faces
        // (4 axis=0 at x=0 and x=2, 4 axis=1 at y=0 and y=2 — wait let
        // me think again).
        //
        // Actually, easiest: use the 4 faces around a single Z-axis
        // edge at (5, 5, 5) axis=2. Those faces all have axis=0 or 1.
        let edge = EdgeKey {
            lower: IVec3::new(5, 5, 5),
            axis: 2,
        };
        let faces: HashSet<FaceKey> = faces_containing_edge(edge).iter().copied().collect();
        let cycles = extract_boundary_cycles(&faces);
        assert_eq!(cycles.len(), 1);

        let plane = pick_splitting_plane(&cycles[0]).expect("splitting plane must exist");
        // Faces have axis 0 or 1, at position 5. Plane must match.
        assert!(plane.axis == 0 || plane.axis == 1);
        assert_eq!(plane.position, 5);
    }

    // -----------------------------------------------------------------
    // Band construction tests
    // -----------------------------------------------------------------

    #[test]
    fn band_along_b1_has_one_edge_per_cell() {
        // Splitting plane at X=5. e1 at (5, 0, 0), e2 at (5, 3, 0) —
        // both axis=0, separated by 3 along Y. The band walks Y from
        // 0 to 3, so 4 cells → 4 edges.
        let plane = SplittingPlane {
            axis: 0,
            position: 5,
            e1: FaceKey {
                lower: IVec3::new(5, 0, 0),
                axis: 0,
            },
            e2: FaceKey {
                lower: IVec3::new(5, 3, 0),
                axis: 0,
            },
        };
        let q = build_band(plane);
        assert_eq!(q.len(), 4);
        for (idx, edge) in q.iter().enumerate() {
            assert_eq!(edge.axis, 0);
            assert_eq!(edge.lower[0], 4, "edge {idx} lower.x");
            assert_eq!(edge.lower[1], idx as i32, "edge {idx} lower.y");
            assert_eq!(edge.lower[2], 0, "edge {idx} lower.z");
        }
    }

    #[test]
    fn band_diagonal_traces_l_shape() {
        // e1 at (2, 0, 0), e2 at (2, 3, 3). Diagonal in Y-Z. L-shape
        // walks Y first (0→3) then Z (0→3): 7 cells total.
        let plane = SplittingPlane {
            axis: 0,
            position: 2,
            e1: FaceKey {
                lower: IVec3::new(2, 0, 0),
                axis: 0,
            },
            e2: FaceKey {
                lower: IVec3::new(2, 3, 3),
                axis: 0,
            },
        };
        let q = build_band(plane);
        assert_eq!(q.len(), 7);
        // Every edge along the same axis and at c.x = 1.
        for edge in &q {
            assert_eq!(edge.axis, 0);
            assert_eq!(edge.lower[0], 1);
        }
    }

    #[test]
    fn band_for_different_plane_axis() {
        // Splitting plane at Y=5; the band lies along the X-Z plane.
        // With axis=1, b1=2=Z, b2=0=X. So e1.lower[b1]=e1.lower[2]=0,
        // e1.lower[b2]=e1.lower[0]=0; e2.lower[b1]=2, e2.lower[b2]=0.
        // Walk is along Z from 0 to 2: 3 cells.
        let plane = SplittingPlane {
            axis: 1,
            position: 5,
            e1: FaceKey {
                lower: IVec3::new(0, 5, 0),
                axis: 1,
            },
            e2: FaceKey {
                lower: IVec3::new(0, 5, 2),
                axis: 1,
            },
        };
        let q = build_band(plane);
        assert_eq!(q.len(), 3);
        for edge in &q {
            assert_eq!(edge.axis, 1);
            assert_eq!(edge.lower[1], 4, "band below the plane in Y");
        }
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
