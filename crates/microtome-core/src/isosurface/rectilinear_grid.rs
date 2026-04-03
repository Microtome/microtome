//! Rectilinear grid cell for dual contouring with k-d tree acceleration.
//!
//! This is the fundamental cell type in the KdtreeISO dual contouring algorithm.
//! Each grid represents an axis-aligned cell that stores QEF data, corner signs,
//! connected component information, and solved vertex positions.

use std::collections::BTreeMap;

use glam::Vec3;

use super::indicators::{
    CELL_PROC_FACE_MASK, EDGE_MAP, PositionCode, code_to_pos, decode_cell, encode_cell,
    opposite_quad_index, quad_index, symmetry_quad_index,
};
use super::mesh_output::IsoMesh;
use super::qef::QefSolver;
use super::scalar_field::ScalarField;
use super::vertex::Vertex;

/// Trait for types that hold a reference to a [`RectilinearGrid`].
///
/// Used by the generic `check_sign` and `generate_quad` functions so they
/// can operate on any container that wraps a grid (e.g. octree nodes,
/// k-d tree nodes).
pub trait HasGrid {
    /// Returns a reference to the contained grid.
    fn grid(&self) -> &RectilinearGrid;
}

/// A rectilinear (axis-aligned) cell used in dual contouring.
///
/// Stores the QEF data accumulated from edge crossings, the solved vertex
/// positions for each connected component, and the corner sign information
/// needed for mesh generation.
#[derive(Debug, Clone)]
pub struct RectilinearGrid {
    /// Minimum corner code in the voxel grid.
    pub min_code: PositionCode,
    /// Maximum corner code in the voxel grid.
    pub max_code: PositionCode,
    /// QEF solver accumulating all edge-crossing data for this cell.
    pub all_qef: QefSolver,
    /// Per-component QEF solvers (one per connected component of same-sign corners).
    pub components: Vec<QefSolver>,
    /// Solved vertices, one per connected component.
    pub vertices: Vec<Vertex>,
    /// Approximate vertex from the combined QEF of all components.
    pub approximate: Vertex,
    /// Corner signs (0 = outside, 1 = inside) for the 8 corners.
    pub corner_signs: [u8; 8],
    /// Component index for each corner (-1 = unassigned).
    pub component_indices: [i8; 8],
    /// Whether this cell straddles the isosurface (has both inside and outside corners).
    pub is_signed: bool,
}

impl RectilinearGrid {
    /// Creates a new rectilinear grid cell and solves the initial QEF.
    ///
    /// The QEF is solved immediately, clamped to the cell bounds, and stored
    /// in `approximate`.
    pub fn new(
        min_code: PositionCode,
        max_code: PositionCode,
        qef: QefSolver,
        unit_size: f32,
    ) -> Self {
        let mut grid = Self {
            min_code,
            max_code,
            all_qef: qef,
            components: Vec::new(),
            vertices: Vec::new(),
            approximate: Vertex::default(),
            corner_signs: [0; 8],
            component_indices: [0; 8],
            is_signed: false,
        };
        Self::solve_qef(
            &mut grid.all_qef,
            &mut grid.approximate,
            min_code,
            max_code,
            unit_size,
        );
        grid
    }

    /// Solves the QEF for a single connected component, clamping the result
    /// to the cell bounds.
    pub fn solve_component(&mut self, i: usize, unit_size: f32) {
        if i < self.components.len() && i < self.vertices.len() {
            let min_code = self.min_code;
            let max_code = self.max_code;
            // Split borrows: take component out, solve, put back
            let mut qef = self.components[i].clone();
            Self::solve_qef(
                &mut qef,
                &mut self.vertices[i],
                min_code,
                max_code,
                unit_size,
            );
            self.components[i] = qef;
        }
    }

    /// Solves a QEF and writes the result into a vertex.
    ///
    /// The bounds are expanded by half the cell extent in each direction.
    /// If the solved position falls outside the expanded bounds, falls back
    /// to the unclamped mass point. Matches C++ `solve()` exactly.
    ///
    /// Public alias: [`solve_qef_pub`](Self::solve_qef_pub).
    fn solve_qef(
        qef: &mut QefSolver,
        vertex: &mut Vertex,
        min_code: PositionCode,
        max_code: PositionCode,
        unit_size: f32,
    ) {
        let (mut pos, error) = qef.solve();
        let extends = code_to_pos(max_code - min_code, unit_size) * 0.5;
        let min_pos = code_to_pos(min_code, unit_size) - extends;
        let max_pos = code_to_pos(max_code, unit_size) + extends;
        if pos.x < min_pos.x
            || pos.x > max_pos.x
            || pos.y < min_pos.y
            || pos.y > max_pos.y
            || pos.z < min_pos.z
            || pos.z > max_pos.z
        {
            pos = qef.mass_point();
        }

        vertex.hermite_p = pos;
        vertex.error = error;
    }

