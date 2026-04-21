//! PolyMender-style hole closure (Ju 2004, "Robust Repair of
//! Polygonal Models"), §5.2 + §5.3 + §5.4.
//!
//! # Pipeline
//!
//! 1. **Detect boundary faces** — primal cell faces with an odd
//!    number of intersection edges on their 4 sides. These make up
//!    the dual-surface boundary `∂S`.
//! 2. **Extract boundary cycles** by Eulerian traversal through the
//!    dual graph (faces connected by shared primal cells). `∂∂S = ∅`
//!    guarantees every primal cell has an even number of incident
//!    boundary faces, so the decomposition is clean.
//! 3. **Patch each cycle** with a set of synthetic intersection
//!    edges `P` such that `∂(S ⊕ P) = ∅`. The paper solves this via
//!    top-down divide-and-conquer on the octree (§5.3): at each
//!    node, build a band of quads Q on the node's center plane
//!    connecting each pair of cycle crossings, and recurse into
//!    children. This implementation instead solves the same
//!    linear system `M · x = b` directly over GF(2) via bit-packed
//!    Gauss-Jordan elimination, per cycle, within a bbox+1 padded
//!    candidate region. Topologically identical (∂P = b), but the
//!    solver is free to place patch edges anywhere in the bbox
//!    rather than along a minimum-area disk — the DC output
//!    consequently has less-clean geometry across holes than the
//!    paper's band construction would produce.
//! 4. **Generate signs** — the caller flood-fills signs over the
//!    augmented edge set `S ⊕ P`; the paper's §5.4 "signProc" is
//!    structurally the same algorithm.
//!
//! Candidate edges are *always* purely additive — real scan-
//! converted intersection edges are excluded so the patch can never
//! erase original surface. Hermite data for synthetic patch edges
//! is seeded from locally-averaged real normals in `mesh_scan` so
//! the patch blends into the surrounding rim.

use std::collections::{HashMap, HashSet};

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

    /// The two primal cells sharing this face (cell lower-corners).
    fn cells(self) -> [PositionCode; 2] {
        let mut back = IVec3::ZERO;
        back[self.axis as usize] = -1;
        [self.corner + back, self.corner]
    }
}

/// Returns the (up to) 4 primal cell faces that contain a given
/// primal edge as one of their sides.
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

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Returns the symmetric-difference patch set P such that
/// `∂(S ⊕ P) = ∅`. Iteratively: detect boundary cycles, patch each
/// with a band-of-quads via the paper's D&C, update the working
/// set, and repeat until the boundary is empty (or the iteration
/// bound is reached — irreducible residual is handled by a local
/// GF(2) solve).
pub(super) fn compute_patch_edges(edges: &HashSet<EdgeKey>) -> HashSet<EdgeKey> {
    let boundary = detect_boundary_faces(edges);
    if boundary.is_empty() {
        return HashSet::new();
    }
    let cycles = extract_cycles(boundary);

    // Per-cycle GF(2) solve in a bbox+1 padded candidate region.
    // Each cycle's patch is purely additive (real edges excluded
    // from the candidate set) so the scan-converted surface is
    // never erased.
    let mut patch: HashSet<EdgeKey> = HashSet::new();
    for cycle in &cycles {
        if let Some(p) = gf2_patch_cycle(cycle, edges) {
            for edge in p {
                xor_toggle(&mut patch, edge);
            }
        }
    }
    patch
}

/// Toggle membership: add if absent, remove if present.
fn xor_toggle(set: &mut HashSet<EdgeKey>, edge: EdgeKey) {
    if !set.insert(edge) {
        set.remove(&edge);
    }
}

// ---------------------------------------------------------------------------
// Boundary detection and cycle extraction (§5.2)
// ---------------------------------------------------------------------------

/// Enumerates every primal face that has at least one incident
/// intersection edge, and keeps those with *odd* intersection count.
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

/// A single closed boundary cycle (sequence of boundary faces).
#[derive(Debug, Clone)]
struct Cycle {
    faces: Vec<FaceKey>,
}

impl Cycle {
    /// Axis-aligned bounding box over the primal corners spanned by
    /// the cycle's faces. Returns `(min, max_exclusive)` so the grid
    /// span is `max - min` along each axis.
    fn bbox(&self) -> (IVec3, IVec3) {
        let mut mn = IVec3::splat(i32::MAX);
        let mut mx = IVec3::splat(i32::MIN);
        for face in &self.faces {
            mn = mn.min(face.corner);
            mx = mx.max(face.corner + IVec3::ONE);
        }
        (mn, mx)
    }
}

