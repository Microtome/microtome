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
        // C++: sizeCode = 1 << (depth - 1)
        let size = 1_i32 << (depth - 1);
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

        // C++: assignSign is called for ALL nodes (leaf and internal)
        node.grid.assign_sign(field, unit_size);

        if depth == 1 {
            // Leaf level
            if !node.grid.is_signed {
                return None;
            }
            // C++: sampleQef calls calCornerComponents internally
            node.grid.cal_corner_components();
            let mut all_qef = QefSolver::new();
            node.grid.sample_qef(field, &mut all_qef, unit_size);
            node.grid.all_qef = all_qef;
            node.is_leaf = true;
            node.clusterable = true; // C++: clusterable defaults to true
        } else {
            // Internal node: recursively build 8 children
            // C++: subSizeCode = 1 << (depth - 2)
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
                let child = Self::build_with_scalar_field(
                    child_min,
                    depth - 1,
                    field,
                    as_mipmap,
                    unit_size,
                );
                if let Some(mut c) = child {
                    c.child_index = i as i8;
                    // C++: accumulate allQef during child loop
                    node.grid.all_qef.combine(&c.grid.all_qef);
                    node.children[i] = Some(c);
                    any_child = true;
                }
            }

            if !any_child {
                return None;
            }

            Self::cal_clusterability(&mut node, field, unit_size);

            if node.clusterable && !as_mipmap {
                node.combine_components(field, unit_size);
            }
            node.is_leaf = false;
        }

        // Solve components and approximate (C++: only solve components when !as_mipmap)
        if !as_mipmap {
            for i in 0..node.grid.components.len() {
                node.grid.solve_component(i, unit_size);
            }
        }
        Self::solve_approximate(&mut node.grid, unit_size);

        Some(node)
    }

    /// Accumulates QEF data from the octree within the given bounding box.
    ///
    /// Matches C++ `Octree::getSum` exactly: uses integer PositionCode
    /// comparisons and clamps the query bounds to the node bounds before
    /// recursing into children.
    pub fn get_sum(
        root: &OctreeNode,
        min_pos: PositionCode,
        max_pos: PositionCode,
        out: &mut QefSolver,
    ) {
        // C++: if (any(greaterThanEqual(minPos, maxPos))) return;
        if min_pos.x >= max_pos.x || min_pos.y >= max_pos.y || min_pos.z >= max_pos.z {
            return;
        }
        // C++: no overlap check
        if min_pos.x >= root.grid.max_code.x
            || min_pos.y >= root.grid.max_code.y
            || min_pos.z >= root.grid.max_code.z
            || max_pos.x <= root.grid.min_code.x
            || max_pos.y <= root.grid.min_code.y
            || max_pos.z <= root.grid.min_code.z
        {
            return;
        }
        // C++: clamp to node bounds
        let min_bound = root.grid.min_code.max(min_pos);
        let max_bound = root.grid.max_code.min(max_pos);
        // C++: fully contained check
        if min_bound == root.grid.min_code && max_bound == root.grid.max_code {
            out.combine(&root.grid.all_qef);
            return;
        }
        // C++: recurse into octree children with clamped bounds
        for c in root.children.iter().flatten() {
            Self::get_sum(c, min_bound, max_bound, out);
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
    /// Matches C++ `Octree::calClusterbility` exactly:
    /// 1. If any present child is not clusterable, this node is not.
    /// 2. For each of the 12 face pairs, compute proper bounds spanning
    ///    the two children and call `RectilinearGrid::cal_clusterability`.
    fn cal_clusterability(node: &mut OctreeNode, field: &dyn ScalarField, unit_size: f32) {
        // All present children must be clusterable
        for c in node.children.iter().flatten() {
            if !c.clusterable {
                node.clusterable = false;
                return;
            }
        }

        // Check face-pair clusterability across the 12 face pairs
        // C++: compute halfSize and per-pair bounds
        let half_size = (node.grid.max_code - node.grid.min_code) / 2;

        for mask in &CELL_PROC_FACE_MASK {
            let left_index = mask[0];
            let right_index = mask[1];
            let dir = mask[2];

            let left = node.children[left_index].as_ref().map(|c| &c.grid);
            let right = node.children[right_index].as_ref().map(|c| &c.grid);

            // C++: minCode = grid.minCode + decodeCell(leftIndex) * halfSize
            //      maxCode = grid.minCode + halfSize + decodeCell(rightIndex) * halfSize
            let left_decode = super::indicators::decode_cell(left_index);
            let right_decode = super::indicators::decode_cell(right_index);
            let pair_min = node.grid.min_code + left_decode * half_size;
            let pair_max = node.grid.min_code + half_size + right_decode * half_size;

            if !RectilinearGrid::cal_clusterability(
                left, right, dir, pair_min, pair_max, field, unit_size,
            ) {
                node.clusterable = false;
                return;
            }
        }

        node.clusterable = true;
    }

    /// Hierarchically combines child grid components along Z, Y, then X axes.
    ///
    /// Matches C++ `Octree::combineComponents` exactly:
    /// - For x in 0..2, for y in 0..2: z-merge children (x,y,0) and (x,y,1)
    /// - Then y-merge the z-results
    /// - Then x-merge the y-results into this node's grid
    /// - Each intermediate grid calls assign_sign and accumulates allQef
    /// - Final MC edge case check + parent pointer path compression
    fn combine_components(&mut self, field: &dyn ScalarField, unit_size: f32) {
        let half_size = (self.grid.max_code - self.grid.min_code) / 2;

        // C++ uses ygridPool[4] and xgridPool[2]
        let mut y_grid_pool: [Option<RectilinearGrid>; 4] = Default::default();
        let mut x_grid_pool: [Option<RectilinearGrid>; 2] = Default::default();

        for x in 0..2 {
            // C++: yMinCode = PositionCode(x, 0, 0) * halfSize + grid.minCode
            //      yMaxCode = PositionCode(x, 1, 1) * halfSize + halfSize + grid.minCode
            let y_min_code = PositionCode::new(x, 0, 0) * half_size + self.grid.min_code;
            let y_max_code =
                PositionCode::new(x, 1, 1) * half_size + half_size + self.grid.min_code;

            let mut y_grids: [Option<usize>; 2] = [None; 2]; // indices into y_grid_pool

            for y in 0..2 {
                // C++: zMinCode = PositionCode(x, y, 0) * halfSize + grid.minCode
                //      zMaxCode = PositionCode(x, y, 1) * halfSize + halfSize + grid.minCode
                let z_min_code = PositionCode::new(x, y, 0) * half_size + self.grid.min_code;
                let z_max_code =
                    PositionCode::new(x, y, 1) * half_size + half_size + self.grid.min_code;

                let l_idx = encode_cell(glam::IVec3::new(x, y, 0));
                let r_idx = encode_cell(glam::IVec3::new(x, y, 1));
                let l = self.children[l_idx].as_ref().map(|c| &c.grid);
                let r = self.children[r_idx].as_ref().map(|c| &c.grid);

                if l.is_none() && r.is_none() {
                    continue;
                }

                let pool_idx = (x as usize) * 2 + (y as usize);
                let mut out =
                    RectilinearGrid::new(z_min_code, z_max_code, QefSolver::new(), unit_size);
                // C++: assignSign and accumulate allQef before combineAAGrid
                out.assign_sign(field, unit_size);
                if let Some(lg) = l {
                    out.all_qef.combine(&lg.all_qef);
                }
                if let Some(rg) = r {
                    out.all_qef.combine(&rg.all_qef);
                }
                RectilinearGrid::combine_aa_grid(l, r, 2, &mut out);
                y_grid_pool[pool_idx] = Some(out);
                y_grids[y as usize] = Some(pool_idx);
            }

            // Y-merge
            let yg0 = y_grids[0].and_then(|i| y_grid_pool[i].as_ref());
            let yg1 = y_grids[1].and_then(|i| y_grid_pool[i].as_ref());

            if yg0.is_none() && yg1.is_none() {
                continue;
            }

            let mut out = RectilinearGrid::new(y_min_code, y_max_code, QefSolver::new(), unit_size);
            out.assign_sign(field, unit_size);
            if let Some(g) = yg0 {
                out.all_qef.combine(&g.all_qef);
            }
            if let Some(g) = yg1 {
                out.all_qef.combine(&g.all_qef);
            }
            RectilinearGrid::combine_aa_grid(yg0, yg1, 1, &mut out);
            x_grid_pool[x as usize] = Some(out);
        }

        // X-merge (final merge into self.grid)
        // C++: combineAAGrid(xgrids[0], xgrids[1], 0, &grid)
        // self.grid already has assignSign called (from build_with_scalar_field)
        let xg0 = x_grid_pool[0].as_ref();
        let xg1 = x_grid_pool[1].as_ref();
        RectilinearGrid::combine_aa_grid(xg0, xg1, 0, &mut self.grid);

        // C++ MC edge case: check if combined point counts match
        let mut count = 0;
        for c in &self.grid.components {
            count += c.point_count();
        }
        if count != self.grid.all_qef.point_count() {
            self.clusterable = false;
            // Null out all child parent pointers
            for ci in 0..8 {
                if let Some(child) = &mut self.children[ci] {
                    for v in &mut child.grid.vertices {
                        v.parent = None;
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
    ///
    /// Matches C++ `contourCell` exactly: passes potentially-null children
    /// to contour_face and contour_edge via Option.
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

        // Process 12 face pairs — pass children directly (may be None)
        for mask in &CELL_PROC_FACE_MASK {
            let n0 = node.children[mask[0]].as_deref();
            let n1 = node.children[mask[1]].as_deref();
            let dir = mask[2];
            Self::contour_face([n0, n1], dir, mesh, field, threshold, unit_size, node.depth);
        }

        // Process 6 edge groups — pass children directly (may be None)
        for mask in &CELL_PROC_EDGE_MASK {
            let dir = mask[4];
            let nodes: [Option<&OctreeNode>; 4] = [
                node.children[mask[0]].as_deref(),
                node.children[mask[1]].as_deref(),
                node.children[mask[2]].as_deref(),
                node.children[mask[3]].as_deref(),
            ];
            let quad_dir2 = (dir + 2) % 3;
            Self::contour_edge(
                nodes, dir, quad_dir2, mesh, field, threshold, unit_size, node.depth,
            );
        }
    }

    /// Face procedure: subdivide across a shared face between two nodes.
    ///
    /// Matches C++ `contourFace`: if either node is None, return immediately.
    /// When subdividing, a non-leaf node's child may be None — propagated
    /// through to the recursive call.
    #[allow(clippy::too_many_arguments)]
    fn contour_face(
        nodes: [Option<&OctreeNode>; 2],
        dir: usize,
        mesh: &mut IsoMesh,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
        max_depth: u32,
    ) {
        // C++: if (!nodes[0] || !nodes[1]) return;
        let n0 = match nodes[0] {
            Some(n) => n,
            None => return,
        };
        let n1 = match nodes[1] {
            Some(n) => n,
            None => return,
        };
        if n0.is_leaf && n1.is_leaf {
            return;
        }
        if max_depth == 0 {
            return;
        }

        // Subdivide into 4 sub-faces
        for mask in &FACE_PROC_FACE_MASK[dir] {
            let child0_idx = mask[0];
            let child1_idx = mask[1];

            // C++: if (!subdivision_face[j]->isLeaf) { subdivision_face[j] = children[...]; }
            let sub0 = if n0.is_leaf {
                Some(n0)
            } else {
                n0.children[child0_idx].as_deref()
            };
            let sub1 = if n1.is_leaf {
                Some(n1)
            } else {
                n1.children[child1_idx].as_deref()
            };

            Self::contour_face(
                [sub0, sub1],
                dir,
                mesh,
                field,
                threshold,
                unit_size,
                max_depth - 1,
            );
        }

        // Process 4 edges along this face
        // C++ faceNodeOrder = {0, 0, 1, 1}
        let face_nodes = [n0, n1];
        for mask in &FACE_PROC_EDGE_MASK[dir] {
            let c = [mask[1], mask[2], mask[3], mask[4]];
            let edge_dir = mask[5];
            let order = [0usize, 0, 1, 1]; // faceNodeOrder

            let mut edge_nodes: [Option<&OctreeNode>; 4] = [None; 4];
            for j in 0..4 {
                let src = face_nodes[order[j]];
                if src.is_leaf {
                    edge_nodes[j] = Some(src);
                } else {
                    edge_nodes[j] = src.children[c[j]].as_deref();
                }
            }

            Self::contour_edge(
                edge_nodes,
                edge_dir,
                dir, // quadDir2 = face direction
                mesh,
                field,
                threshold,
                unit_size,
                max_depth - 1,
            );
        }
    }

    /// Edge procedure: either generate a quad or subdivide further.
    ///
    /// Matches C++ `contourEdge`: if any node is None, return immediately.
    /// When subdividing, a non-leaf node's child may be None.
    #[allow(clippy::too_many_arguments)]
    fn contour_edge(
        nodes: [Option<&OctreeNode>; 4],
        dir: usize,
        quad_dir2: usize,
        mesh: &mut IsoMesh,
        field: &dyn ScalarField,
        threshold: f32,
        unit_size: f32,
        max_depth: u32,
    ) {
        // C++: if (!nodes[0] || !nodes[1] || !nodes[2] || !nodes[3]) return;
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

        if n0.is_leaf && n1.is_leaf && n2.is_leaf && n3.is_leaf {
            Self::generate_quad_from_nodes(
                &[n0, n1, n2, n3],
                dir,
                quad_dir2,
                mesh,
                field,
                threshold,
                unit_size,
            );
            return;
        }
        if max_depth == 0 {
            return;
        }

        // Subdivide: matching C++ exactly
        let quad_dir1 = 3 - dir - quad_dir2;
        let all = [n0, n1, n2, n3];
        for i in 0i32..2 {
            let mut sub_nodes: [Option<&OctreeNode>; 4] = [Some(n0), Some(n1), Some(n2), Some(n3)];

            for j in 0..4usize {
                if !all[j].is_leaf {
                    let mut code = glam::IVec3::ZERO;
                    code[dir] = i;
                    let ji = j as i32;
                    code[quad_dir1] = (3 - ji) % 2;
                    code[quad_dir2] = (3 - ji) / 2;
                    let child_idx = encode_cell(code);
                    // C++: subdivision_edge[j] = nodes[j]->children[...]; (can be null)
                    sub_nodes[j] = all[j].children[child_idx].as_deref();
                }
            }

            Self::contour_edge(
                sub_nodes,
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
                let sphere = Sphere::with_center(3.0, glam::Vec3::new(4.0, 4.0, 4.0));
                let unit_size = 1.0;
                let depth = 4; // sizeCode = 1 << 3 = 8 → grid 0..8
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

    fn count_clusterable(node: &OctreeNode) -> (usize, usize) {
        if node.is_leaf {
            return (0, 0); // leaves don't count
        }
        let mut total = 1usize;
        let mut clusterable = if node.clusterable { 1usize } else { 0 };
        for c in node.children.iter().flatten() {
            let (ct, cc) = count_clusterable(c);
            total += ct;
            clusterable += cc;
        }
        (total, clusterable)
    }

    #[test]
    fn aabb_simplification_works() {
        use crate::isosurface::scalar_field::Aabb;
        let result = std::thread::Builder::new()
            .stack_size(16 * 1024 * 1024)
            .spawn(|| {
                // Simple box — no cylinder — so flat faces are easy to simplify
                let field = Aabb::new(glam::Vec3::splat(-3.0), glam::Vec3::splat(3.0));
                let depth = 5_u32;
                // size_code = 16, unit_size = 0.5 → world [-8, 8], box [-3, 3] fits
                let size_code = glam::IVec3::splat(1 << (depth - 1));
                let unit_size = 0.5;
                let min_code = -size_code / 2;

                let root = OctreeNode::build_with_scalar_field(
                    min_code, depth, &field, false, unit_size,
                );
                assert!(root.is_some(), "Octree should not be empty for box-cylinder");
                let mut root = root.unwrap_or_else(|| unreachable!());

                let (total_internal, num_clusterable) = count_clusterable(&root);
                let leaves_before = count_leaves(&root);
                eprintln!(
                    "Before simplify: {leaves_before} leaves, {total_internal} internal, {num_clusterable} clusterable"
                );
                assert!(
                    num_clusterable > 0,
                    "Some internal nodes should be clusterable for a box scene, got 0/{total_internal}"
                );

                OctreeNode::simplify(&mut root, 1.0);
                let leaves_after = count_leaves(&root);
                eprintln!("After simplify: {leaves_after} leaves (was {leaves_before})");
                assert!(
                    leaves_after < leaves_before,
                    "Simplify should reduce leaves: before={leaves_before}, after={leaves_after}"
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
}