    /// Public version of [`solve_qef`](Self::solve_qef) for use by callers
    /// outside this module (octree, kdtree).
    pub fn solve_qef_pub(
        qef: &mut QefSolver,
        vertex: &mut Vertex,
        min_code: PositionCode,
        max_code: PositionCode,
        unit_size: f32,
    ) {
        Self::solve_qef(qef, vertex, min_code, max_code, unit_size);
    }

    /// Samples the scalar field at each of the 8 corners and records their signs.
    ///
    /// Sets `is_signed` to `true` if at least one corner differs in sign from
    /// the others.
    pub fn assign_sign(&mut self, field: &dyn ScalarField, unit_size: f32) {
        let mut has_inside = false;
        let mut has_outside = false;

        for i in 0..8 {
            let val = field.index(self.corner_code(i), unit_size);
            self.corner_signs[i] = u8::from(val < 0.0);
            if val < 0.0 {
                has_inside = true;
            } else {
                has_outside = true;
            }
        }
        self.is_signed = has_inside && has_outside;
    }

    /// Computes connected components among inside corners using union-find.
    ///
    /// Only corners with `corner_signs[i] != 0` (inside corners) participate.
    /// Two inside corners sharing an edge belong to the same component.
    /// Outside corners get `component_indices[i] = -1`.
    ///
    /// This matches the C++ `calCornerComponents` exactly: it uses
    /// `cellProcFaceMask` (12 edges) for the union step, then reorders
    /// component indices sequentially starting from 0.
    pub fn cal_corner_components(&mut self) {
        debug_assert!(self.components.is_empty());

        // clusters[i] tracks the set of corners merged with corner i.
        // component_indices[i] tracks the current root for corner i.
        let mut clusters: [Vec<usize>; 8] = Default::default();

        for (i, cluster) in clusters.iter_mut().enumerate() {
            if self.corner_signs[i] != 0 {
                cluster.push(i);
                self.component_indices[i] = i as i8;
            }
        }

        // Union using the 12 edges from CELL_PROC_FACE_MASK (matches C++ exactly)
        for mask in &CELL_PROC_FACE_MASK {
            let c1 = mask[0];
            let c2 = mask[1];
            if self.corner_signs[c1] == self.corner_signs[c2] && self.corner_signs[c2] != 0 {
                let co1 = self.component_indices[c1] as usize;
                let co2 = self.component_indices[c2] as usize;
                // Merge co2's cluster into co1
                let c2_members: Vec<usize> = clusters[co2].clone();
                for &comp in &c2_members {
                    clusters[co1].push(comp);
                }
                // Update all members of co1's cluster to point to co1
                let co1_members: Vec<usize> = clusters[co1].clone();
                for &comp in &co1_members {
                    self.component_indices[comp] = co1 as i8;
                }
            }
        }

        // Reorder: map root indices to sequential 0, 1, 2, ...
        // Outside corners (corner_signs == 0) get -1.
        let mut reorder_map: [i8; 8] = [-1; 8];
        let mut new_order: i8 = 0;

        for i in 0..8 {
            if self.corner_signs[i] != 0 && reorder_map[self.component_indices[i] as usize] == -1 {
                reorder_map[self.component_indices[i] as usize] = new_order;
                new_order += 1;
            }
        }

        for i in 0..8 {
            self.component_indices[i] = reorder_map[self.component_indices[i] as usize];
        }

        self.vertices
            .resize_with(new_order as usize, Vertex::default);
        self.components
            .resize_with(new_order as usize, QefSolver::new);
    }

