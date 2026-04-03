//! K-d tree based dual contouring for isosurface extraction.
//!
//! Builds a binary space partition tree from an octree, producing fewer
//! triangles than the octree alone for the same error threshold. This is a
//! port of the KdtreeISO algorithm.

use super::indicators::{PositionCode, code_to_pos, opposite_quad_index};
use super::mesh_output::IsoMesh;
use super::octree::OctreeNode;
use super::qef::QefSolver;
use super::rectilinear_grid::{HasGrid, RectilinearGrid, generate_quad};
use super::scalar_field::ScalarField;

/// A node in the k-d tree used for dual contouring.
///
/// Each node represents an axis-aligned cell that can be split along one axis
/// (the `plane_dir`). Leaf nodes have no children; internal nodes have exactly
/// two children split along the chosen axis.
#[derive(Debug, Clone)]
pub struct KdTreeNode {
    /// The rectilinear grid cell for this node (signs, QEF, vertices).
    pub grid: RectilinearGrid,
    /// Split axis (0 = X, 1 = Y, 2 = Z).
    pub plane_dir: usize,
    /// Depth of this node in the tree.
    pub depth: u32,
    /// Whether this node and its descendants can be merged without artifacts.
    pub clusterable: bool,
    /// Two children: `[0]` = lower half, `[1]` = upper half along `plane_dir`.
    pub children: [Option<Box<KdTreeNode>>; 2],
}

impl HasGrid for KdTreeNode {
    fn grid(&self) -> &RectilinearGrid {
        &self.grid
    }
}

/// An axis-aligned line used during edge contouring.
struct AALine {
    point: PositionCode,
    dir: usize,
}

impl KdTreeNode {
    /// Returns `true` if this node has no children (is a leaf).
    pub fn is_leaf(&self) -> bool {
        self.children[0].is_none() && self.children[1].is_none()
    }

    /// Returns `true` if this node should be treated as a contouring leaf.
    ///
    /// A node is a contouring leaf if it has no children, or if it is
    /// clusterable and all vertex errors are at or below the threshold.
    pub fn is_contouring_leaf(&self, threshold: f32) -> bool {
        if self.is_leaf() {
            return true;
        }
        for v in &self.grid.vertices {
            if v.error > threshold {
                return false;
            }
        }
        self.clusterable
    }

    /// Returns the split plane position along `plane_dir`.
    fn axis(&self) -> i32 {
        if let Some(c) = &self.children[0] {
            c.grid.max_code[self.plane_dir]
        } else if let Some(c) = &self.children[1] {
            c.grid.min_code[self.plane_dir]
        } else {
            0
        }
    }

    /// Combines QEF data from children into this node's grid.
    fn combine_qef(&mut self, unit_size: f32) {
        if !self.clusterable || self.is_leaf() {
            return;
        }
        let left = self.children[0].as_ref();
        let right = self.children[1].as_ref();
        if let (Some(l), Some(r)) = (left, right) {
            let dir = self.plane_dir;
            let min_code = self.grid.min_code;
            let max_code = self.grid.max_code;
            let qef = QefSolver::new();
            let mut out = RectilinearGrid::new(min_code, max_code, qef, unit_size);
            RectilinearGrid::combine_aa_grid(&l.grid, &r.grid, dir, &mut out, unit_size);
            self.grid.components = out.components;
            self.grid.vertices = out.vertices;
            self.grid.corner_signs = out.corner_signs;
            self.grid.component_indices = out.component_indices;
            self.grid.is_signed = out.is_signed;
            self.grid.all_qef = out.all_qef;
            self.grid.approximate = out.approximate;
        }
    }

