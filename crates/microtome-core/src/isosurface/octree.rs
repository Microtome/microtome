//! Octree-based dual contouring for isosurface extraction.
//!
//! Builds an adaptive octree from a scalar field, optionally simplifies it
//! by collapsing clusterable nodes, and extracts a triangle mesh via dual
//! contouring (cell, face, and edge procedures).

use super::indicators::{
    CELL_PROC_EDGE_MASK, CELL_PROC_FACE_MASK, FACE_PROC_EDGE_MASK, FACE_PROC_FACE_MASK,
    PositionCode, encode_cell, min_offset_subdivision,
};
use super::mesh_output::IsoMesh;
use super::qef::QefSolver;
use super::rectilinear_grid::{HasGrid, RectilinearGrid, check_sign, generate_quad};
use super::scalar_field::ScalarField;

/// A node in the adaptive octree used for dual contouring.
///
/// Each node represents an axis-aligned cube in the voxel grid.  Leaf nodes
/// store QEF data and solved vertex positions via their embedded
/// [`RectilinearGrid`].  Internal nodes have up to 8 children.
#[derive(Debug, Clone)]
pub struct OctreeNode {
    /// The rectilinear grid cell for this node (signs, QEF, vertices).
    pub grid: RectilinearGrid,
    /// Up to 8 children (one per octant). `None` means the octant is empty.
    pub children: [Option<Box<OctreeNode>>; 8],
    /// Index of this node within its parent's children array (0..7), or -1
    /// if this is the root.
    pub child_index: i8,
    /// Whether this is a leaf node (no children or collapsed).
    pub is_leaf: bool,
    /// Whether all children can be merged without introducing artifacts.
    pub clusterable: bool,
    /// Depth in the octree (root has the largest depth; leaves have depth 1).
    pub depth: u32,
}

impl HasGrid for OctreeNode {
    fn grid(&self) -> &RectilinearGrid {
        &self.grid
    }
}

impl OctreeNode {
    /// Solves the approximate vertex using the grid's solve_qef.
    fn solve_approximate(grid: &mut RectilinearGrid, unit_size: f32) {
        let min_c = grid.min_code;
        let max_c = grid.max_code;
        let mut qef = grid.all_qef.clone();
        RectilinearGrid::solve_qef_pub(&mut qef, &mut grid.approximate, min_c, max_c, unit_size);
        grid.all_qef = qef;
    }

    /// Recursively builds an octree from a scalar field.
    ///
    /// `min_code` is the minimum corner of this cell in voxel coordinates.
    /// `depth` is the remaining subdivision depth (1 = leaf level).
    /// When `as_mipmap` is true the tree is kept fully expanded (no
    /// component combining at internal nodes).
    /// `unit_size` is the world-space size of one voxel unit.
    ///
    /// Returns `None` if the cell is entirely inside or outside the surface.
    pub fn build_with_scalar_field(
        min_code: PositionCode,
        depth: u32,
        field: &dyn ScalarField,
        as_mipmap: bool,
        unit_size: f32,
    ) -> Option<Box<OctreeNode>> {
        let size = 1_i32 << depth;
        let max_code = min_code + PositionCode::splat(size);

        let qef = QefSolver::new();
        let grid = RectilinearGrid::new(min_code, max_code, qef, unit_size);

        let mut node = Box::new(OctreeNode {
            grid,
            children: Default::default(),
            child_index: -1,
            is_leaf: false,
            clusterable: false,
            depth,
        });

        if depth == 1 {
            // Leaf level
            node.grid.assign_sign(field, unit_size);
            if !node.grid.is_signed {
                return None;
            }
            node.grid.cal_corner_components();
            let mut all_qef = QefSolver::new();
            node.grid.sample_qef(field, &mut all_qef, unit_size);
            node.grid.all_qef = all_qef;

            for i in 0..node.grid.components.len() {
                node.grid.solve_component(i, unit_size);
            }
            Self::solve_approximate(&mut node.grid, unit_size);

            node.is_leaf = true;
            node.clusterable = true;
            return Some(node);
        }

        // Internal node: recursively build 8 children
        let half = size / 2;
        let mut any_child = false;
        for i in 0..8 {
            let offset = min_offset_subdivision(i);
            let child_min = min_code
                + PositionCode::new(
                    (offset.x as i32) * half,
                    (offset.y as i32) * half,
                    (offset.z as i32) * half,
                );
            let child =
                Self::build_with_scalar_field(child_min, depth - 1, field, as_mipmap, unit_size);
            if let Some(mut c) = child {
                c.child_index = i as i8;
                node.children[i] = Some(c);
                any_child = true;
            }
        }

        if !any_child {
            return None;
        }

        // Accumulate children's QEFs into this node
        node.grid.all_qef.reset();
        for c in node.children.iter().flatten() {
            node.grid.all_qef.combine(&c.grid.all_qef);
        }

        // Calculate clusterability
        Self::cal_clusterability(&mut node, field, unit_size);

        // If clusterable and not building a mipmap, combine components
        if node.clusterable && !as_mipmap {
            node.combine_components(unit_size);
        }

        // Solve all components
        for i in 0..node.grid.components.len() {
            node.grid.solve_component(i, unit_size);
        }

        Self::solve_approximate(&mut node.grid, unit_size);

        Some(node)
    }

