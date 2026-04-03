//! K-d tree based dual contouring for isosurface extraction.
//!
//! Builds a binary space partition tree from an octree, producing fewer
//! triangles than the octree alone for the same error threshold. This is a
//! port of the KdtreeISO algorithm.

use super::indicators::{PositionCode, code_to_pos};
use super::mesh_output::IsoMesh;
use super::octree::OctreeNode;
use super::qef::QefSolver;
use super::rectilinear_grid::{HasGrid, RectilinearGrid, check_sign, generate_quad};
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
        if !self.clusterable {
            return false;
        }
        for v in &self.grid.vertices {
            if v.error > threshold {
                return false;
            }
        }
        self.grid.approximate.error <= threshold
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

        for child in self.children.iter().flatten() {
            if !child.clusterable {
                self.clusterable = false;
                return;
            }
        }

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

        self.clusterable = true;
    }

    /// Chooses the best split axis using variance-weighted selection.
    ///
    /// Selects the axis with the highest weighted variance from the QEF,
    /// ensuring the cell is at least 2 units wide along that axis.
    fn choose_axis_dir(qef: &QefSolver, min_code: PositionCode, max_code: PositionCode) -> usize {
        let size = max_code - min_code;
        let mass_point = qef.mass_point();
        let variance = qef.get_variance(mass_point);

        let mut best_axis = 0;
        let mut best_score = f32::NEG_INFINITY;

        for axis in 0..3 {
            if size[axis] < 2 {
                continue;
            }
            let score = variance[axis] * size[axis] as f32;
            if score > best_score {
                best_score = score;
                best_axis = axis;
            }
        }

        // Fallback: if no axis has size >= 2, pick the largest
        if best_score == f32::NEG_INFINITY {
            for axis in 0..3 {
                let s = size[axis] as f32;
                if s > best_score {
                    best_score = s;
                    best_axis = axis;
                }
            }
        }

        best_axis
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
        let size = max_code - min_code;

        if size.x <= 1 && size.y <= 1 && size.z <= 1 {
            return None;
        }
        if depth > 64 {
            return None;
        }

        // Accumulate QEF from octree for this region
        let min_pos = code_to_pos(min_code, unit_size);
        let max_pos = code_to_pos(max_code, unit_size);
        let qef = OctreeNode::get_sum(octree, min_pos, max_pos, unit_size);

        if qef.point_count() <= 0 {
            return None;
        }

        let grid = RectilinearGrid::new(min_code, max_code, qef, unit_size);
        let mut node = Box::new(KdTreeNode {
            grid,
            plane_dir: 0,
            depth,
            clusterable: false,
            children: [None, None],
        });

        // Assign signs from the field
        node.grid.assign_sign(field, unit_size);

        // At leaf-sized cells, bail if no sign change
        let is_leaf_size = size.x <= 2 && size.y <= 2 && size.z <= 2;
        if is_leaf_size && !node.grid.is_signed {
            return None;
        }

        // Compute corner components and QEF for signed cells
        if node.grid.is_signed {
            node.grid.cal_corner_components();

            let mut all_qef = QefSolver::new();
            node.grid.sample_qef(field, &mut all_qef, unit_size);
            node.grid.all_qef = all_qef;

            for i in 0..node.grid.components.len() {
                node.grid.solve_component(i, unit_size);
            }
            Self::solve_approximate(&mut node.grid, unit_size);
        }

        // Check if further splitting is possible
        let has_min_size = size.x >= 2 || size.y >= 2 || size.z >= 2;

        if !has_min_size {
            node.clusterable = true;
            return if node.grid.is_signed {
                Some(node)
            } else {
                None
            };
        }

        // Choose split axis
        let axis = Self::choose_axis_dir(&node.grid.all_qef, min_code, max_code);
        node.plane_dir = axis;

        // Find split plane via binary search
        let split = Self::find_split_plane(octree, min_code, max_code, axis, unit_size);

        // Build children
        let mut left_max = max_code;
        left_max[axis] = split;

        let mut right_min = min_code;
        right_min[axis] = split;

        if split > min_code[axis] && split < max_code[axis] {
            node.children[0] =
                Self::build_from_octree(octree, min_code, left_max, field, depth + 1, unit_size);
            node.children[1] =
                Self::build_from_octree(octree, right_min, max_code, field, depth + 1, unit_size);
        }

        // If no children were created and cell is not signed, nothing here
        if node.is_leaf() && !node.grid.is_signed {
            return None;
        }

        // Calculate clusterability
        node.cal_clusterability(field, unit_size);

        // Combine QEF from children if clusterable
        if node.clusterable && !node.is_leaf() {
            node.combine_qef(unit_size);
        }

        Some(node)
    }

    /// Finds the optimal split plane along an axis using binary search.
    fn find_split_plane(
        octree: &OctreeNode,
        min_code: PositionCode,
        max_code: PositionCode,
        axis: usize,
        unit_size: f32,
    ) -> i32 {
        let lo = min_code[axis];
        let hi = max_code[axis];

        if hi - lo <= 1 {
            return lo + 1;
        }

        let mut best_split = (lo + hi) / 2;
        let mut best_diff = f32::MAX;

        let mut search_lo = lo + 1;
        let mut search_hi = hi;

        for _ in 0..16 {
            if search_lo >= search_hi {
                break;
            }
            let mid = (search_lo + search_hi) / 2;

            let mut left_max = max_code;
            left_max[axis] = mid;
            let mut right_min = min_code;
            right_min[axis] = mid;

            let left_qef = OctreeNode::get_sum(
                octree,
                code_to_pos(min_code, unit_size),
                code_to_pos(left_max, unit_size),
                unit_size,
            );
            let right_qef = OctreeNode::get_sum(
                octree,
                code_to_pos(right_min, unit_size),
                code_to_pos(max_code, unit_size),
                unit_size,
            );

            let diff = (left_qef.point_count() - right_qef.point_count()).abs() as f32;
            if diff < best_diff {
                best_diff = diff;
                best_split = mid;
            }

            if left_qef.point_count() < right_qef.point_count() {
                search_lo = mid + 1;
            } else {
                search_hi = mid;
            }
        }

        best_split
    }

    /// Solves the approximate vertex from the all_qef, clamped to the cell.
    fn solve_approximate(grid: &mut RectilinearGrid, unit_size: f32) {
        let min_c = grid.min_code;
        let max_c = grid.max_code;
        let (mut pos, error) = grid.all_qef.solve();
        let min_pos = code_to_pos(min_c, unit_size);
        let max_pos = code_to_pos(max_c, unit_size);
        pos = pos.clamp(min_pos, max_pos);
        if error > 1.0e-2 {
            let mp = grid.all_qef.mass_point();
            pos = mp.clamp(min_pos, max_pos);
        }
        grid.approximate.hermite_p = pos;
        grid.approximate.error = error;
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
    if node.is_contouring_leaf(threshold) {
        for v in &mut node.grid.vertices {
            mesh.add_vertex(v, |p| field.normal(p));
        }
        return;
    }

    for child in node.children.iter_mut().flatten() {
        generate_vertex_indices(child, threshold, field, mesh);
    }
}