    /// Checks whether this node can be clustered (merged with its children).
    fn cal_clusterability(&mut self, field: &dyn ScalarField, unit_size: f32) {
        if self.is_leaf() {
            self.clusterable = true;
            return;
        }

        // C++ checks calClusterability first, then checks children
        if let (Some(left), Some(right)) = (&self.children[0], &self.children[1]) {
            let dir = self.plane_dir;
            if !RectilinearGrid::cal_clusterability(
                &left.grid,
                &right.grid,
                dir,
                self.grid.min_code,
                self.grid.max_code,
                field,
                unit_size,
            ) {
                self.clusterable = false;
                return;
            }
        }

        for child in self.children.iter().flatten() {
            if !child.clusterable {
                self.clusterable = false;
                return;
            }
        }

        self.clusterable = true;
    }

    /// Chooses the best split axis using variance-weighted selection.
    ///
    /// Selects the axis with the highest weighted variance from the QEF,
    /// ensuring the cell is at least 2 units wide along that axis.
    fn choose_axis_dir(
        qef: &mut QefSolver,
        min_code: PositionCode,
        max_code: PositionCode,
    ) -> usize {
        let size = max_code - min_code;

        // Solve QEF to get the approximate position, then compute variance
        let (approximate, _error) = qef.solve();
        let mut variance = qef.get_variance(approximate);
        variance.x *= size.x as f32;
        variance.y *= size.y as f32;
        variance.z *= size.z as f32;

        // Find max and min variance directions
        let mut max_var_dir: usize = 0;
        let mut min_var_dir: usize = 1;
        if variance[1] > variance[0] {
            max_var_dir = 1;
            min_var_dir = 0;
        }
        if variance[2] > variance[max_var_dir] {
            max_var_dir = 2;
        }
        if variance[min_var_dir] > variance[2] {
            min_var_dir = 2;
        }

        let mut dir = max_var_dir;
        if size[max_var_dir] < 2 {
            dir = 3 - max_var_dir - min_var_dir;
            if size[3 - max_var_dir - min_var_dir] < 2 {
                dir = min_var_dir;
            }
        }

        dir
    }

    /// Finds the optimal split plane along an axis using binary search on QEF error.
    fn find_split_plane(
        octree: &OctreeNode,
        min_code: PositionCode,
        max_code: PositionCode,
        axis: usize,
        unit_size: f32,
    ) -> (PositionCode, PositionCode) {
        let mut min_axis = min_code[axis];
        let mut max_axis = max_code[axis];

        let mut best_left_max = min_code;
        let mut best_right_min = max_code;
        let mut min_error = f32::MAX;

        while max_axis - min_axis > 1 {
            let mid = (max_axis + min_axis) / 2;

            let mut right_min_code = min_code;
            right_min_code[axis] = mid;
            let mut left_max_code = max_code;
            left_max_code[axis] = mid;

            let mut left_sum = OctreeNode::get_sum(
                octree,
                code_to_pos(min_code, unit_size),
                code_to_pos(left_max_code, unit_size),
                unit_size,
            );
            let mut right_sum = OctreeNode::get_sum(
                octree,
                code_to_pos(right_min_code, unit_size),
                code_to_pos(max_code, unit_size),
                unit_size,
            );

            let (_left_approx, left_error) = left_sum.solve();
            let (_right_approx, right_error) = right_sum.solve();

            let diff = (left_error - right_error).abs();
            if diff < min_error {
                min_error = diff;
                best_left_max = left_max_code;
                best_right_min = right_min_code;
            }

            if left_error > right_error {
                max_axis = mid;
            } else if left_error < right_error {
                min_axis = mid;
            } else {
                break;
            }
        }

        (best_left_max, best_right_min)
    }