    /// Accumulates QEF data from the octree within the given bounding box.
    ///
    /// Walks the tree and returns the combined QEF for all leaves whose
    /// cells overlap `[min_pos, max_pos]`.
    pub fn get_sum(
        root: &OctreeNode,
        min_pos: glam::Vec3,
        max_pos: glam::Vec3,
        unit_size: f32,
    ) -> QefSolver {
        let mut result = QefSolver::new();
        Self::get_sum_recursive(root, min_pos, max_pos, unit_size, &mut result);
        result
    }

    fn get_sum_recursive(
        node: &OctreeNode,
        min_pos: glam::Vec3,
        max_pos: glam::Vec3,
        unit_size: f32,
        out: &mut QefSolver,
    ) {
        let node_min = super::indicators::code_to_pos(node.grid.min_code, unit_size);
        let node_max = super::indicators::code_to_pos(node.grid.max_code, unit_size);

        // No overlap
        if node_max.x <= min_pos.x
            || node_max.y <= min_pos.y
            || node_max.z <= min_pos.z
            || node_min.x >= max_pos.x
            || node_min.y >= max_pos.y
            || node_min.z >= max_pos.z
        {
            return;
        }

        // Fully contained
        if node_min.x >= min_pos.x
            && node_min.y >= min_pos.y
            && node_min.z >= min_pos.z
            && node_max.x <= max_pos.x
            && node_max.y <= max_pos.y
            && node_max.z <= max_pos.z
        {
            out.combine(&node.grid.all_qef);
            return;
        }

        if node.is_leaf {
            // Partial overlap at leaf: include anyway
            out.combine(&node.grid.all_qef);
            return;
        }

        for c in node.children.iter().flatten() {
            Self::get_sum_recursive(c, min_pos, max_pos, unit_size, out);
        }
    }

    /// Simplifies the octree by collapsing clusterable nodes whose error is
    /// below `threshold`.
    ///
    /// After simplification, collapsed internal nodes become leaves.
    pub fn simplify(root: &mut OctreeNode, threshold: f32) {
        if root.is_leaf {
            return;
        }

        for c in root.children.iter_mut().flatten() {
            Self::simplify(c, threshold);
        }

        if root.clusterable && root.grid.approximate.error <= threshold {
            // Collapse: remove all children, become a leaf
            for child in &mut root.children {
                *child = None;
            }
            root.is_leaf = true;
        }
    }

    /// Checks whether this node's children can be clustered (merged).
    ///
    /// Sets `clusterable` on the node based on whether all children are
    /// themselves clusterable and the face-pair compatibility checks pass.
    fn cal_clusterability(node: &mut OctreeNode, field: &dyn ScalarField, unit_size: f32) {
        // All present children must be clusterable
        for c in node.children.iter().flatten() {
            if !c.clusterable {
                node.clusterable = false;
                return;
            }
        }

        // Check face-pair clusterability across the 12 face pairs
        for mask in &CELL_PROC_FACE_MASK {
            let c0_idx = mask[0];
            let c1_idx = mask[1];
            let dir = mask[2];

            let (left, right) = match (&node.children[c0_idx], &node.children[c1_idx]) {
                (Some(l), Some(r)) => (l, r),
                _ => continue,
            };

            if !RectilinearGrid::cal_clusterability(
                &left.grid,
                &right.grid,
                dir,
                node.grid.min_code,
                node.grid.max_code,
                field,
                unit_size,
            ) {
                node.clusterable = false;
                return;
            }
        }

        node.clusterable = true;
    }

