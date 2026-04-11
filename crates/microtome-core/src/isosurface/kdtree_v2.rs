//! K-d tree dual contouring for isosurface extraction (v2).
//!
//! Clean reimplementation of the k-d tree algorithm with correct floating-point
//! binary search. The split-plane search uses `f32::abs()` for error comparison
//! instead of integer truncation, producing better split decisions.

use glam::Vec3;

use super::indicators::{PositionCode, opposite_quad_index};
use super::mesh_output::IsoMesh;
use super::octree::OctreeNode;
use super::qef::QefSolver;
use super::rectilinear_grid::{HasGrid, RectilinearGrid, generate_quad};
use super::scalar_field::ScalarField;

/// A node in the k-d tree used for adaptive dual contouring.
///
/// Each node represents an axis-aligned cell that may be split along one axis
/// into two children. Leaf nodes contain QEF-solved vertex data; internal nodes
/// aggregate child data for hierarchical simplification.
#[derive(Debug, Clone)]
pub struct KdTreeV2Node {
    /// The rectilinear grid cell holding QEF data, vertices, and corner signs.
    pub grid: RectilinearGrid,
    /// The axis (0=X, 1=Y, 2=Z) along which this node is split.
    pub plane_dir: usize,
    /// Depth of this node in the tree (root = 0).
    pub depth: u32,
    /// Whether this node's children can be merged for simplification.
    pub clusterable: bool,
    /// Left and right children (split along `plane_dir`).
    pub children: [Option<Box<KdTreeV2Node>>; 2],
}

/// An axis-aligned line used during edge contouring.
///
/// Represents the intersection of two splitting planes, parameterized by
/// a point on the line and the axis direction along which it extends.
struct AALine {
    point: PositionCode,
    dir: usize,
}

impl HasGrid for KdTreeV2Node {
    fn grid(&self) -> &RectilinearGrid {
        &self.grid
    }
}

impl KdTreeV2Node {
    /// Returns `true` if this node has no children.
    pub fn is_leaf(&self) -> bool {
        self.children[0].is_none() && self.children[1].is_none()
    }

    /// Returns `true` if this node should be treated as a leaf during contouring.
    ///
    /// A node is a contouring leaf if it is a true leaf, or if it is clusterable
    /// and all its vertices have error at or below the threshold.
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

    /// Returns the split plane coordinate along `plane_dir`.
    ///
    /// For non-leaf nodes, this is the boundary between the two children.
    pub fn axis(&self) -> i32 {
        if let Some(c) = &self.children[0] {
            c.grid.max_code[self.plane_dir]
        } else if let Some(c) = &self.children[1] {
            c.grid.min_code[self.plane_dir]
        } else {
            0
        }
    }

