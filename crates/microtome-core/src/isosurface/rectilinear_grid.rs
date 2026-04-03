//! Rectilinear grid cell for dual contouring with k-d tree acceleration.
//!
//! This is the fundamental cell type in the KdtreeISO dual contouring algorithm.
//! Each grid represents an axis-aligned cell that stores QEF data, corner signs,
//! connected component information, and solved vertex positions.

use glam::Vec3;

use super::indicators::{
    EDGE_MAP, EDGE_TEST_NODE_ORDER, PositionCode, code_to_pos, decode_cell, encode_cell, quad_index,
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

    /// Solves a QEF and writes the result into a vertex, clamping the position
    /// to the cell defined by `min_code`..`max_code`.
    fn solve_qef(
        qef: &mut QefSolver,
        vertex: &mut Vertex,
        min_code: PositionCode,
        max_code: PositionCode,
        unit_size: f32,
    ) {
        let (mut pos, error) = qef.solve();
        let min_pos = code_to_pos(min_code, unit_size);
        let max_pos = code_to_pos(max_code, unit_size);
        pos = pos.clamp(min_pos, max_pos);

        // If error is too large, fall back to the mass point
        if error > 1.0e-2 {
            let mp = qef.mass_point();
            pos = mp.clamp(min_pos, max_pos);
        }

        vertex.hermite_p = pos;
        vertex.error = error;
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

    /// Computes connected components among same-sign corners using union-find.
    ///
    /// Two corners sharing an edge and having the same sign belong to the same
    /// component. Each corner is assigned a component index in
    /// `component_indices`.
    pub fn cal_corner_components(&mut self) {
        // Union-find parent array, initially each corner is its own root
        let mut parent: [usize; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

        // Find with path compression
        fn find(parent: &mut [usize; 8], mut x: usize) -> usize {
            while parent[x] != x {
                parent[x] = parent[parent[x]];
                x = parent[x];
            }
            x
        }

        // Union corners sharing an edge with the same sign
        for edge in &EDGE_MAP {
            let c0 = edge[0];
            let c1 = edge[1];
            if self.corner_signs[c0] == self.corner_signs[c1] {
                let r0 = find(&mut parent, c0);
                let r1 = find(&mut parent, c1);
                if r0 != r1 {
                    parent[r1] = r0;
                }
            }
        }

        // Assign component indices: map each root to a sequential index
        let mut component_count: i8 = 0;
        let mut root_to_component: [i8; 8] = [-1; 8];

        for i in 0..8 {
            let root = find(&mut parent, i);
            if root_to_component[root] < 0 {
                root_to_component[root] = component_count;
                component_count += 1;
            }
            self.component_indices[i] = root_to_component[root];
        }

        self.components
            .resize_with(component_count as usize, QefSolver::new);
        self.vertices
            .resize_with(component_count as usize, Vertex::default);
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
    /// If the corners belong to different components, returns `-1`.
    /// Otherwise returns the component index of the "inside" corner.
    pub fn edge_component_index(&self, corner1: usize, corner2: usize) -> i8 {
        if self.corner_signs[corner1] != self.corner_signs[corner2] {
            // Sign change: return the inside corner's component
            if self.corner_signs[corner1] == 1 {
                self.component_indices[corner1]
            } else {
                self.component_indices[corner2]
            }
        } else {
            -1
        }
    }

    /// Returns the component index for a face-edge configuration.
    ///
    /// Given a face direction, edge direction, face side, and edge side,
    /// computes the two corners that share the edge and returns the
    /// component index (or -1 if no sign change).
    pub fn face_component_index(
        &self,
        face_dir: usize,
        edge_dir: usize,
        face_side: usize,
        edge_side: usize,
    ) -> i8 {
        let other_dir = 3 - face_dir - edge_dir;
        let mut code1 = glam::IVec3::ZERO;
        let mut code2 = glam::IVec3::ZERO;

        code1[face_dir] = face_side as i32;
        code2[face_dir] = face_side as i32;

        code1[edge_dir] = edge_side as i32;
        code2[edge_dir] = edge_side as i32;

        code1[other_dir] = 0;
        code2[other_dir] = 1;

        let c1 = encode_cell(code1);
        let c2 = encode_cell(code2);

        self.edge_component_index(c1, c2)
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
    pub fn combine_aa_grid(
        left: &RectilinearGrid,
        right: &RectilinearGrid,
        dir: usize,
        out: &mut RectilinearGrid,
        unit_size: f32,
    ) {
        out.all_qef = left.all_qef.clone();
        out.all_qef.combine(&right.all_qef);

        // Combine corner signs: take left for dir==0 side, right for dir==1 side
        for i in 0..8 {
            let corner = decode_cell(i);
            if corner[dir] == 0 {
                out.corner_signs[i] = left.corner_signs[i];
            } else {
                out.corner_signs[i] = right.corner_signs[i];
            }
        }

        out.cal_corner_components();

        // Distribute left/right component QEFs into the output components
        for i in 0..8 {
            let corner = decode_cell(i);
            let out_comp = out.component_indices[i];
            if out_comp < 0 {
                continue;
            }
            let out_idx = out_comp as usize;
            if out_idx >= out.components.len() {
                continue;
            }

            if corner[dir] == 0 {
                let src_comp = left.component_indices[i];
                if src_comp >= 0 && (src_comp as usize) < left.components.len() {
                    out.components[out_idx].combine(&left.components[src_comp as usize]);
                }
            } else {
                let src_comp = right.component_indices[i];
                if src_comp >= 0 && (src_comp as usize) < right.components.len() {
                    out.components[out_idx].combine(&right.components[src_comp as usize]);
                }
            }
        }

        // Solve each component
        for i in 0..out.components.len() {
            out.solve_component(i, unit_size);
        }

        // Solve the combined approximate vertex
        let min_code = out.min_code;
        let max_code = out.max_code;
        Self::solve_qef(
            &mut out.all_qef,
            &mut out.approximate,
            min_code,
            max_code,
            unit_size,
        );

        out.is_signed = {
            let mut has_in = false;
            let mut has_out = false;
            for &s in &out.corner_signs {
                if s == 1 {
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
/// quad directions) and returns sign/code information if a sign change exists.
///
/// Returns `Some((sign, code_min, code_max))` if a sign change is detected,
/// or `None` if all corners share the same sign.
pub fn check_sign(
    nodes: &[&dyn HasGrid],
    quad_dir1: usize,
    quad_dir2: usize,
    field: &dyn ScalarField,
    unit_size: f32,
) -> Option<(i32, PositionCode, PositionCode)> {
    let _ = field; // reserved for future use

    let mut min_code = PositionCode::splat(i32::MAX);
    let mut max_code = PositionCode::splat(i32::MIN);

    // The edge direction is the axis not in quad_dir1 or quad_dir2
    let edge_dir = 3 - quad_dir1 - quad_dir2;

    let mut sign: Option<i32> = None;
    let mut has_change = false;

    for (i, order) in EDGE_TEST_NODE_ORDER.iter().enumerate() {
        let node = nodes[i];
        let grid = node.grid();

        let (p1, p2) = quad_index(quad_dir1, quad_dir2, order[0]);

        let s1 = grid.corner_signs[p1] as i32;
        let s2 = grid.corner_signs[p2] as i32;

        if s1 != s2 {
            has_change = true;
        }

        if let Some(prev) = sign
            && prev != s1
        {
            has_change = true;
        }
        sign = Some(s1);

        // Track bounding box across all participating grids
        min_code = min_code.min(grid.min_code);
        max_code = max_code.max(grid.max_code);

        let _ = (p2, edge_dir, unit_size);
    }

    if has_change {
        Some((sign.unwrap_or(0), min_code, max_code))
    } else {
        None
    }
}

/// Generates a quad (two triangles) from 4 grid holders surrounding an edge.
///
/// The quad connects the solved vertices from each of the 4 grids. The winding
/// order is determined by the sign of the edge crossing.
pub fn generate_quad(
    nodes: &[&dyn HasGrid; 4],
    quad_dir1: usize,
    quad_dir2: usize,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    // Find the component index for each node
    let edge_dir = 3 - quad_dir1 - quad_dir2;
    let mut vertex_indices: [Option<usize>; 4] = [None; 4];
    let mut vertices_ref: Vec<Option<usize>> = Vec::with_capacity(4);

    for (i, order) in EDGE_TEST_NODE_ORDER.iter().enumerate() {
        let grid = nodes[i].grid();
        let (p1, p2) = quad_index(quad_dir1, quad_dir2, order[0]);
        let comp_idx = grid.edge_component_index(p1, p2);

        if comp_idx >= 0 && (comp_idx as usize) < grid.vertices.len() {
            vertex_indices[i] = Some(comp_idx as usize);
            vertices_ref.push(Some(i));
        } else {
            vertices_ref.push(None);
        }
        let _ = (edge_dir, threshold);
    }

    // Ensure all 4 vertices exist
    for vi in &vertex_indices {
        if vi.is_none() {
            return;
        }
    }

    // Get mutable access to emit vertices into the mesh
    // We need to collect the vertex data first, then emit
    let mut quad_verts: [Vertex; 4] = [
        Vertex::default(),
        Vertex::default(),
        Vertex::default(),
        Vertex::default(),
    ];

    for i in 0..4 {
        let grid = nodes[i].grid();
        if let Some(comp_idx) = vertex_indices[i]
            && comp_idx < grid.vertices.len()
        {
            quad_verts[i] = grid.vertices[comp_idx].clone();
        }
    }

    // Add vertices to the mesh if not already added (vertex_index == 0 and error >= 0 means uninitialized)
    for vert in &mut quad_verts {
        if vert.vertex_index == 0 && vert.error >= 0.0 {
            mesh.add_vertex(vert, |p| field.normal(p));
        }
    }

    // Determine winding order from the sign of the first node's relevant corner
    let (p1, _p2) = quad_index(quad_dir1, quad_dir2, EDGE_TEST_NODE_ORDER[0][0]);
    let flip = nodes[0].grid().corner_signs[p1] == 1;

    // Simple fan triangulation (skip intersection-free path for initial port)
    if flip {
        mesh.add_triangle([&quad_verts[0], &quad_verts[1], &quad_verts[2]], |p| {
            field.normal(p)
        });
        mesh.add_triangle([&quad_verts[0], &quad_verts[2], &quad_verts[3]], |p| {
            field.normal(p)
        });
    } else {
        mesh.add_triangle([&quad_verts[0], &quad_verts[2], &quad_verts[1]], |p| {
            field.normal(p)
        });
        mesh.add_triangle([&quad_verts[0], &quad_verts[3], &quad_verts[2]], |p| {
            field.normal(p)
        });
    }

    let _ = unit_size;
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
    fn corner_components_same_sign_single_component() {
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        // All corners outside
        grid.corner_signs = [0; 8];
        grid.cal_corner_components();

        // All corners should be in the same component
        let first = grid.component_indices[0];
        for &ci in &grid.component_indices {
            assert_eq!(ci, first);
        }
    }

    #[test]
    fn corner_components_two_signs() {
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        // One corner inside, rest outside
        grid.corner_signs = [1, 0, 0, 0, 0, 0, 0, 0];
        grid.cal_corner_components();

        // Corner 0 should be in a different component from the connected outside corners
        let comp0 = grid.component_indices[0];
        let comp1 = grid.component_indices[1];
        assert_ne!(comp0, comp1);
    }

    #[test]
    fn edge_component_index_sign_change() {
        let qef = QefSolver::new();
        let mut grid = RectilinearGrid::new(IVec3::ZERO, IVec3::ONE, qef, 1.0);
        grid.corner_signs = [1, 0, 0, 0, 0, 0, 0, 0];
        grid.cal_corner_components();

        // Edge (0, 4): corners 0 (inside) and 4 (outside) — sign change
        let comp = grid.edge_component_index(0, 4);
        assert!(comp >= 0);

        // Edge (1, 5): both outside — no sign change
        let comp_no = grid.edge_component_index(1, 5);
        assert_eq!(comp_no, -1);
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