    /// Hierarchically combines child grid components along Z, Y, then X axes.
    ///
    /// This is the core of the multi-component QEF merge.  It creates
    /// intermediate grids by combining pairs of children along each axis,
    /// then resolves parent pointers so the final combined grid has the
    /// correct component structure.
    fn combine_components(&mut self, unit_size: f32) {
        let min_code = self.grid.min_code;
        let max_code = self.grid.max_code;
        let mid_code = (min_code + max_code) / 2;

        // Step 1: Combine along Z axis (4 pairs)
        // Pairs: (0,1), (2,3), (4,5), (6,7)
        let z_pairs: [(usize, usize); 4] = [(0, 1), (2, 3), (4, 5), (6, 7)];
        let mut z_grids: [Option<RectilinearGrid>; 4] = Default::default();

        for (gi, &(left_idx, right_idx)) in z_pairs.iter().enumerate() {
            let (left, right) = match (&self.children[left_idx], &self.children[right_idx]) {
                (Some(l), Some(r)) => (l, r),
                (Some(l), None) => {
                    z_grids[gi] = Some(l.grid.clone());
                    continue;
                }
                (None, Some(r)) => {
                    z_grids[gi] = Some(r.grid.clone());
                    continue;
                }
                (None, None) => continue,
            };

            let offset = min_offset_subdivision(left_idx);
            let half = (max_code - min_code) / 2;
            let g_min = PositionCode::new(
                min_code.x + (offset.x as i32) * half.x,
                min_code.y + (offset.y as i32) * half.y,
                min_code.z,
            );
            let g_max = PositionCode::new(g_min.x + half.x, g_min.y + half.y, max_code.z);

            let qef = QefSolver::new();
            let mut out = RectilinearGrid::new(g_min, g_max, qef, unit_size);
            RectilinearGrid::combine_aa_grid(&left.grid, &right.grid, 2, &mut out, unit_size);
            z_grids[gi] = Some(out);
        }

        // Step 2: Combine along Y axis (2 pairs from the Z results)
        // Pairs: (z0, z1) and (z2, z3)
        let y_pairs: [(usize, usize); 2] = [(0, 1), (2, 3)];
        let mut y_grids: [Option<RectilinearGrid>; 2] = Default::default();

        for (gi, &(left_idx, right_idx)) in y_pairs.iter().enumerate() {
            let (left, right) = match (&z_grids[left_idx], &z_grids[right_idx]) {
                (Some(l), Some(r)) => (l, r),
                (Some(l), None) => {
                    y_grids[gi] = Some(l.clone());
                    continue;
                }
                (None, Some(r)) => {
                    y_grids[gi] = Some(r.clone());
                    continue;
                }
                (None, None) => continue,
            };

            let g_min = PositionCode::new(
                if gi == 0 { min_code.x } else { mid_code.x },
                min_code.y,
                min_code.z,
            );
            let g_max = PositionCode::new(
                if gi == 0 { mid_code.x } else { max_code.x },
                max_code.y,
                max_code.z,
            );

            let qef = QefSolver::new();
            let mut out = RectilinearGrid::new(g_min, g_max, qef, unit_size);
            RectilinearGrid::combine_aa_grid(left, right, 1, &mut out, unit_size);
            y_grids[gi] = Some(out);
        }

        // Step 3: Combine along X axis (final merge)
        match (&y_grids[0], &y_grids[1]) {
            (Some(left), Some(right)) => {
                let qef = QefSolver::new();
                let mut out = RectilinearGrid::new(min_code, max_code, qef, unit_size);
                RectilinearGrid::combine_aa_grid(left, right, 0, &mut out, unit_size);

                // Check for MC edge case: if combined component point counts
                // don't match all_qef, mark not clusterable.
                let total_component_points: i32 =
                    out.components.iter().map(|c| c.point_count()).sum();
                if total_component_points != out.all_qef.point_count() {
                    self.clusterable = false;
                }

                self.grid.components = out.components;
                self.grid.vertices = out.vertices;
                self.grid.corner_signs = out.corner_signs;
                self.grid.component_indices = out.component_indices;
                self.grid.is_signed = out.is_signed;
            }
            (Some(single), None) | (None, Some(single)) => {
                self.grid.components = single.components.clone();
                self.grid.vertices = single.vertices.clone();
                self.grid.corner_signs = single.corner_signs;
                self.grid.component_indices = single.component_indices;
                self.grid.is_signed = single.is_signed;
            }
            (None, None) => {}
        }

        // Resolve parent pointers: set each child vertex's parent to the
        // corresponding component vertex index in this node.
        for ci in 0..8 {
            if let Some(child) = &mut self.children[ci] {
                for corner in 0..8 {
                    let child_comp = child.grid.component_indices[corner];
                    if child_comp < 0 {
                        continue;
                    }
                    let parent_comp = self.grid.component_indices[corner];
                    if parent_comp < 0 {
                        continue;
                    }
                    let child_comp_idx = child_comp as usize;
                    let parent_comp_idx = parent_comp as usize;
                    if child_comp_idx < child.grid.vertices.len()
                        && parent_comp_idx < self.grid.vertices.len()
                    {
                        child.grid.vertices[child_comp_idx].parent = Some(parent_comp_idx);
                    }
                }
            }
        }
    }