    /// Selects the best axis to split along based on QEF variance.
    ///
    /// Prefers the axis with highest variance weighted by cell extent,
    /// falling back to other axes if the preferred axis has extent < 2.
    pub fn choose_axis_dir(
        qef: &mut QefSolver,
        min_code: PositionCode,
        max_code: PositionCode,
    ) -> usize {
        let size = max_code - min_code;

        let mut approximate = Vec3::ZERO;
        let mut _error = 0.0_f32;
        qef.solve(&mut approximate, &mut _error);
        let mut variance = qef.get_variance(approximate);
        variance.x *= size.x as f32;
        variance.y *= size.y as f32;
        variance.z *= size.z as f32;

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

    /// Builds a k-d tree from an octree over the given region.
    ///
    /// Recursively subdivides the cell using a binary search to find the best
    /// split plane that balances QEF error between the two halves. Uses correct
    /// `f32::abs()` for error difference comparison.
    ///
    /// Returns `None` if the region is degenerate, exceeds max depth, or
    /// contains no octree data.
    pub fn build_from_octree(
        octree: &OctreeNode,
        min_code: PositionCode,
        max_code: PositionCode,
        field: &dyn ScalarField,
        depth: u32,
        unit_size: f32,
    ) -> Option<Box<KdTreeV2Node>> {
        if min_code.x >= max_code.x || min_code.y >= max_code.y || min_code.z >= max_code.z {
            return None;
        }
        if depth > 64 {
            return None;
        }

        let mut qef = QefSolver::new();
        OctreeNode::get_sum(octree, min_code, max_code, &mut qef);

        if qef.point_count() <= 0 {
            return None;
        }

        let dir = Self::choose_axis_dir(&mut qef, min_code, max_code);

        // Binary search for the best split plane
        let mut min_axis = min_code[dir];
        let mut max_axis = max_code[dir];
        let mut best_left_max = min_code;
        let mut best_right_min = max_code;
        let mut min_error = f32::MAX;

        while max_axis - min_axis > 1 {
            let mid = (max_axis + min_axis) / 2;

            let mut right_min_code = min_code;
            right_min_code[dir] = mid;
            let mut left_max_code = max_code;
            left_max_code[dir] = mid;

            let mut left_sum = QefSolver::new();
            OctreeNode::get_sum(octree, min_code, left_max_code, &mut left_sum);
            let mut right_sum = QefSolver::new();
            OctreeNode::get_sum(octree, right_min_code, max_code, &mut right_sum);

            let mut _left_approx = Vec3::ZERO;
            let mut left_error = 0.0_f32;
            left_sum.solve(&mut _left_approx, &mut left_error);
            let mut _right_approx = Vec3::ZERO;
            let mut right_error = 0.0_f32;
            right_sum.solve(&mut _right_approx, &mut right_error);

            // v2 fix: use correct f32 abs instead of integer truncation
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

        let grid = RectilinearGrid::new(min_code, max_code, qef, unit_size);
        let mut node = Box::new(KdTreeV2Node {
            grid,
            plane_dir: dir,
            depth,
            clusterable: true,
            children: [None, None],
        });

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
            node.grid.assign_sign(field, unit_size);
            node.grid.cal_corner_components();
            let mut throwaway = QefSolver::new();
            node.grid.sample_qef(field, &mut throwaway, unit_size);
        } else {
            node.grid.assign_sign(field, unit_size);
            node.cal_clusterability(field, unit_size);
            node.combine_qef();
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
    /// First assigns vertex indices, then runs the contouring procedures
    /// (cell, face, edge) to generate triangles. The `threshold` parameter
    /// controls simplification: higher values merge more cells.
    pub fn extract_mesh(
        root: &mut KdTreeV2Node,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
    ) -> IsoMesh {
        let mut mesh = IsoMesh::new();
        generate_vertex_indices(root, threshold, field, &mut mesh);
        contour_cell(root, &mut mesh, field, threshold, unit_size);
        mesh
    }

    /// Aggregates child QEF data into this node's grid.
    ///
    /// Clones child grids to work around simultaneous borrow constraints,
    /// then delegates to `RectilinearGrid::combine_aa_grid`.
    fn combine_qef(&mut self) {
        if !self.clusterable || self.is_leaf() {
            return;
        }
        let left = self.children[0].as_ref().map(|c| c.grid.clone());
        let right = self.children[1].as_ref().map(|c| c.grid.clone());
        let dir = self.plane_dir;
        RectilinearGrid::combine_aa_grid(left.as_ref(), right.as_ref(), dir, &mut self.grid);
    }

    /// Determines whether this node can be clustered for simplification.
    ///
    /// Checks both the grid-level clusterability (sign compatibility across
    /// the split plane) and that all children are themselves clusterable.
    fn cal_clusterability(&mut self, field: &dyn ScalarField, unit_size: f32) {
        if self.is_leaf() {
            self.clusterable = true;
            return;
        }

        let left = self.children[0].as_ref().map(|c| &c.grid);
        let right = self.children[1].as_ref().map(|c| &c.grid);
        let dir = self.plane_dir;
        if !RectilinearGrid::cal_clusterability(
            left,
            right,
            dir,
            self.grid.min_code,
            self.grid.max_code,
            field,
            unit_size,
        ) {
            self.clusterable = false;
            return;
        }

        for child in self.children.iter().flatten() {
            if !child.clusterable {
                self.clusterable = false;
                return;
            }
        }

        self.clusterable = true;
    }
}

// ---------------------------------------------------------------------------
// Free functions for mesh generation
// ---------------------------------------------------------------------------

/// Assigns mesh vertex indices to all vertices in the tree.
///
/// Walks the entire tree (ignoring contouring-leaf status) and registers
/// each vertex with the output mesh, computing its normal from the field.
#[allow(clippy::only_used_in_recursion)]
fn generate_vertex_indices(
    node: &mut KdTreeV2Node,
    threshold: f32,
    field: &dyn ScalarField,
    mesh: &mut IsoMesh,
) {
    for v in &mut node.grid.vertices {
        mesh.add_vertex(v, |p| field.normal(p));
    }

    for child in node.children.iter_mut().flatten() {
        generate_vertex_indices(child, threshold, field, mesh);
    }
}

/// Recursively contours a cell by processing the face between its children.
fn contour_cell(
    node: &KdTreeV2Node,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    if node.is_contouring_leaf(threshold) {
        return;
    }

    if let (Some(left), Some(right)) = (&node.children[0], &node.children[1]) {
        let face_nodes: [&KdTreeV2Node; 2] = [left, right];
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

/// Checks whether two face nodes share a valid minimal face region.
///
/// Returns the face min/max codes if the overlap has positive extent in
/// both directions perpendicular to `dir`.
fn check_minimal_face(
    nodes: [&KdTreeV2Node; 2],
    dir: usize,
) -> Option<(PositionCode, PositionCode)> {
    let face_max = nodes[0].grid.max_code.min(nodes[1].grid.max_code);
    let face_min = nodes[0].grid.min_code.max(nodes[1].grid.min_code);
    let offset = face_max - face_min;
    if offset[(dir + 1) % 3] > 0 && offset[(dir + 2) % 3] > 0 {
        Some((face_min, face_max))
    } else {
        None
    }
}

/// Checks whether four edge nodes share a valid minimal edge region.
///
/// Returns the edge min/max endpoint codes if the overlap has positive
/// extent along the edge direction.
fn check_minimal_edge(
    nodes: [&KdTreeV2Node; 4],
    line: &AALine,
) -> Option<(PositionCode, PositionCode)> {
    let mut min_end = line.point;
    let mut max_end = line.point;
    let dir = line.dir;
    min_end[dir] = nodes
        .iter()
        .map(|n| n.grid.min_code[dir])
        .fold(i32::MIN, |a, b| a.max(b));
    max_end[dir] = nodes
        .iter()
        .map(|n| n.grid.max_code[dir])
        .fold(i32::MAX, |a, b| a.min(b));
    if min_end[dir] < max_end[dir] {
        Some((min_end, max_end))
    } else {
        None
    }
}

/// Constructs an axis-aligned line at the intersection of two splitting planes.
///
/// The line lies at the face between `face_nodes` (at coordinate `axis` along
/// `origin_face_dir`) and is perpendicular to both the face direction and the
/// non-leaf node's split direction.
fn construct_line(
    face_nodes: [&KdTreeV2Node; 2],
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

/// Computes the child index to follow when walking toward an edge.
fn next_quad_index(dir1: usize, dir2: usize, plane_dir: usize, i: usize) -> usize {
    let mut pos = PositionCode::ZERO;
    pos[dir1] = 1 - (i % 2) as i32;
    pos[dir2] = 1 - (i / 2) as i32;
    pos[plane_dir] as usize
}

/// Processes the face between two adjacent nodes, recursing into sub-faces
/// and emitting edges where splitting planes intersect.
///
/// Phase 1: Walks each node toward the face by following same-axis children.
/// Phase 2: Finds the first non-leaf node, recurses into its children for
/// sub-faces, and emits an edge at the split plane if it lies within bounds.
#[allow(clippy::too_many_arguments)]
fn contour_face(
    mut nodes: [&KdTreeV2Node; 2],
    dir: usize,
    axis: i32,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    if nodes[0].is_contouring_leaf(threshold) && nodes[1].is_contouring_leaf(threshold) {
        return;
    }

    let (face_min, face_max) = match check_minimal_face(nodes, dir) {
        Some(v) => v,
        None => return,
    };

    // Phase 1: walk each node toward the shared face along the same axis
    for (i, node) in nodes.iter_mut().enumerate() {
        while !node.is_contouring_leaf(threshold) && node.plane_dir == dir {
            match &node.children[1 - i] {
                Some(c) => *node = c,
                None => return,
            }
        }
    }

    // Phase 2: find first non-leaf, recurse into its children
    for i in 0..2 {
        if !nodes[i].is_contouring_leaf(threshold) {
            for j in 0..2 {
                if let Some(child) = &nodes[i].children[j] {
                    let mut next_face = nodes;
                    next_face[i] = child;
                    contour_face(next_face, dir, axis, mesh, field, threshold, unit_size);
                }
            }
            if nodes[i].axis() > face_min[nodes[i].plane_dir]
                && nodes[i].axis() < face_max[nodes[i].plane_dir]
            {
                let mut edge_nodes: [Option<&KdTreeV2Node>; 4] = [
                    Some(nodes[0]),
                    Some(nodes[0]),
                    Some(nodes[1]),
                    Some(nodes[1]),
                ];
                edge_nodes[i * 2] = nodes[i].children[0].as_deref();
                edge_nodes[i * 2 + 1] = nodes[i].children[1].as_deref();
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

/// Splits shared quad nodes that straddle the edge until they are resolved.
///
/// For each pair of nodes sharing the same pointer, walks down the tree
/// splitting them based on which side of the edge they fall on.
fn detect_quad(nodes: &mut [Option<&KdTreeV2Node>; 4], line: &AALine, threshold: f32) {
    for i in 0..2 {
        while let (Some(a), Some(b)) = (nodes[i * 2], nodes[i * 2 + 1]) {
            if !std::ptr::eq(a, b) || a.is_contouring_leaf(threshold) || a.plane_dir == line.dir {
                break;
            }
            let common = a;
            if common.axis() == line.point[common.plane_dir] {
                nodes[i * 2] = common.children[0].as_deref();
                nodes[i * 2 + 1] = common.children[1].as_deref();
            } else if common.axis() > line.point[common.plane_dir] {
                let c = common.children[0].as_deref();
                nodes[i * 2] = c;
                nodes[i * 2 + 1] = c;
            } else {
                let c = common.children[1].as_deref();
                nodes[i * 2] = c;
                nodes[i * 2 + 1] = c;
            }
        }
    }
}

/// Updates a quad node, propagating the change to the opposite node if they
/// currently point to the same location.
fn set_quad_node<'a>(
    nodes: &mut [Option<&'a KdTreeV2Node>; 4],
    i: usize,
    new_node: Option<&'a KdTreeV2Node>,
) {
    let opp = opposite_quad_index(i);
    match (nodes[opp], nodes[i]) {
        (Some(a), Some(b)) if std::ptr::eq(a, b) => {
            nodes[opp] = new_node;
        }
        (None, None) => {
            nodes[opp] = new_node;
        }
        _ => {}
    }
    nodes[i] = new_node;
}

/// Processes an edge shared by four quad nodes, generating quads where
/// the edge crosses the isosurface.
///
/// First resolves shared nodes via `detect_quad`, then walks non-equal nodes
/// toward the edge. If all four nodes are contouring leaves, emits a quad.
/// Otherwise, finds the first node split along the edge direction and recurses.
#[allow(clippy::too_many_arguments)]
fn contour_edge(
    mut nodes: [Option<&KdTreeV2Node>; 4],
    line: &AALine,
    quad_dir1: usize,
    field: &dyn ScalarField,
    threshold: f32,
    mesh: &mut IsoMesh,
    unit_size: f32,
) {
    detect_quad(&mut nodes, line, threshold);

    let n0 = match nodes[0] {
        Some(n) => n,
        None => return,
    };
    let n1 = match nodes[1] {
        Some(n) => n,
        None => return,
    };
    let n2 = match nodes[2] {
        Some(n) => n,
        None => return,
    };
    let n3 = match nodes[3] {
        Some(n) => n,
        None => return,
    };

    let quad_dir2 = 3 - quad_dir1 - line.dir;

    if check_minimal_edge([n0, n1, n2, n3], line).is_none() {
        return;
    }

    // Walk non-equal nodes toward the edge
    let mut unwrapped: [&KdTreeV2Node; 4] = [n0, n1, n2, n3];
    for i in 0..4 {
        if !std::ptr::eq(unwrapped[i], unwrapped[opposite_quad_index(i)]) {
            while !unwrapped[i].is_contouring_leaf(threshold) && unwrapped[i].plane_dir != line.dir
            {
                let child_idx = next_quad_index(quad_dir1, quad_dir2, unwrapped[i].plane_dir, i);
                match &unwrapped[i].children[child_idx] {
                    Some(c) => unwrapped[i] = c,
                    None => return,
                }
            }
        }
    }

    // If all four are contouring leaves, emit a quad
    if unwrapped[0].is_contouring_leaf(threshold)
        && unwrapped[1].is_contouring_leaf(threshold)
        && unwrapped[2].is_contouring_leaf(threshold)
        && unwrapped[3].is_contouring_leaf(threshold)
    {
        kd_generate_quad(
            unwrapped, quad_dir1, quad_dir2, mesh, field, threshold, unit_size,
        );
        return;
    }

    // Find the first node split along the edge direction and recurse
    let opt_nodes: [Option<&KdTreeV2Node>; 4] = [
        Some(unwrapped[0]),
        Some(unwrapped[1]),
        Some(unwrapped[2]),
        Some(unwrapped[3]),
    ];
    for i in 0..4 {
        if let Some(n) = opt_nodes[i]
            && !n.is_contouring_leaf(threshold)
            && n.plane_dir == line.dir
        {
            let mut next_nodes = opt_nodes;
            set_quad_node(&mut next_nodes, i, n.children[0].as_deref());
            contour_edge(
                next_nodes, line, quad_dir1, field, threshold, mesh, unit_size,
            );

            let mut next_nodes = opt_nodes;
            set_quad_node(&mut next_nodes, i, n.children[1].as_deref());
            contour_edge(
                next_nodes, line, quad_dir1, field, threshold, mesh, unit_size,
            );
            return;
        }
    }
}

/// Emits a quad from four contouring-leaf nodes surrounding an edge.
///
/// Delegates to the shared `generate_quad` implementation in `rectilinear_grid`.
#[allow(clippy::too_many_arguments)]
fn kd_generate_quad(
    nodes: [&KdTreeV2Node; 4],
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
    use crate::isosurface::scalar_field::Sphere;

    #[test]
    fn build_kdtree_v2_sphere_produces_triangles() {
        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let sphere = Sphere::with_center(3.0, glam::Vec3::new(4.0, 4.0, 4.0));
                let unit_size = 1.0;
                let depth = 4;
                let size_code = PositionCode::splat(1 << (depth - 1));
                let min_code = PositionCode::splat(0);
                let max_code = size_code;

                let octree =
                    OctreeNode::build_with_scalar_field(min_code, depth, &sphere, true, unit_size);
                assert!(octree.is_some(), "Octree should not be empty for a sphere");
                let octree = match octree {
                    Some(o) => o,
                    None => return,
                };

                let kdtree = KdTreeV2Node::build_from_octree(
                    &octree, min_code, max_code, &sphere, 0, unit_size,
                );
                assert!(
                    kdtree.is_some(),
                    "KdTreeV2 should not be empty for a sphere"
                );
                let mut kdtree = match kdtree {
                    Some(k) => k,
                    None => return,
                };

                let mesh = KdTreeV2Node::extract_mesh(&mut kdtree, &sphere, 0.0, unit_size);

                assert!(
                    !mesh.positions.is_empty(),
                    "KdTreeV2 mesh should have vertices"
                );
                assert!(
                    !mesh.indices.is_empty(),
                    "KdTreeV2 mesh should have indices"
                );

                let triangles = mesh.triangle_count();
                assert!(triangles > 0, "KdTreeV2 mesh should have triangles, got 0");
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
    fn kdtree_v2_leaf_node() {
        let qef = QefSolver::new();
        let grid = RectilinearGrid::new(PositionCode::splat(0), PositionCode::splat(2), qef, 1.0);
        let node = KdTreeV2Node {
            grid,
            plane_dir: 0,
            depth: 0,
            clusterable: true,
            children: [None, None],
        };
        assert!(node.is_leaf());
        assert!(node.is_contouring_leaf(1.0));
    }
}