    /// Builds a k-d tree from an octree using binary search split planes.
    ///
    /// Recursively subdivides the region `[min_code, max_code]` by choosing
    /// the best split axis and finding the optimal split plane via binary
    /// search in the octree's QEF data.
    ///
    /// Returns `None` if the region contains no surface.
    pub fn build_from_octree(
        octree: &OctreeNode,
        min_code: PositionCode,
        max_code: PositionCode,
        field: &dyn ScalarField,
        depth: u32,
        unit_size: f32,
    ) -> Option<Box<KdTreeNode>> {
        // C++: if (any(greaterThanEqual(minCode, maxCode))) return nullptr;
        if min_code.x >= max_code.x || min_code.y >= max_code.y || min_code.z >= max_code.z {
            return None;
        }
        if depth > 64 {
            return None;
        }

        // Accumulate QEF from octree for this region
        let min_pos = code_to_pos(min_code, unit_size);
        let max_pos = code_to_pos(max_code, unit_size);
        let mut qef = OctreeNode::get_sum(octree, min_pos, max_pos, unit_size);

        if qef.point_count() <= 0 {
            return None;
        }

        // Choose split axis
        let dir = Self::choose_axis_dir(&mut qef, min_code, max_code);

        // Find split plane via binary search
        let (best_left_max, best_right_min) =
            Self::find_split_plane(octree, min_code, max_code, dir, unit_size);

        // Create the node
        let grid = RectilinearGrid::new(min_code, max_code, qef, unit_size);
        let mut node = Box::new(KdTreeNode {
            grid,
            plane_dir: dir,
            depth,
            clusterable: true,
            children: [None, None],
        });

        // Build children
        node.children[0] =
            Self::build_from_octree(octree, min_code, best_left_max, field, depth + 1, unit_size);
        node.children[1] = Self::build_from_octree(
            octree,
            best_right_min,
            max_code,
            field,
            depth + 1,
            unit_size,
        );

        if node.is_leaf() {
            // Leaf: assign signs and sample QEF
            node.grid.assign_sign(field, unit_size);
            if node.grid.is_signed {
                node.grid.cal_corner_components();
                let mut all_qef = QefSolver::new();
                node.grid.sample_qef(field, &mut all_qef, unit_size);
                node.grid.all_qef = all_qef;
            }
        } else {
            // Internal: assign signs and check clusterability
            node.grid.assign_sign(field, unit_size);
            node.cal_clusterability(field, unit_size);
            node.combine_qef(unit_size);
        }

        if node.clusterable {
            for i in 0..node.grid.components.len() {
                node.grid.solve_component(i, unit_size);
            }
        }

        Some(node)
    }

    /// Extracts a triangle mesh from the k-d tree.
    ///
    /// Assigns vertex indices to all contouring leaves, then runs the
    /// recursive contouring procedures (cell, face, edge) to generate quads.
    pub fn extract_mesh(
        root: &mut KdTreeNode,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
    ) -> IsoMesh {
        let mut mesh = IsoMesh::new();
        generate_vertex_indices(root, threshold, field, &mut mesh);
        contour_cell(root, &mut mesh, field, threshold, unit_size);
        mesh
    }
}

/// Recursively assigns mesh vertex indices to all contouring leaf vertices.
fn generate_vertex_indices(
    node: &mut KdTreeNode,
    threshold: f32,
    field: &dyn ScalarField,
    mesh: &mut IsoMesh,
) {
    for v in &mut node.grid.vertices {
        mesh.add_vertex(v, |p| field.normal(p));
    }

    if node.is_contouring_leaf(threshold) {
        return;
    }

    for child in node.children.iter_mut().flatten() {
        generate_vertex_indices(child, threshold, field, mesh);
    }
}

/// Cell procedure: contour the shared face, then recurse into children.
fn contour_cell(
    node: &KdTreeNode,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    if node.is_contouring_leaf(threshold) {
        return;
    }

    // C++: face contouring happens BEFORE recursing into children
    if let (Some(left), Some(right)) = (&node.children[0], &node.children[1]) {
        let face_nodes: [&KdTreeNode; 2] = [left, right];
        contour_face(
            face_nodes,
            node.plane_dir,
            node.axis(),
            mesh,
            field,
            threshold,
            unit_size,
        );
    }

    for child in node.children.iter().flatten() {
        contour_cell(child, mesh, field, threshold, unit_size);
    }
}