    /// Extracts a triangle mesh from the octree.
    ///
    /// First assigns vertex indices to all leaf vertices, then runs the
    /// contouring procedures (cell, face, edge) to generate quads.
    pub fn extract_mesh(root: &mut OctreeNode, field: &dyn ScalarField, unit_size: f32) -> IsoMesh {
        let mut mesh = IsoMesh::new();
        Self::generate_vertex_indices(root, &mut mesh, field);
        Self::contour_cell(root, &mut mesh, field, 0.0, unit_size);
        mesh
    }

    /// Recursively assigns mesh vertex indices to all leaf vertices.
    fn generate_vertex_indices(node: &mut OctreeNode, mesh: &mut IsoMesh, field: &dyn ScalarField) {
        if node.is_leaf {
            for v in &mut node.grid.vertices {
                mesh.add_vertex(v, |p| field.normal(p));
            }
            return;
        }

        for c in node.children.iter_mut().flatten() {
            Self::generate_vertex_indices(c, mesh, field);
        }
    }

    /// Cell procedure: recurse into children, then process face and edge pairs.
    fn contour_cell(
        node: &OctreeNode,
        mesh: &mut IsoMesh,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
    ) {
        if node.is_leaf {
            return;
        }

        // Recurse into children
        for c in node.children.iter().flatten() {
            Self::contour_cell(c, mesh, field, threshold, unit_size);
        }

        // Process 12 face pairs
        for mask in &CELL_PROC_FACE_MASK {
            let c0 = mask[0];
            let c1 = mask[1];
            let dir = mask[2];

            let n0: &OctreeNode = match &node.children[c0] {
                Some(c) => c,
                None => continue,
            };
            let n1: &OctreeNode = match &node.children[c1] {
                Some(c) => c,
                None => continue,
            };

            Self::contour_face(
                &[n0, n1],
                dir,
                mesh,
                field,
                threshold,
                unit_size,
                node.depth,
            );
        }

        // Process 6 edge groups
        for mask in &CELL_PROC_EDGE_MASK {
            let dir = mask[4];
            let mut nodes: [Option<&OctreeNode>; 4] = [None; 4];
            let mut all_present = true;
            for i in 0..4 {
                match &node.children[mask[i]] {
                    Some(c) => nodes[i] = Some(c),
                    None => {
                        all_present = false;
                        break;
                    }
                }
            }
            if !all_present {
                continue;
            }
            let quad_dir2 = match dir {
                0 => 2,
                1 => 0,
                2 => 1,
                _ => continue,
            };
            Self::contour_edge(
                &[
                    nodes[0].unwrap_or(node),
                    nodes[1].unwrap_or(node),
                    nodes[2].unwrap_or(node),
                    nodes[3].unwrap_or(node),
                ],
                dir,
                quad_dir2,
                mesh,
                field,
                threshold,
                unit_size,
                node.depth,
            );
        }
    }

