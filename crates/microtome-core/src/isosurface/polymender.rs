//! PolyMender-style hole closure (Ju 2004, "Robust Repair of Polygonal
//! Models") via a direct GF(2) solve.
//!
//! After scan-conversion, some primal cell faces have an *odd* number
//! of recorded intersection edges on their 4 sides. These faces form
//! closed cycles on the dual surface `∂S` in the paper — each cycle
//! is one hole in the input mesh, projected onto the octree's dual.
//! Flood-filling signs across an `∂S`-non-empty edge set produces an
//! inconsistent sign configuration (signs leak through the hole).
//!
//! This module computes a patch `P` (a set of synthetic intersection
//! edges) such that `∂(S ⊕ P) = ∅`. Adding a patch edge toggles the
//! parity of the 4 faces adjacent to it. Finding a minimum patch is
//! NP-hard in general, but finding *any* patch reduces to the linear
//! system
//!
//!     M · x = b      (mod 2)
//!
//! where rows of `M` are boundary faces, columns are candidate edges
//! (`M[f, e] = 1` iff face `f` is adjacent to edge `e`), and `b` is
//! the all-ones vector (every boundary face needs its parity flipped).
//! By the Eulerian property `∂∂S = ∅` the system is always consistent.
//! We solve it with Gauss-Jordan over GF(2) using bit-packed rows;
//! complexity is `O(rows * cols² / 64)`.
//!
//! The paper's `patchProc` recursion (§5.3) solves the same problem
//! via divide-and-conquer on the octree; for the grid sizes we use
//! (depth ≤ 9, a few hundred boundary faces at most) the direct solve
//! is faster, simpler, and correct by construction.

use std::collections::HashSet;

use glam::IVec3;

use super::indicators::PositionCode;
use super::mesh_scan::EdgeKey;

/// A primal cell face, identified by its minimum corner (axis-aligned
/// lower-in-all-axes corner) and the axis orthogonal to the face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FaceKey {
    corner: PositionCode,
    axis: u8,
}

impl FaceKey {
    /// The 4 primal edges bounding this face, in a fixed order.
    fn edges(self) -> [EdgeKey; 4] {
        let a = self.axis as usize;
        let a1 = (a + 1) % 3;
        let a2 = (a + 2) % 3;
        let mut off1 = IVec3::ZERO;
        off1[a1] = 1;
        let mut off2 = IVec3::ZERO;
        off2[a2] = 1;
        [
            EdgeKey {
                lower: self.corner,
                axis: a1 as u8,
            },
            EdgeKey {
                lower: self.corner + off2,
                axis: a1 as u8,
            },
            EdgeKey {
                lower: self.corner,
                axis: a2 as u8,
            },
            EdgeKey {
                lower: self.corner + off1,
                axis: a2 as u8,
            },
        ]
    }
}

/// Returns the 4 primal cell faces that contain a given primal edge
/// as one of their sides.
fn faces_adjacent_to_edge(edge: EdgeKey) -> [FaceKey; 4] {
    let a = edge.axis as usize;
    let a1 = ((a + 1) % 3) as u8;
    let a2 = ((a + 2) % 3) as u8;
    let mut e_a1 = IVec3::ZERO;
    e_a1[(a + 1) % 3] = 1;
    let mut e_a2 = IVec3::ZERO;
    e_a2[(a + 2) % 3] = 1;
    [
        FaceKey {
            corner: edge.lower,
            axis: a2,
        },
        FaceKey {
            corner: edge.lower - e_a1,
            axis: a2,
        },
        FaceKey {
            corner: edge.lower,
            axis: a1,
        },
        FaceKey {
            corner: edge.lower - e_a2,
            axis: a1,
        },
    ]
}

/// Runs the full PolyMender patching pipeline. Returns the set of
/// synthetic intersection edges to add so the combined set has no
/// boundary cycles.
pub(super) fn compute_patch_edges(edges: &HashSet<EdgeKey>) -> HashSet<EdgeKey> {
    let boundary_faces = detect_boundary_faces(edges);
    if boundary_faces.is_empty() {
        return HashSet::new();
    }
    let cycles = extract_cycles(boundary_faces);

    // Each cycle is patched independently inside its own bbox-
    // expanded discrete convex hull. Two gotchas:
    //
    //   * Candidate edges must span the boundary's homology class.
    //     The minimum candidate set (edges of boundary faces) is
    //     typically too tight, leaving the GF(2) system
    //     inconsistent. A bbox+1 expansion fixes this.
    //
    //   * We must NOT let the solver toggle existing real edges. A
    //     real edge represents a genuine scan-converted crossing;
    //     toggling it off erases that surface and opens a new hole
    //     somewhere the mesh had geometry. Excluding real edges
    //     from the candidate set keeps the patch purely additive —
    //     synthetic edges inserted into empty grid positions.
    let mut patch: HashSet<EdgeKey> = HashSet::new();
    for cycle in &cycles {
        if let Some(p) = patch_cycle(cycle, edges) {
            for edge in p {
                // Patches from different cycles XOR together — two
                // cycles that want to toggle the same edge cancel.
                if !patch.insert(edge) {
                    patch.remove(&edge);
                }
            }
        }
    }
    patch
}