/// Checks whether a face between two nodes has sufficient overlap.
///
/// Returns `Some((face_min, face_max))` if the face is valid, `None` otherwise.
fn check_minimal_face(nodes: [&KdTreeNode; 2], dir: usize) -> Option<(PositionCode, PositionCode)> {
    let face_max = nodes[0].grid.max_code.min(nodes[1].grid.max_code);
    let face_min = nodes[0].grid.min_code.max(nodes[1].grid.min_code);
    let offset = face_max - face_min;
    if offset[(dir + 1) % 3] > 0 && offset[(dir + 2) % 3] > 0 {
        Some((face_min, face_max))
    } else {
        None
    }
}

/// Constructs an axis-aligned line at the intersection of a face and a split plane.
fn construct_line(
    face_nodes: [&KdTreeNode; 2],
    side: usize,
    origin_face_dir: usize,
    axis: i32,
) -> AALine {
    let mut point = PositionCode::ZERO;
    point[origin_face_dir] = axis;
    let dir = 3 - origin_face_dir - face_nodes[side].plane_dir;
    point[face_nodes[side].plane_dir] = face_nodes[side].axis();
    AALine { point, dir }
}

/// Face procedure: given two adjacent nodes sharing a face along `dir`.
///
/// Matches C++ `contourFace` (lines 241-285).
#[allow(clippy::too_many_arguments)]
fn contour_face(
    mut nodes: [&KdTreeNode; 2],
    dir: usize,
    axis: i32,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    // Both are contouring leaves -> return
    if nodes[0].is_contouring_leaf(threshold) && nodes[1].is_contouring_leaf(threshold) {
        return;
    }

    // Check face overlap
    let (face_min, face_max) = match check_minimal_face(nodes, dir) {
        Some(v) => v,
        None => return,
    };

    // Descend while planeDir == dir for each node
    #[allow(clippy::needless_range_loop)]
    for i in 0..2usize {
        while !nodes[i].is_contouring_leaf(threshold) && nodes[i].plane_dir == dir {
            let child = &nodes[i].children[1 - i];
            match child {
                Some(c) => nodes[i] = c,
                None => return,
            }
        }
    }

    // For each non-leaf node (that doesn't split along dir)
    for i in 0..2 {
        if !nodes[i].is_contouring_leaf(threshold) {
            // Recurse into both children
            for j in 0..2 {
                if let Some(child) = &nodes[i].children[j] {
                    let mut next_face = nodes;
                    next_face[i] = child;
                    contour_face(next_face, dir, axis, mesh, field, threshold, unit_size);
                }
            }
            // Generate edge contour where the split creates a new edge
            if nodes[i].axis() > face_min[nodes[i].plane_dir]
                && nodes[i].axis() < face_max[nodes[i].plane_dir]
            {
                let mut edge_nodes: [&KdTreeNode; 4] = [nodes[0], nodes[0], nodes[1], nodes[1]];
                if let Some(c0) = &nodes[i].children[0] {
                    edge_nodes[i * 2] = c0;
                }
                if let Some(c1) = &nodes[i].children[1] {
                    edge_nodes[i * 2 + 1] = c1;
                }
                let line = construct_line(nodes, i, dir, axis);
                contour_edge(
                    edge_nodes,
                    &line,
                    nodes[i].plane_dir,
                    field,
                    threshold,
                    mesh,
                    unit_size,
                );
            }
            return;
        }
    }
}

/// Checks whether an edge between four nodes has sufficient overlap.
fn check_minimal_edge(
    nodes: [&KdTreeNode; 4],
    line: &AALine,
) -> Option<(PositionCode, PositionCode)> {
    let mut min_end = line.point;
    let mut max_end = line.point;
    let dir = line.dir;
    min_end[dir] = nodes
        .iter()
        .map(|n| n.grid.min_code[dir])
        .max()
        .unwrap_or(0);
    max_end[dir] = nodes
        .iter()
        .map(|n| n.grid.max_code[dir])
        .min()
        .unwrap_or(0);
    if min_end[dir] < max_end[dir] {
        Some((min_end, max_end))
    } else {
        None
    }
}