    /// Returns a child node for face/edge subdivision.
    ///
    /// If the node is a leaf, returns the node itself.  If the node is
    /// internal and the child exists, returns that child.  If the child is
    /// `None` the node itself is returned -- the caller treats it as having
    /// no geometry in that octant.
    fn get_child_or_self(node: &OctreeNode, child_idx: usize) -> &OctreeNode {
        if node.is_leaf {
            return node;
        }
        match &node.children[child_idx] {
            Some(c) => c,
            None => node,
        }
    }

    /// Face procedure: subdivide across a shared face between two nodes.
    #[allow(clippy::too_many_arguments)]
    fn contour_face(
        nodes: &[&OctreeNode; 2],
        dir: usize,
        mesh: &mut IsoMesh,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
        max_depth: u32,
    ) {
        if nodes[0].is_leaf && nodes[1].is_leaf {
            return;
        }
        if max_depth == 0 {
            return;
        }

        // Subdivide into 4 sub-faces
        // C++ always picks sub0 from nodes[0] and sub1 from nodes[1]
        for mask in &FACE_PROC_FACE_MASK[dir] {
            let child0_idx = mask[0];
            let child1_idx = mask[1];

            let sub0: &OctreeNode = Self::get_child_or_self(nodes[0], child0_idx);
            let sub1: &OctreeNode = Self::get_child_or_self(nodes[1], child1_idx);

            Self::contour_face(
                &[sub0, sub1],
                dir,
                mesh,
                field,
                threshold,
                unit_size,
                max_depth - 1,
            );
        }

        // Process 4 edges along this face
        for mask in &FACE_PROC_EDGE_MASK[dir] {
            let order = mask[0]; // node selector for child pairing
            let c0_idx = mask[1];
            let c1_idx = mask[2];
            let c2_idx = mask[3];
            let c3_idx = mask[4];
            let edge_dir = mask[5]; // the actual 3D edge direction

            let e0 = Self::get_child_or_self(nodes[order], c0_idx);
            let e1 = Self::get_child_or_self(nodes[order], c1_idx);
            let e2 = Self::get_child_or_self(nodes[1 - order], c2_idx);
            let e3 = Self::get_child_or_self(nodes[1 - order], c3_idx);

            // For an edge along edge_dir, the two quad directions are
            // the other two axes.
            let quad_dir2 = 3 - dir - edge_dir;

            Self::contour_edge(
                &[e0, e1, e2, e3],
                edge_dir,
                quad_dir2,
                mesh,
                field,
                threshold,
                unit_size,
                max_depth - 1,
            );
        }
    }

    /// Edge procedure: either generate a quad or subdivide further.
    #[allow(clippy::too_many_arguments)]
    fn contour_edge(
        nodes: &[&OctreeNode; 4],
        dir: usize,
        quad_dir2: usize,
        mesh: &mut IsoMesh,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
        max_depth: u32,
    ) {
        if nodes[0].is_leaf && nodes[1].is_leaf && nodes[2].is_leaf && nodes[3].is_leaf {
            Self::generate_quad_from_nodes(
                nodes, dir, quad_dir2, mesh, field, threshold, unit_size,
            );
            return;
        }
        if max_depth == 0 {
            return;
        }

        // Subdivide: compute child indices dynamically from quadDir1, quadDir2
        // matching C++ exactly: code[dir]=i, code[quadDir1]=(3-j)%2, code[quadDir2]=(3-j)/2
        let quad_dir1 = 3 - dir - quad_dir2;
        for i in 0..2_i32 {
            let mut sub_nodes: [&OctreeNode; 4] = [nodes[0], nodes[1], nodes[2], nodes[3]];

            for j in 0..4_i32 {
                if !nodes[j as usize].is_leaf {
                    let mut code = glam::IVec3::ZERO;
                    code[dir] = i;
                    code[quad_dir1] = (3 - j) % 2;
                    code[quad_dir2] = (3 - j) / 2;
                    let child_idx = encode_cell(code);
                    if let Some(c) = &nodes[j as usize].children[child_idx] {
                        sub_nodes[j as usize] = c;
                    }
                }
            }

            Self::contour_edge(
                &sub_nodes,
                dir,
                quad_dir2,
                mesh,
                field,
                threshold,
                unit_size,
                max_depth - 1,
            );
        }
    }