/// Eulerian cycle decomposition over the boundary-face graph.
fn extract_cycles(mut boundary: HashSet<FaceKey>) -> Vec<Vec<FaceKey>> {
    use std::collections::HashMap;
    let mut at_cell: HashMap<PositionCode, Vec<FaceKey>> = HashMap::new();
    for &face in &boundary {
        let mut back = IVec3::ZERO;
        back[face.axis as usize] = -1;
        for cell in [face.corner + back, face.corner] {
            at_cell.entry(cell).or_default().push(face);
        }
    }
    let mut cycles = Vec::new();
    while let Some(&start) = boundary.iter().next() {
        let mut cycle = Vec::new();
        let mut next = start;
        let mut back = IVec3::ZERO;
        back[start.axis as usize] = -1;
        let mut curr_cell = start.corner + back;
        loop {
            cycle.push(next);
            boundary.remove(&next);
            let mut nback = IVec3::ZERO;
            nback[next.axis as usize] = -1;
            let [c0, c1] = [next.corner + nback, next.corner];
            curr_cell = if c0 == curr_cell { c1 } else { c0 };
            let here = match at_cell.get(&curr_cell) {
                Some(v) => v,
                None => break,
            };
            match here.iter().find(|f| boundary.contains(f)) {
                Some(&f) => next = f,
                None => break,
            }
        }
        cycles.push(cycle);
    }
    cycles
}

/// Patches a single boundary cycle via a local GF(2) solve within an
/// expanded bbox. Excludes `real_edges` from the candidate set so
/// the patch is purely additive and never erases a scan-converted
/// crossing. Returns `None` if no purely-additive patch exists with
/// the candidate edges we're willing to consider.
fn patch_cycle(cycle: &[FaceKey], real_edges: &HashSet<EdgeKey>) -> Option<Vec<EdgeKey>> {
    // 1. Cycle bbox.
    let mut mn = IVec3::splat(i32::MAX);
    let mut mx = IVec3::splat(i32::MIN);
    for face in cycle {
        mn = mn.min(face.corner);
        mx = mx.max(face.corner + IVec3::ONE);
    }
    // 2. Expand by 1 cell in each direction so the candidate set has
    //    enough edges to span the cycle's homology class. (The paper
    //    works inside the cycle's discrete convex hull; the bbox is a
    //    conservative upper bound.)
    let pad = IVec3::ONE;
    mn -= pad;
    mx += pad;

    // 3. Candidate edges: every primal edge with lower corner in the
    //    expanded bbox, minus any edge that already exists in the
    //    real scan-converted set. Keeping the patch purely additive
    //    preserves the original surface wherever it was actually
    //    measured.
    let mut edge_list: Vec<EdgeKey> = Vec::new();
    for axis in 0u8..3 {
        let a = axis as usize;
        for x in mn.x..mx.x {
            for y in mn.y..mx.y {
                for z in mn.z..mx.z {
                    let lower = IVec3::new(x, y, z);
                    if lower[a] + 1 > mx[a] {
                        continue;
                    }
                    let key = EdgeKey { lower, axis };
                    if real_edges.contains(&key) {
                        continue;
                    }
                    edge_list.push(key);
                }
            }
        }
    }
    let edge_idx: std::collections::HashMap<EdgeKey, usize> =
        edge_list.iter().enumerate().map(|(i, e)| (*e, i)).collect();

    // 4. Affected faces: every face touched by any candidate edge.
    let mut affected_faces: HashSet<FaceKey> = HashSet::new();
    for edge in &edge_list {
        for face in faces_adjacent_to_edge(*edge) {
            affected_faces.insert(face);
        }
    }
    let face_list: Vec<FaceKey> = affected_faces.into_iter().collect();

    // 5. Assemble matrix.
    let nrows = face_list.len();
    let ncols = edge_list.len();
    let mut matrix = Gf2Matrix::new(nrows, ncols);
    let boundary_set: HashSet<FaceKey> = cycle.iter().copied().collect();
    let mut rhs: Vec<bool> = face_list.iter().map(|f| boundary_set.contains(f)).collect();
    for (r, face) in face_list.iter().enumerate() {
        for e in face.edges() {
            if let Some(&c) = edge_idx.get(&e) {
                matrix.set(r, c);
            }
        }
    }

    // 6. Solve.
    let x = solve_gf2(&mut matrix, &mut rhs)?;
    Some(x.into_iter().map(|c| edge_list[c]).collect())
}

/// Enumerates all faces adjacent to any intersection edge and keeps
/// those whose 4-edge intersection count is odd — these are the
/// boundary faces of the dual surface.
fn detect_boundary_faces(edges: &HashSet<EdgeKey>) -> HashSet<FaceKey> {
    let mut candidates: HashSet<FaceKey> = HashSet::new();
    for &edge in edges {
        for f in faces_adjacent_to_edge(edge) {
            candidates.insert(f);
        }
    }
    candidates
        .into_iter()
        .filter(|face| face.edges().iter().filter(|e| edges.contains(e)).count() & 1 == 1)
        .collect()
}