/// Computes the child index for quad descent.
fn next_quad_index(dir1: usize, dir2: usize, plane_dir: usize, i: usize) -> usize {
    let mut pos = PositionCode::ZERO;
    pos[dir1] = 1 - (i % 2) as i32;
    pos[dir2] = 1 - (i / 2) as i32;
    pos[plane_dir] as usize
}

/// Descends paired nodes to the correct children for quad detection.
fn detect_quad(nodes: &mut [&KdTreeNode; 4], line: &AALine, threshold: f32) {
    for i in 0..2 {
        loop {
            let a = nodes[i * 2];
            let b = nodes[i * 2 + 1];
            if !std::ptr::eq(a, b) {
                break;
            }
            if a.is_contouring_leaf(threshold) {
                break;
            }
            if a.plane_dir == line.dir {
                break;
            }
            let common = a;
            if common.axis() == line.point[common.plane_dir] {
                match (&common.children[0], &common.children[1]) {
                    (Some(c0), Some(c1)) => {
                        nodes[i * 2] = c0;
                        nodes[i * 2 + 1] = c1;
                    }
                    _ => break,
                }
            } else if common.axis() > line.point[common.plane_dir] {
                match &common.children[0] {
                    Some(c0) => {
                        nodes[i * 2] = c0;
                        nodes[i * 2 + 1] = c0;
                    }
                    None => break,
                }
            } else {
                match &common.children[1] {
                    Some(c1) => {
                        nodes[i * 2] = c1;
                        nodes[i * 2 + 1] = c1;
                    }
                    None => break,
                }
            }
        }
    }
}

/// Sets a quad node, also updating the opposite node if they point to the same node.
fn set_quad_node<'a>(nodes: &mut [&'a KdTreeNode; 4], i: usize, new_node: &'a KdTreeNode) {
    if std::ptr::eq(nodes[opposite_quad_index(i)], nodes[i]) {
        nodes[opposite_quad_index(i)] = new_node;
    }
    nodes[i] = new_node;
}

/// Edge procedure: generates quads along an edge shared by 4 nodes.
///
/// Matches C++ `contourEdge` (lines 332-390).
#[allow(clippy::too_many_arguments)]
fn contour_edge(
    mut nodes: [&KdTreeNode; 4],
    line: &AALine,
    quad_dir1: usize,
    field: &dyn ScalarField,
    threshold: f32,
    mesh: &mut IsoMesh,
    unit_size: f32,
) {
    detect_quad(&mut nodes, line, threshold);

    let quad_dir2 = 3 - quad_dir1 - line.dir;

    if check_minimal_edge(nodes, line).is_none() {
        return;
    }

    // Descend non-leaf nodes that don't split along line.dir
    for i in 0..4 {
        if !std::ptr::eq(nodes[i], nodes[opposite_quad_index(i)]) {
            while !nodes[i].is_contouring_leaf(threshold) && nodes[i].plane_dir != line.dir {
                let child_idx = next_quad_index(quad_dir1, quad_dir2, nodes[i].plane_dir, i);
                match &nodes[i].children[child_idx] {
                    Some(c) => nodes[i] = c,
                    None => return,
                }
            }
        }
    }

    // All 4 are contouring leaves: generate quad
    if nodes[0].is_contouring_leaf(threshold)
        && nodes[1].is_contouring_leaf(threshold)
        && nodes[2].is_contouring_leaf(threshold)
        && nodes[3].is_contouring_leaf(threshold)
    {
        kd_generate_quad(
            nodes, quad_dir1, quad_dir2, mesh, field, threshold, unit_size,
        );
        return;
    }

    // Recurse: find a node splitting along line.dir
    for i in 0..4 {
        if !nodes[i].is_contouring_leaf(threshold) && nodes[i].plane_dir == line.dir {
            if let Some(c0) = &nodes[i].children[0] {
                let mut next_nodes = nodes;
                set_quad_node(&mut next_nodes, i, c0);
                contour_edge(
                    next_nodes, line, quad_dir1, field, threshold, mesh, unit_size,
                );
            }
            if let Some(c1) = &nodes[i].children[1] {
                let mut next_nodes = nodes;
                set_quad_node(&mut next_nodes, i, c1);
                contour_edge(
                    next_nodes, line, quad_dir1, field, threshold, mesh, unit_size,
                );
            }
            return;
        }
    }
}