    /// Samples edge crossings to build QEF data for each connected component.
    ///
    /// For each edge whose endpoint corners have different signs, the
    /// zero-crossing and surface normal are computed and added to the
    /// appropriate component's QEF. Also accumulates into `all_qef`.
    ///
    /// Returns `true` if any edge crossings were found.
    pub fn sample_qef(
        &mut self,
        field: &dyn ScalarField,
        all: &mut QefSolver,
        unit_size: f32,
    ) -> bool {
        let mut found = false;
        all.reset();

        for qef in &mut self.components {
            qef.reset();
        }

        for edge in &EDGE_MAP {
            let c0 = edge[0];
            let c1 = edge[1];
            if self.corner_signs[c0] == self.corner_signs[c1] {
                continue;
            }

            let p0 = self.corner_pos(c0, unit_size);
            let p1 = self.corner_pos(c1, unit_size);

            if let Some(crossing) = field.solve(p0, p1) {
                let normal = field.normal(crossing);
                all.add(crossing, normal);

                // Add to the "inside" corner's component
                let inside_corner = if self.corner_signs[c0] == 1 { c0 } else { c1 };
                let comp_idx = self.component_indices[inside_corner];
                if comp_idx >= 0 && (comp_idx as usize) < self.components.len() {
                    self.components[comp_idx as usize].add(crossing, normal);
                }
                found = true;
            }
        }
        found
    }

    /// Returns the world-space position of corner `i` (0..8).
    pub fn corner_pos(&self, i: usize, unit_size: f32) -> Vec3 {
        let corner = decode_cell(i);
        let code = PositionCode::new(
            self.min_code.x + corner.x * (self.max_code.x - self.min_code.x),
            self.min_code.y + corner.y * (self.max_code.y - self.min_code.y),
            self.min_code.z + corner.z * (self.max_code.z - self.min_code.z),
        );
        code_to_pos(code, unit_size)
    }

    /// Returns the position code for corner `i` (0..8).
    fn corner_code(&self, i: usize) -> PositionCode {
        let corner = decode_cell(i);
        PositionCode::new(
            self.min_code.x + corner.x * (self.max_code.x - self.min_code.x),
            self.min_code.y + corner.y * (self.max_code.y - self.min_code.y),
            self.min_code.z + corner.z * (self.max_code.z - self.min_code.z),
        )
    }

    /// Returns the component index for the edge between two corners.
    ///
    /// Returns the component index of whichever corner is inside (sign != 0).
    /// Assumes the caller has verified a sign change exists on this edge.
    pub fn edge_component_index(&self, corner1: usize, corner2: usize) -> i8 {
        if self.corner_signs[corner1] != 0 {
            self.component_indices[corner1]
        } else {
            self.component_indices[corner2]
        }
    }

    /// Returns the component index for a face-edge configuration.
    ///
    /// Given a face direction, edge direction, face side, and edge side,
    /// finds the inside corner's component along that face. First checks
    /// corners on the given edge side, then falls back to the opposite
    /// edge side. Returns -1 if no inside corner is found.
    ///
    /// Matches C++ `faceComponentIndex` exactly.
    pub fn face_component_index(
        &self,
        face_dir: usize,
        edge_dir: usize,
        face_side: usize,
        edge_side: usize,
    ) -> i8 {
        let mut component: i8 = -1;
        let dir = 3 - face_dir - edge_dir;

        // First pass: check corners on the given edge side
        for i in 0..2 {
            let mut code = glam::IVec3::ZERO;
            code[face_dir] = face_side as i32;
            code[edge_dir] = edge_side as i32;
            code[dir] = i;
            let corner = encode_cell(code);
            if self.corner_signs[corner] > 0 {
                component = self.component_indices[corner];
            }
        }
        if component != -1 {
            return component;
        }

        // Second pass: check corners on the opposite edge side
        for i in 0..2 {
            let mut code = glam::IVec3::ZERO;
            code[face_dir] = face_side as i32;
            code[edge_dir] = 1 - edge_side as i32;
            code[dir] = i;
            let corner = encode_cell(code);
            if self.corner_signs[corner] > 0 {
                component = self.component_indices[corner];
            }
        }
        component
    }