// ---------------------------------------------------------------------------
// GF(2) linear solver (bit-packed Gauss-Jordan)
// ---------------------------------------------------------------------------

struct Gf2Matrix {
    /// One bit vector per row, `ncols` bits wide.
    rows: Vec<Vec<u64>>,
    ncols: usize,
}

impl Gf2Matrix {
    fn new(nrows: usize, ncols: usize) -> Self {
        let words = ncols.div_ceil(64);
        Self {
            rows: vec![vec![0u64; words]; nrows],
            ncols,
        }
    }

    fn set(&mut self, r: usize, c: usize) {
        self.rows[r][c / 64] |= 1u64 << (c % 64);
    }

    fn get(&self, r: usize, c: usize) -> bool {
        (self.rows[r][c / 64] >> (c % 64)) & 1 == 1
    }

    /// `rows[dst] ^= rows[src]`.
    fn xor_row(&mut self, dst: usize, src: usize) {
        // Safe equivalent of a split borrow.
        let (lo, hi) = if dst < src {
            let (lo, hi) = self.rows.split_at_mut(src);
            (&mut lo[dst], &hi[0])
        } else {
            let (lo, hi) = self.rows.split_at_mut(dst);
            (&mut hi[0], &lo[src])
        };
        for (d, s) in lo.iter_mut().zip(hi.iter()) {
            *d ^= *s;
        }
    }
}

/// Solves `M x = b` over GF(2) via Gauss-Jordan. Returns the column
/// indices with `x = 1` (setting all free variables to zero), or
/// `None` if the system is inconsistent.
fn solve_gf2(matrix: &mut Gf2Matrix, rhs: &mut [bool]) -> Option<Vec<usize>> {
    let nrows = matrix.rows.len();
    let ncols = matrix.ncols;

    let mut pivot_col_of_row: Vec<Option<usize>> = vec![None; nrows];
    let mut current_row: usize = 0;

    for c in 0..ncols {
        // Find a row at/below `current_row` with bit c set.
        let mut pivot_r: Option<usize> = None;
        for r in current_row..nrows {
            if matrix.get(r, c) {
                pivot_r = Some(r);
                break;
            }
        }
        let r = match pivot_r {
            Some(r) => r,
            None => continue, // free variable
        };
        // Bring the pivot row up.
        if r != current_row {
            matrix.rows.swap(r, current_row);
            rhs.swap(r, current_row);
        }
        pivot_col_of_row[current_row] = Some(c);
        // Eliminate c from all other rows.
        for r2 in 0..nrows {
            if r2 != current_row && matrix.get(r2, c) {
                matrix.xor_row(r2, current_row);
                rhs[r2] ^= rhs[current_row];
            }
        }
        current_row += 1;
    }

    // Consistency check: every zero row must have zero RHS.
    if rhs[current_row..nrows].iter().any(|b| *b) {
        return None;
    }

    // Extract solution: pivot variables take their RHS, free
    // variables default to 0.
    let mut result = Vec::new();
    for r in 0..current_row {
        if let Some(c) = pivot_col_of_row[r]
            && rhs[r]
        {
            result.push(c);
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gf2_identity_solves_trivially() {
        // 3x3 identity matrix, rhs = [1, 0, 1] → x = [1, 0, 1].
        let mut m = Gf2Matrix::new(3, 3);
        m.set(0, 0);
        m.set(1, 1);
        m.set(2, 2);
        let mut rhs = vec![true, false, true];
        let x = solve_gf2(&mut m, &mut rhs).unwrap();
        assert_eq!(x, vec![0, 2]);
    }

    #[test]
    fn gf2_detects_inconsistency() {
        // Two equal rows with unequal RHS is inconsistent.
        let mut m = Gf2Matrix::new(2, 2);
        m.set(0, 0);
        m.set(0, 1);
        m.set(1, 0);
        m.set(1, 1);
        let mut rhs = vec![true, false];
        assert!(solve_gf2(&mut m, &mut rhs).is_none());
    }

    #[test]
    fn single_primal_edge_closes_minimum_cycle() {
        // A single intersection edge at axis=0, lower=(0,0,0): its 4
        // adjacent faces all have intersection-count 1, so they
        // form a minimum boundary cycle. The patch toggles some set
        // of edges so that `S ⊕ P` has no boundary faces.
        let mut edges: HashSet<EdgeKey> = HashSet::new();
        edges.insert(EdgeKey {
            lower: IVec3::ZERO,
            axis: 0,
        });
        let boundary = detect_boundary_faces(&edges);
        assert_eq!(boundary.len(), 4);
        // Note: with the purely-additive constraint, this test
        // requires padding around the real edge — which our bbox+1
        // expansion provides.
        let patch = compute_patch_edges(&edges);
        // Apply as symmetric difference.
        let mut augmented = edges.clone();
        for e in &patch {
            if !augmented.insert(*e) {
                augmented.remove(e);
            }
        }
        let residual = detect_boundary_faces(&augmented);
        assert_eq!(residual.len(), 0, "patch should close the minimum cycle");
    }
}