/// Generates a quad from 4 contouring leaf nodes.
///
/// Delegates to `RectilinearGrid::generate_quad`.
#[allow(clippy::too_many_arguments)]
fn kd_generate_quad(
    nodes: [&KdTreeNode; 4],
    quad_dir1: usize,
    quad_dir2: usize,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    let has_grid: [&dyn HasGrid; 4] = [nodes[0], nodes[1], nodes[2], nodes[3]];
    generate_quad(
        &has_grid, quad_dir1, quad_dir2, mesh, field, threshold, unit_size,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::indicators::opposite_quad_index;
    use crate::isosurface::scalar_field::Sphere;

    #[test]
    fn build_kdtree_from_octree_sphere() {
        // Use a dedicated thread with a larger stack to avoid overflow
        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let sphere = Sphere::with_center(3.0, glam::Vec3::new(8.0, 8.0, 8.0));
                let unit_size = 1.0;
                let depth = 4; // 16x16x16 grid
                let size_code = PositionCode::splat(1 << (depth - 1));
                let min_code = -size_code / 2;
                let max_code = size_code;

                // Build octree first
                let octree = OctreeNode::build_with_scalar_field(
                    min_code, depth, &sphere, true, unit_size,
                );
                assert!(octree.is_some(), "Octree should not be empty for a sphere");
                let octree = match octree {
                    Some(o) => o,
                    None => return,
                };

                // Build k-d tree from octree
                let kdtree = KdTreeNode::build_from_octree(
                    &octree, min_code, max_code, &sphere, 0, unit_size,
                );
                assert!(kdtree.is_some(), "KdTree should not be empty for a sphere");
                let mut kdtree = match kdtree {
                    Some(k) => k,
                    None => return,
                };

                // Extract mesh from k-d tree
                let kd_mesh = KdTreeNode::extract_mesh(&mut kdtree, &sphere, 0.0, unit_size);

                assert!(
                    !kd_mesh.positions.is_empty(),
                    "KdTree mesh should have vertices"
                );
                assert!(
                    !kd_mesh.indices.is_empty(),
                    "KdTree mesh should have indices"
                );

                let kd_triangles = kd_mesh.triangle_count();
                assert!(
                    kd_triangles > 0,
                    "KdTree mesh should have triangles, got 0"
                );

                // Also extract from octree for comparison
                let mut octree_for_mesh = OctreeNode::build_with_scalar_field(
                    min_code, depth, &sphere, false, unit_size,
                );
                if let Some(ref mut oct_root) = octree_for_mesh {
                    let oct_mesh = OctreeNode::extract_mesh(oct_root, &sphere, unit_size);
                    let oct_triangles = oct_mesh.triangle_count();

                    // K-d tree should produce a reasonable number of triangles
                    assert!(
                        kd_triangles <= oct_triangles * 2,
                        "KdTree triangles ({kd_triangles}) should be reasonable compared to octree ({oct_triangles})"
                    );
                }
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
    fn kdtree_leaf_node() {
        let qef = QefSolver::new();
        let grid = RectilinearGrid::new(PositionCode::splat(0), PositionCode::splat(2), qef, 1.0);
        let node = KdTreeNode {
            grid,
            plane_dir: 0,
            depth: 0,
            clusterable: true,
            children: [None, None],
        };
        assert!(node.is_leaf());
        assert!(node.is_contouring_leaf(1.0));
    }

    #[test]
    fn opposite_quad_index_roundtrip() {
        for i in 0..4 {
            assert_eq!(opposite_quad_index(opposite_quad_index(i)), i);
        }
    }
}