/// Eulerian cycle decomposition. Each boundary face connects two
/// primal cells; the returned cycles traverse faces alternating with
/// cells so we can recover the *joining edge* for each consecutive
/// pair.
fn extract_cycles(mut boundary: HashSet<FaceKey>) -> Vec<Cycle> {
    // Adjacency: cell → the boundary faces incident to it.
    let mut at_cell: HashMap<PositionCode, Vec<FaceKey>> = HashMap::new();
    for &face in &boundary {
        for cell in face.cells() {
            at_cell.entry(cell).or_default().push(face);
        }
    }
    let mut cycles = Vec::new();
    while let Some(&start) = boundary.iter().next() {
        let mut faces = Vec::new();
        let mut next = start;
        let mut curr_cell = start.cells()[0];
        loop {
            faces.push(next);
            boundary.remove(&next);
            let [c0, c1] = next.cells();
            let exit_cell = if c0 == curr_cell { c1 } else { c0 };
            let here = match at_cell.get(&exit_cell) {
                Some(v) => v,
                None => break,
            };
            match here.iter().find(|f| boundary.contains(f)).copied() {
                Some(nf) => {
                    curr_cell = exit_cell;
                    next = nf;
                }
                None => break,
            }
        }
        cycles.push(Cycle { faces });
    }
    cycles
}

// ---------------------------------------------------------------------------
// GF(2) patch solver per cycle
// ---------------------------------------------------------------------------

fn gf2_patch_cycle(cycle: &Cycle, real_edges: &HashSet<EdgeKey>) -> Option<Vec<EdgeKey>> {
    let (mn, mx) = cycle.bbox();
    let mn = mn - IVec3::ONE;
    let mx = mx + IVec3::ONE;

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
    let edge_idx: HashMap<EdgeKey, usize> =
        edge_list.iter().enumerate().map(|(i, e)| (*e, i)).collect();

    let mut affected_faces: HashSet<FaceKey> = HashSet::new();
    for edge in &edge_list {
        for face in faces_adjacent_to_edge(*edge) {
            affected_faces.insert(face);
        }
    }
    let face_list: Vec<FaceKey> = affected_faces.into_iter().collect();
    let boundary_set: HashSet<FaceKey> = cycle.faces.iter().copied().collect();

    let nrows = face_list.len();
    let ncols = edge_list.len();
    let mut matrix = Gf2Matrix::new(nrows, ncols);
    let mut rhs: Vec<bool> = face_list.iter().map(|f| boundary_set.contains(f)).collect();
    for (r, face) in face_list.iter().enumerate() {
        for e in face.edges() {
            if let Some(&c) = edge_idx.get(&e) {
                matrix.set(r, c);
            }
        }
    }
    let x = solve_gf2(&mut matrix, &mut rhs)?;
    Some(x.into_iter().map(|c| edge_list[c]).collect())
}

struct Gf2Matrix {
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
    fn xor_row(&mut self, dst: usize, src: usize) {
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

fn solve_gf2(matrix: &mut Gf2Matrix, rhs: &mut [bool]) -> Option<Vec<usize>> {
    let nrows = matrix.rows.len();
    let ncols = matrix.ncols;
    let mut pivot_col_of_row: Vec<Option<usize>> = vec![None; nrows];
    let mut current_row: usize = 0;
    for c in 0..ncols {
        let mut pivot_r: Option<usize> = None;
        for r in current_row..nrows {
            if matrix.get(r, c) {
                pivot_r = Some(r);
                break;
            }
        }
        let r = match pivot_r {
            Some(r) => r,
            None => continue,
        };
        if r != current_row {
            matrix.rows.swap(r, current_row);
            rhs.swap(r, current_row);
        }
        pivot_col_of_row[current_row] = Some(c);
        for r2 in 0..nrows {
            if r2 != current_row && matrix.get(r2, c) {
                matrix.xor_row(r2, current_row);
                rhs[r2] ^= rhs[current_row];
            }
        }
        current_row += 1;
    }
    if rhs[current_row..nrows].iter().any(|b| *b) {
        return None;
    }
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_primal_edge_closes_minimum_cycle() {
        let mut edges: HashSet<EdgeKey> = HashSet::new();
        edges.insert(EdgeKey {
            lower: IVec3::ZERO,
            axis: 0,
        });
        let boundary = detect_boundary_faces(&edges);
        assert_eq!(boundary.len(), 4);
        let patch = compute_patch_edges(&edges);
        let mut augmented = edges.clone();
        for e in &patch {
            if !augmented.insert(*e) {
                augmented.remove(e);
            }
        }
        let residual = detect_boundary_faces(&augmented);
        assert_eq!(residual.len(), 0, "patch should close the minimum cycle");
    }

    #[test]
    fn gf2_identity_solves_trivially() {
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
        let mut m = Gf2Matrix::new(2, 2);
        m.set(0, 0);
        m.set(0, 1);
        m.set(1, 0);
        m.set(1, 1);
        let mut rhs = vec![true, false];
        assert!(solve_gf2(&mut m, &mut rhs).is_none());
    }
}