/// Cell procedure: recurse into children, then contour the shared face.
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

    for child in node.children.iter().flatten() {
        contour_cell(child, mesh, field, threshold, unit_size);
    }

    if let (Some(left), Some(right)) = (&node.children[0], &node.children[1]) {
        contour_face(
            left,
            right,
            node.plane_dir,
            mesh,
            field,
            threshold,
            unit_size,
        );
    }
}

/// Face procedure: given two adjacent nodes sharing a face along `dir`,
/// recursively descend to find leaf-leaf pairs and generate quads.
#[allow(clippy::too_many_arguments)]
fn contour_face(
    n0: &KdTreeNode,
    n1: &KdTreeNode,
    dir: usize,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    let n0_leaf = n0.is_contouring_leaf(threshold);
    let n1_leaf = n1.is_contouring_leaf(threshold);

    if n0_leaf && n1_leaf {
        // Both leaves: generate quads for sign-changing edges on this face
        generate_face_quads(n0, n1, dir, mesh, field, threshold, unit_size);
        return;
    }

    // Descend n0 if it splits along dir
    if !n0_leaf
        && n0.plane_dir == dir
        && let (Some(c0), Some(c1)) = (&n0.children[0], &n0.children[1])
    {
        contour_face(c1, n1, dir, mesh, field, threshold, unit_size);
        contour_face(c0, c1, dir, mesh, field, threshold, unit_size);
        return;
    }

    // Descend n1 if it splits along dir
    if !n1_leaf
        && n1.plane_dir == dir
        && let (Some(c0), Some(c1)) = (&n1.children[0], &n1.children[1])
    {
        contour_face(n0, c0, dir, mesh, field, threshold, unit_size);
        contour_face(c0, c1, dir, mesh, field, threshold, unit_size);
        return;
    }

    // Descend n0 if it splits perpendicular to dir
    if !n0_leaf && let (Some(l0), Some(l1)) = (&n0.children[0], &n0.children[1]) {
        contour_face(l0, n1, dir, mesh, field, threshold, unit_size);
        contour_face(l1, n1, dir, mesh, field, threshold, unit_size);
        return;
    }

    // Descend n1 if it splits perpendicular to dir
    if !n1_leaf && let (Some(r0), Some(r1)) = (&n1.children[0], &n1.children[1]) {
        contour_face(n0, r0, dir, mesh, field, threshold, unit_size);
        contour_face(n0, r1, dir, mesh, field, threshold, unit_size);
    }
}