    /// Delegates to the generic `generate_quad` function.
    fn generate_quad_from_nodes(
        nodes: &[&OctreeNode; 4],
        dir: usize,
        quad_dir2: usize,
        mesh: &mut IsoMesh,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
    ) {
        let quad_dir1 = 3 - quad_dir2 - dir;

        let has_grid: [&dyn HasGrid; 4] = [nodes[0], nodes[1], nodes[2], nodes[3]];
        let has_grid_refs: Vec<&dyn HasGrid> = has_grid.to_vec();

        if check_sign(&has_grid_refs, quad_dir1, quad_dir2, field, unit_size).is_none() {
            return;
        }

        generate_quad(
            &has_grid, quad_dir1, quad_dir2, mesh, field, threshold, unit_size,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::scalar_field::Sphere;

    #[test]
    fn build_sphere_octree_produces_vertices() {
        // Use a dedicated thread with a larger stack to avoid overflow
        // from the recursive octree build + contouring.
        let result = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                let sphere = Sphere::with_center(3.0, glam::Vec3::new(8.0, 8.0, 8.0));
                let unit_size = 1.0;
                let depth = 4; // 16x16x16 grid
                let min_code = PositionCode::splat(0);

                let root =
                    OctreeNode::build_with_scalar_field(min_code, depth, &sphere, false, unit_size);
                assert!(root.is_some(), "Octree should not be empty for a sphere");

                let mut root = root.unwrap_or_else(|| unreachable!());
                let mesh = OctreeNode::extract_mesh(&mut root, &sphere, unit_size);

                assert!(
                    !mesh.positions.is_empty(),
                    "Mesh should have vertices, got 0"
                );
                assert!(!mesh.indices.is_empty(), "Mesh should have indices, got 0");
                assert!(
                    mesh.triangle_count() > 0,
                    "Mesh should have triangles, got 0"
                );
            });
        match result {
            Ok(handle) => {
                if let Err(e) = handle.join() {
                    std::panic::resume_unwind(e);
                }
            }
            Err(e) => panic!("Failed to spawn test thread: {e}"),
        }
    }

    #[test]
    fn simplify_reduces_leaf_count() {
        let sphere = Sphere::with_center(3.0, glam::Vec3::new(4.0, 4.0, 4.0));
        let unit_size = 1.0;
        let depth = 3;
        let min_code = PositionCode::splat(0);

        let root = OctreeNode::build_with_scalar_field(min_code, depth, &sphere, false, unit_size);
        assert!(root.is_some());
        let mut root = root.unwrap_or_else(|| unreachable!());

        let leaves_before = count_leaves(&root);
        OctreeNode::simplify(&mut root, 1.0);
        let leaves_after = count_leaves(&root);

        assert!(
            leaves_after <= leaves_before,
            "Simplify should not increase leaf count: before={leaves_before}, after={leaves_after}"
        );
    }

    #[test]
    fn empty_field_produces_no_octree() {
        let sphere = Sphere::with_center(1.0, glam::Vec3::new(100.0, 100.0, 100.0));
        let unit_size = 1.0;
        let depth = 3;
        let min_code = PositionCode::splat(0);

        let root = OctreeNode::build_with_scalar_field(min_code, depth, &sphere, false, unit_size);
        assert!(
            root.is_none(),
            "Octree should be empty when field is far away"
        );
    }

    fn count_leaves(node: &OctreeNode) -> usize {
        if node.is_leaf {
            return 1;
        }
        node.children
            .iter()
            .flatten()
            .map(|c| count_leaves(c))
            .sum()
    }
}