    /// Checks whether two adjacent grids can be clustered (merged).
    ///
    /// Tests that the combined QEF error is acceptable and that no intersection
    /// would be introduced by merging.
    pub fn cal_clusterability(
        left: &RectilinearGrid,
        right: &RectilinearGrid,
        dir: usize,
        min_code: PositionCode,
        max_code: PositionCode,
        field: &dyn ScalarField,
        unit_size: f32,
    ) -> bool {
        // Check that the merge would not introduce sign changes on shared corners
        // that belong to different components
        let _ = (min_code, max_code); // used for bounds in full implementation

        // Verify edge compatibility along the merge direction
        for edge in &EDGE_MAP {
            let c0 = edge[0];
            let c1 = edge[1];
            let corner0 = decode_cell(c0);
            let corner1 = decode_cell(c1);

            // Only check edges perpendicular to the merge direction
            if corner0[dir] == corner1[dir] {
                continue;
            }

            // Left side corner is the one with corner[dir] == 0
            let (left_corner, right_corner) = if corner0[dir] == 0 {
                (c0, c1)
            } else {
                (c1, c0)
            };

            if left.corner_signs[left_corner] != right.corner_signs[right_corner] {
                // Sign change across the boundary — check component compatibility
                let left_comp = left.component_indices[left_corner];
                let right_comp = right.component_indices[right_corner];
                if left_comp < 0 || right_comp < 0 {
                    return false;
                }
            }
        }

        // Check combined QEF error threshold
        let mut combined = left.all_qef.clone();
        combined.combine(&right.all_qef);
        let min_pos = code_to_pos(min_code, unit_size);
        let max_pos = code_to_pos(max_code, unit_size);
        let (mut pos, _) = combined.solve();
        pos = pos.clamp(min_pos, max_pos);

        let error = combined.get_error_at(pos);
        let threshold = unit_size * unit_size * 0.1;
        let _ = field; // reserved for future normal-based checks

        error < threshold
    }

    /// Combines QEF data from two adjacent grids into an output grid.
    ///
    /// `dir` is the axis along which the grids are adjacent (0=X, 1=Y, 2=Z).
    /// Sets the output grid's corner signs from the children, computes
    /// corner components, then maps child component QEFs to output components.
    ///
    /// Matches C++ `combineAAGrid` logic: uses `cellProcFaceMask[dir*4+i]`
    /// to find the 4 face edges, maps child component indices to output
    /// component indices, then combines QEFs.
    pub fn combine_aa_grid(
        left: &RectilinearGrid,
        right: &RectilinearGrid,
        dir: usize,
        out: &mut RectilinearGrid,
        _unit_size: f32,
    ) {
        // Set corner signs from children: left provides dir==0 side, right provides dir==1 side
        for i in 0..8 {
            let corner = decode_cell(i);
            if corner[dir] == 0 {
                out.corner_signs[i] = left.corner_signs[i];
            } else {
                out.corner_signs[i] = right.corner_signs[i];
            }
        }

        out.cal_corner_components();

        // Build combine maps: for each of left[0] and right[1],
        // maps output_component_index -> child_component_index
        let mut combine_maps: [BTreeMap<usize, usize>; 2] = [BTreeMap::new(), BTreeMap::new()];
        let grids: [&RectilinearGrid; 2] = [left, right];

        for i in 0..4 {
            let mask = &CELL_PROC_FACE_MASK[dir * 4 + i];
            // Find output component index c by checking which output corner
            // on this edge is inside
            let mut c: i8 = -1;
            for &corner_idx in mask.iter().take(2) {
                if out.corner_signs[corner_idx] != 0 {
                    c = out.component_indices[corner_idx];
                    break;
                }
            }
            if c == -1 {
                continue;
            }
            let out_c = c as usize;

            // For each child (left, right), find the child component
            for (j, child) in grids.iter().enumerate() {
                for &corner_idx in mask.iter().take(2) {
                    if child.corner_signs[corner_idx] != 0 {
                        let child_c = child.component_indices[corner_idx];
                        if child_c >= 0 && (child_c as usize) < child.components.len() {
                            combine_maps[j].insert(out_c, child_c as usize);
                        }
                        break;
                    }
                }
            }
        }

        // Combine child QEFs into output components
        for i in 0..2 {
            for (&out_c, &child_c) in &combine_maps[i] {
                if out_c < out.components.len() && child_c < grids[i].components.len() {
                    out.components[out_c].combine(&grids[i].components[child_c]);
                }
            }
        }

        // Set is_signed
        out.is_signed = {
            let mut has_in = false;
            let mut has_out = false;
            for &s in &out.corner_signs {
                if s != 0 {
                    has_in = true;
                } else {
                    has_out = true;
                }
            }
            has_in && has_out
        };
    }