/// Generates quads for sign-changing edges on the face between two leaf nodes.
///
/// For each edge on the shared face that has a sign change, emits a quad
/// using the two leaf nodes as two of the four cells. The other two cells
/// are the same two nodes (since in dual contouring, each cell contributes
/// one vertex per component).
#[allow(clippy::too_many_arguments)]
fn generate_face_quads(
    n0: &KdTreeNode,
    n1: &KdTreeNode,
    dir: usize,
    mesh: &mut IsoMesh,
    field: &dyn ScalarField,
    threshold: f32,
    unit_size: f32,
) {
    if !n0.grid.is_signed && !n1.grid.is_signed {
        return;
    }

    // The face between n0 and n1 is perpendicular to `dir`.
    // For each of the two perpendicular directions, check edges along
    // the face. In a k-d tree, since cells can be large, we use the
    // cells themselves as all 4 nodes around each edge (duplicated).
    let quad_dir1 = dir;
    let other_dirs = [(dir + 1) % 3, (dir + 2) % 3];
    let quad_dir2 = other_dirs[0];

    let has_grid: [&dyn HasGrid; 4] = [n0, n0, n1, n1];
    let refs: Vec<&dyn HasGrid> = has_grid.to_vec();
    if check_sign(&refs, quad_dir1, quad_dir2, field, unit_size).is_some() {
        generate_quad(
            &has_grid, quad_dir1, quad_dir2, mesh, field, threshold, unit_size,
        );
    }

    let quad_dir2b = other_dirs[1];
    let has_grid2: [&dyn HasGrid; 4] = [n0, n0, n1, n1];
    let refs2: Vec<&dyn HasGrid> = has_grid2.to_vec();
    if check_sign(&refs2, quad_dir1, quad_dir2b, field, unit_size).is_some() {
        generate_quad(
            &has_grid2, quad_dir1, quad_dir2b, mesh, field, threshold, unit_size,
        );
    }
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
                let min_code = PositionCode::splat(0);
                let max_code = PositionCode::splat(1 << depth);

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