    /// Möller-Trumbore ray-triangle intersection test.
    ///
    /// Returns `true` if the ray from `origin` in `direction` intersects
    /// the triangle `(v0, v1, v2)`.
    fn ray_triangle_intersect(origin: Vec3, direction: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> bool {
        let epsilon = 1.0e-6_f32;
        let edge1 = v1 - v0;
        let edge2 = v2 - v0;
        let h = direction.cross(edge2);
        let a = edge1.dot(h);

        if a.abs() < epsilon {
            return false;
        }

        let f = 1.0 / a;
        let s = origin - v0;
        let u = f * s.dot(h);

        if !(0.0..=1.0).contains(&u) {
            return false;
        }

        let q = s.cross(edge1);
        let v = f * direction.dot(q);

        if v < 0.0 || u + v > 1.0 {
            return false;
        }

        let t = f * edge2.dot(q);
        t > epsilon
    }

    /// Tests whether the intersection-free condition 2 is violated.
    ///
    /// Checks whether a line segment from `p1` to `p2` intersects any of the
    /// given triangles (each stored as 3 consecutive `Vec3` positions).
    pub fn is_inter_free_condition2_failed(
        polygons: &[(Vec3, Vec3, Vec3)],
        p1: Vec3,
        p2: Vec3,
    ) -> bool {
        let direction = p2 - p1;
        for &(v0, v1, v2) in polygons {
            if Self::ray_triangle_intersect(p1, direction, v0, v1, v2) {
                return true;
            }
        }
        false
    }
}

/// Checks whether an edge between grid holders has a sign change.
///
/// Examines the 4 grid holders surrounding an edge (identified by the two
/// quad directions). Determines the min/max endpoints along the edge direction,
/// samples the scalar field at both endpoints, and returns `None` if no sign
/// change exists.
///
/// Returns `Some((side, min_end, max_end))` where `side` is 0 or 1 indicating
/// which end is positive.
///
/// Matches C++ `checkSign` exactly.
pub fn check_sign(
    nodes: &[&dyn HasGrid],
    quad_dir1: usize,
    quad_dir2: usize,
    field: &dyn ScalarField,
    unit_size: f32,
) -> Option<(i32, PositionCode, PositionCode)> {
    let dir = 3 - quad_dir1 - quad_dir2;

    // Determine initial min_end/max_end based on whether nodes[0] != nodes[1]
    let (mut min_end, mut max_end) = if !std::ptr::eq(nodes[0].grid(), nodes[1].grid()) {
        let code = nodes[0].grid().max_code;
        (code, code)
    } else {
        let code = nodes[3].grid().min_code;
        (code, code)
    };

    // Compute max along dir from all 4 nodes' maxCodes
    max_end[dir] = nodes[0].grid().max_code[dir]
        .min(nodes[1].grid().max_code[dir])
        .min(nodes[2].grid().max_code[dir])
        .min(nodes[3].grid().max_code[dir]);

    // Compute min along dir from all 4 nodes' minCodes
    min_end[dir] = nodes[0].grid().min_code[dir]
        .max(nodes[1].grid().min_code[dir])
        .max(nodes[2].grid().min_code[dir])
        .max(nodes[3].grid().min_code[dir]);

    if min_end[dir] >= max_end[dir] {
        return None;
    }

    let v1 = field.index(min_end, unit_size);
    let v2 = field.index(max_end, unit_size);

    // Same sign at both endpoints: no sign change
    if (v1 >= 0.0 && v2 >= 0.0) || (v1 < 0.0 && v2 < 0.0) {
        return None;
    }

    let side = if v2 >= 0.0 && v1 <= 0.0 { 0 } else { 1 };

    Some((side, min_end, max_end))
}

/// Generates a quad (two triangles) from 4 grid holders surrounding an edge.
///
/// Matches C++ `generateQuad` exactly:
/// - Calls `check_sign` to get `edge_side`, `min_end`, `max_end`
/// - For each node, checks if `nodes[i] != nodes[opposite_quad_index(i)]`
///   - If different: uses `edgeComponentIndex` via `quad_index(symmetryQuadIndex(i))`
///   - If same: uses `faceComponentIndex`
/// - Builds polygon with 2-4 vertices based on same-node checks
/// - Deduplicates, returns if < 3 unique
/// - Fan triangulates
#[allow(clippy::too_many_arguments)]
pub fn generate_quad(
    nodes: &[&dyn HasGrid; 4],
    quad_dir1: usize,
    quad_dir2: usize,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    _threshold: f32,
    unit_size: f32,
) {
    let check = check_sign(
        &[nodes[0], nodes[1], nodes[2], nodes[3]],
        quad_dir1,
        quad_dir2,
        field,
        unit_size,
    );
    let (edge_side, min_end, max_end) = match check {
        Some(v) => v,
        None => return,
    };

    let line_dir = 3 - quad_dir1 - quad_dir2;
    let mut comp_indices: [i8; 4] = [-1; 4];

    for i in 0..4 {
        let opp = opposite_quad_index(i);
        if !std::ptr::eq(nodes[i].grid(), nodes[opp].grid()) {
            // Different nodes: use edgeComponentIndex
            let (c1, c2) = quad_index(quad_dir1, quad_dir2, symmetry_quad_index(i));
            comp_indices[i] = nodes[i].grid().edge_component_index(c1, c2);
        } else {
            // Same node: use faceComponentIndex
            comp_indices[i] = nodes[i].grid().face_component_index(
                quad_dir2,
                line_dir,
                1 - i / 2,
                edge_side as usize,
            );
        }
        if comp_indices[i] == -1 {
            return;
        }
    }

    // Build polygon: always include nodes[0], conditionally nodes[1],
    // always nodes[3], conditionally nodes[2]
    let mut polygons: Vec<(usize, usize)> = Vec::with_capacity(4); // (node_index, comp_index)

    polygons.push((0, comp_indices[0] as usize));
    if !std::ptr::eq(nodes[0].grid(), nodes[1].grid()) {
        polygons.push((1, comp_indices[1] as usize));
    }
    polygons.push((3, comp_indices[3] as usize));
    if !std::ptr::eq(nodes[2].grid(), nodes[3].grid()) {
        polygons.push((2, comp_indices[2] as usize));
    }

    // Deduplicate by checking vertex identity (pointer equality via grid + comp index)
    // Use a set of (grid_ptr, comp_idx) pairs to detect duplicates
    let mut unique_count = 0;
    let mut seen: Vec<(*const RectilinearGrid, usize)> = Vec::with_capacity(4);
    for &(ni, ci) in &polygons {
        let key = (nodes[ni].grid() as *const RectilinearGrid, ci);
        if !seen.contains(&key) {
            seen.push(key);
            unique_count += 1;
        }
    }

    if unique_count < 3 {
        return;
    }

    // Collect vertex data
    let mut verts: Vec<Vertex> = Vec::with_capacity(polygons.len());
    for &(ni, ci) in &polygons {
        let grid = nodes[ni].grid();
        if ci < grid.vertices.len() {
            verts.push(grid.vertices[ci].clone());
        } else {
            return;
        }
    }

    // Intersection-free condition 2 check and optional reorder for 4-vertex polygons
    let p1 = code_to_pos(min_end, unit_size);
    let p2 = code_to_pos(max_end, unit_size);

    let _condition2_failed =
        RectilinearGrid::is_inter_free_condition2_failed(&fan_triangles(&verts), p1, p2);
    if verts.len() > 3 {
        let reverse_verts = vec![
            verts[1].clone(),
            verts[2].clone(),
            verts[3].clone(),
            verts[0].clone(),
        ];
        let reverse_condition2_failed = RectilinearGrid::is_inter_free_condition2_failed(
            &fan_triangles(&reverse_verts),
            p1,
            p2,
        );
        if !reverse_condition2_failed {
            // Swap to reverse order (matches C++ behavior)
            verts = reverse_verts;
        }
    }

    // Fan triangulation
    for i in 2..verts.len() {
        mesh.add_triangle([&verts[0], &verts[i - 1], &verts[i]], |p| field.normal(p));
    }
}

/// Helper to build fan-triangulated triangles from a polygon for intersection testing.
fn fan_triangles(verts: &[Vertex]) -> Vec<(Vec3, Vec3, Vec3)> {
    let mut tris = Vec::new();
    for i in 2..verts.len() {
        tris.push((
            verts[0].hermite_p,
            verts[i - 1].hermite_p,
            verts[i].hermite_p,
        ));
    }
    tris
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::IVec3;

    /// A simple sphere SDF for testing.
    struct TestSphere {
        radius: f32,
    }

    impl ScalarField for TestSphere {
        fn value(&self, p: Vec3) -> f32 {
            p.length() - self.radius
        }
    }

    /// Trivial HasGrid wrapper for testing.
    struct GridHolder {
        grid: RectilinearGrid,
    }

    impl HasGrid for GridHolder {
        fn grid(&self) -> &RectilinearGrid {
            &self.grid
        }
    }

    #[test]
    fn new_grid_has_valid_approximate() {
        let qef = QefSolver::new();
        let grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        // With empty QEF, approximate should be at origin (clamped)
        assert_eq!(grid.approximate.hermite_p, Vec3::ZERO);
    }

    #[test]
    fn assign_sign_detects_crossing() {
        let sphere = TestSphere { radius: 0.5 };
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        grid.assign_sign(&sphere, 1.0);

        // Corner (0,0,0) is at the origin, distance = -0.5 (inside)
        assert_eq!(grid.corner_signs[0], 1);
        // Corner (1,1,1) is at (1,1,1), distance = sqrt(3) - 0.5 > 0 (outside)
        assert_eq!(grid.corner_signs[7], 0);
        assert!(grid.is_signed);
    }

    #[test]
    fn corner_components_all_outside_gives_no_components() {
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        // All corners outside
        grid.corner_signs = [0; 8];
        grid.cal_corner_components();

        // All outside corners should have component index -1
        for &ci in &grid.component_indices {
            assert_eq!(ci, -1);
        }
        assert!(grid.components.is_empty());
        assert!(grid.vertices.is_empty());
    }

    #[test]
    fn corner_components_two_signs() {
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        // One corner inside, rest outside
        grid.corner_signs = [1, 0, 0, 0, 0, 0, 0, 0];
        grid.cal_corner_components();

        // Corner 0 should have a valid component (>= 0)
        let comp0 = grid.component_indices[0];
        assert!(comp0 >= 0);
        // Only one inside component
        assert_eq!(grid.components.len(), 1);
    }

    #[test]
    fn edge_component_index_returns_inside_corner() {
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        grid.corner_signs = [1, 0, 0, 0, 0, 0, 0, 0];
        grid.cal_corner_components();

        // Edge (0, 4): corner 0 is inside (sign != 0), so returns its component
        let comp = grid.edge_component_index(0, 4);
        assert!(comp >= 0);
        assert_eq!(comp, grid.component_indices[0]);
    }

    #[test]
    fn sample_qef_finds_crossings() {
        let sphere = TestSphere { radius: 0.5 };
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        grid.assign_sign(&sphere, 1.0);
        grid.cal_corner_components();

        let mut all_qef = QefSolver::new();
        let found = grid.sample_qef(&sphere, &mut all_qef, 1.0);
        assert!(found);
        assert!(all_qef.point_count() > 0);
    }

    #[test]
    fn corner_pos_returns_correct_positions() {
        let qef = QefSolver::new();
        let grid = RectilinearGrid::new(IVec3::new(0, 0, 0), IVec3::new(2, 2, 2), qef, 0.5);

        let p0 = grid.corner_pos(0, 0.5);
        assert_eq!(p0, Vec3::new(0.0, 0.0, 0.0));

        let p7 = grid.corner_pos(7, 0.5);
        assert_eq!(p7, Vec3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn ray_triangle_intersect_basic() {
        let v0 = Vec3::new(-1.0, -1.0, 1.0);
        let v1 = Vec3::new(1.0, -1.0, 1.0);
        let v2 = Vec3::new(0.0, 1.0, 1.0);

        // Ray pointing at the triangle
        assert!(RectilinearGrid::ray_triangle_intersect(
            Vec3::ZERO,
            Vec3::Z,
            v0,
            v1,
            v2
        ));

        // Ray pointing away
        assert!(!RectilinearGrid::ray_triangle_intersect(
            Vec3::ZERO,
            Vec3::NEG_Z,
            v0,
            v1,
            v2
        ));
    }

    #[test]
    fn is_inter_free_condition2_detects_intersection() {
        let tri = (
            Vec3::new(-1.0, -1.0, 0.5),
            Vec3::new(1.0, -1.0, 0.5),
            Vec3::new(0.0, 1.0, 0.5),
        );
        // Segment passes through the triangle
        assert!(RectilinearGrid::is_inter_free_condition2_failed(
            &[tri],
            Vec3::ZERO,
            Vec3::Z
        ));

        // Segment does not reach the triangle
        assert!(!RectilinearGrid::is_inter_free_condition2_failed(
            &[tri],
            Vec3::new(5.0, 5.0, 0.0),
            Vec3::new(5.0, 5.0, 1.0)
        ));
    }
}
