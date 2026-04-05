// K-d tree based dual contouring for isosurface extraction.
//
// Ported line-by-line from KdtreeISO-master/src/KdtreeISO/lib/Kdtree.cpp
// and KdtreeISO-master/src/KdtreeISO/include/Kdtree.h

use glam::Vec3;
use super::indicators::{PositionCode, opposite_quad_index};
use super::mesh_output::IsoMesh;
use super::octree::OctreeNode;
use super::qef::QefSolver;
use super::rectilinear_grid::{HasGrid, RectilinearGrid, generate_quad};
use super::scalar_field::ScalarField;

// ============================================================================
// Kdtree.h — struct Kdtree
// ============================================================================

// struct Kdtree {
//   typedef std::array<Kdtree *, 2> FaceKd;
//   typedef std::array<Kdtree *, 4> EdgeKd;
//   RectilinearGrid grid;
//   int planeDir;
//   int depth;
//   bool clusterable{true};
//   Kdtree *children[2]{nullptr, nullptr};
#[derive(Debug, Clone)]
pub struct KdTreeNode {
    pub grid: RectilinearGrid,
    pub plane_dir: usize,
    pub depth: u32,
    pub clusterable: bool,
    pub children: [Option<Box<KdTreeNode>>; 2],
}

//   Kdtree(QefSolver sum,
//          const PositionCode &minCode,
//          const PositionCode &maxCode,
//          int dir,
//          int depth)
//     : grid(minCode, maxCode, sum),
//       planeDir(dir),
//       depth(depth) {}

impl HasGrid for KdTreeNode {
    fn grid(&self) -> &RectilinearGrid {
        &self.grid
    }
}

// AxisAlignedLine.h — struct AALine
// struct AALine {
//   PositionCode point;
//   int dir;
// };
struct AALine {
    point: PositionCode,
    dir: usize,
}

impl KdTreeNode {
    //   inline bool isLeaf() const {
    //     return !children[0] && !children[1];
    //   }
    pub fn is_leaf(&self) -> bool {
        self.children[0].is_none() && self.children[1].is_none()
    }

    //   inline bool isContouringLeaf(float threshold) const {
    //     if (!children[0] && !children[1]) {
    //       return true;
    //     }
    //     for (auto &v : grid.vertices) {
    //       if (v.error > threshold) {
    //         return false;
    //       }
    //     }
    //     return clusterable;
    //   }
    pub fn is_contouring_leaf(&self, threshold: f32) -> bool {
        if self.children[0].is_none() && self.children[1].is_none() {
            return true;
        }
        for v in &self.grid.vertices {
            if v.error > threshold {
                return false;
            }
        }
        self.clusterable
    }

    //   inline int axis() {
    //     assert(!isLeaf());
    //     if (children[0]) {
    //       return children[0]->grid.maxCode[planeDir];
    //     }
    //     return children[1]->grid.minCode[planeDir];
    //   }
    fn axis(&self) -> i32 {
        if let Some(c) = &self.children[0] {
            c.grid.max_code[self.plane_dir]
        } else if let Some(c) = &self.children[1] {
            c.grid.min_code[self.plane_dir]
        } else {
            0
        }
    }

    // ========================================================================
    // Kdtree.cpp — void Kdtree::combineQef()
    // ========================================================================

    // void Kdtree::combineQef() {
    //   if (!clusterable || isLeaf()) {
    //     return;
    //   }
    //   RectilinearGrid::combineAAGrid(children[0] ? &children[0]->grid : nullptr,
    //                                  children[1] ? &children[1]->grid : nullptr,
    //                                  planeDir,
    //                                  &grid);
    // }
    // void Kdtree::combineQef() {
    fn combine_qef(&mut self) {
        if !self.clusterable || self.is_leaf() {
            return;
        }
        // C++ passes &grid directly to combineAAGrid. We can't borrow
        // children and &mut self.grid simultaneously, so clone the child
        // grids, then operate on self.grid in place — matching C++ exactly.
        let left = self.children[0].as_ref().map(|c| c.grid.clone());
        let right = self.children[1].as_ref().map(|c| c.grid.clone());
        let dir = self.plane_dir;
        // C++: combineAAGrid does NOT call assignSign — grid already has
        // corner_signs set from the earlier assignSign call in buildFromOctree.
        RectilinearGrid::combine_aa_grid(
            left.as_ref(),
            right.as_ref(),
            dir,
            &mut self.grid,
        );
    }

    // ========================================================================
    // Kdtree.cpp — void Kdtree::calClusterability(ScalarField *t)
    // ========================================================================

    // void Kdtree::calClusterability(ScalarField *t) {
    //   bool selfClusterable = RectilinearGrid::calClusterability(
    //     children[0] ? &children[0]->grid : nullptr,
    //     children[1] ? &children[1]->grid : nullptr,
    //     planeDir, grid.minCode, grid.maxCode, t);
    //   if (!selfClusterable) {
    //     clusterable = false;
    //     return;
    //   }
    //   for (auto child : children) {
    //     if (child && !child->clusterable) {
    //       clusterable = false;
    //       return;
    //     }
    //   }
    //   clusterable = true;
    //   return;
    // }
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

    // ========================================================================
    // Kdtree.cpp — int Kdtree::chooseAxisDir(...)
    // ========================================================================

    // int Kdtree::chooseAxisDir(QefSolver &qef, const PositionCode &minCode, const PositionCode &maxCode) {
    //   int dir = 0;
    //   int strategy = 1;
    //   auto size = maxCode - minCode;
    //   // strategy == 1: variance approach
    //   glm::fvec3 approximate;
    //   float error;
    //   qef.solve(approximate, error);
    //   auto variance = qef.getVariance(approximate);
    //   variance[0] *= size[0];
    //   variance[1] *= size[1];
    //   variance[2] *= size[2];
    //   int maxVarDir = 0, minVarDir = 1;
    //   if (variance[1] > variance[0]) {
    //     maxVarDir = 1;
    //     minVarDir = 0;
    //   }
    //   if (variance[2] > variance[maxVarDir]) {
    //     maxVarDir = 2;
    //   }
    //   if (variance[minVarDir] > variance[2]) {
    //     minVarDir = 2;
    //   }
    //   dir = maxVarDir;
    //   if (size[maxVarDir] < 2) {
    //     dir = 3 - maxVarDir - minVarDir;
    //     if (size[3 - maxVarDir - minVarDir] < 2) {
    //       dir = minVarDir;
    //     }
    //   }
    //   return dir;
    // }
    fn choose_axis_dir(
        qef: &mut QefSolver,
        min_code: PositionCode,
        max_code: PositionCode,
    ) -> usize {
        let size = max_code - min_code;

        // C++: qef.solve(approximate, error);
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

    // ========================================================================
    // Kdtree.cpp — Kdtree *Kdtree::buildFromOctree(...)
    // ========================================================================

    // Kdtree *Kdtree::buildFromOctree(Octree *octree, const PositionCode &minCode,
    //   const PositionCode &maxCode, ScalarField *t, int depth) {
    //   if (glm::any(glm::greaterThanEqual(minCode, maxCode))) {
    //     return nullptr;
    //   }
    //   QefSolver sum;
    //   Octree::getSum(octree, minCode, maxCode, sum);
    //   if (sum.pointCount == 0) {
    //     return nullptr;
    //   }
    //   int strategy = 1;
    //   PositionCode bestRightMinCode = maxCode, bestLeftMaxCode = minCode;
    //   int dir = chooseAxisDir(sum, minCode, maxCode);
    //   int minAxis = minCode[dir];
    //   int maxAxis = maxCode[dir];
    //   // strategy == 1: binary search split
    //   QefSolver leftSum, rightSum;
    //   float minError = std::numeric_limits<float>::max();
    //   while (maxAxis - minAxis > 1) {
    //     int mid = (maxAxis + minAxis) / 2;
    //     PositionCode rightMinCode = minCode;
    //     rightMinCode[dir] = mid;
    //     PositionCode leftMaxCode = maxCode;
    //     leftMaxCode[dir] = mid;
    //     glm::fvec3 leftApproximate, rightApproximate;
    //     leftSum.reset();
    //     rightSum.reset();
    //     Octree::getSum(octree, minCode, leftMaxCode, leftSum);
    //     Octree::getSum(octree, rightMinCode, maxCode, rightSum);
    //     float leftError = 0.f;
    //     float rightError = 0.f;
    //     leftSum.solve(leftApproximate, leftError);
    //     rightSum.solve(rightApproximate, rightError);
    //     if (abs(leftError - rightError) < minError) {
    //       minError = abs(leftError - rightError);
    //       bestLeftMaxCode = leftMaxCode;
    //       bestRightMinCode = rightMinCode;
    //     }
    //     if (leftError > rightError) {
    //       maxAxis = mid;
    //     }
    //     else if (leftError < rightError) {
    //       minAxis = mid;
    //     }
    //     else {
    //       break;
    //     }
    //   }
    //   auto kd = new Kdtree(sum, minCode, maxCode, dir, depth);
    //   kd->children[0] = buildFromOctree(octree, minCode, bestLeftMaxCode, t, depth + 1);
    //   kd->children[1] = buildFromOctree(octree, bestRightMinCode, maxCode, t, depth + 1);
    //   if (kd->isLeaf()) {
    //     kd->grid.assignSign(t);
    //     kd->grid.sampleQef(t, false);
    //   }
    //   else {
    //     kd->grid.assignSign(t);
    //     kd->calClusterability(t);
    //     kd->combineQef();
    //   }
    //   if (kd->clusterable) {
    //     for (int i = 0; i < kd->grid.components.size(); ++i) {
    //       kd->grid.solveComponent(i);
    //     }
    //   }
    //   return kd;
    // }
    pub fn build_from_octree(
        octree: &OctreeNode,
        min_code: PositionCode,
        max_code: PositionCode,
        field: &dyn ScalarField,
        depth: u32,
        unit_size: f32,
    ) -> Option<Box<KdTreeNode>> {
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

        // Binary search for split plane
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
        let mut node = Box::new(KdTreeNode {
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

    // ========================================================================
    // Kdtree.cpp — Mesh *Kdtree::extractMesh(...)
    // ========================================================================

    // Mesh *Kdtree::extractMesh(Kdtree *root, ScalarField *t, float threshold) {
    //   Mesh *mesh = new Mesh;
    //   generateVertexIndices(root, mesh, t, threshold);
    //   contourCell(root, mesh, t, threshold);
    //   return mesh;
    // }
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

// ============================================================================
// Kdtree.cpp — free functions
// ============================================================================

// void Kdtree::generateVertexIndices(Kdtree *root, Mesh *mesh, ScalarField *t, float threshold) {
//   if (!root) {
//     return;
//   }
//   for (int i = 0; i < root->grid.vertices.size(); ++i) {
//     auto &v = root->grid.vertices[i];
//     mesh->addVertex(&v, t);
//   }
//   generateVertexIndices(root->children[0], mesh, t, threshold);
//   generateVertexIndices(root->children[1], mesh, t, threshold);
// }
#[allow(clippy::only_used_in_recursion)]
fn generate_vertex_indices(
    node: &mut KdTreeNode,
    threshold: f32,
    field: &dyn ScalarField,
    mesh: &mut IsoMesh,
) {
    for v in &mut node.grid.vertices {
        mesh.add_vertex(v, |p| field.normal(p));
    }

    // C++: always recurse into children (no contouring leaf check)
    for child in node.children.iter_mut().flatten() {
        generate_vertex_indices(child, threshold, field, mesh);
    }
}

// void Kdtree::contourCell(Kdtree *node, Mesh *mesh, ScalarField *t, float threshold) {
//   if (!node || node->isContouringLeaf(threshold)) {
//     return;
//   }
//   FaceKd faceNodes = {node->children[0], node->children[1]};
//   contourFace(faceNodes, node->planeDir, node->axis(), mesh, t, threshold);
//   contourCell(node->children[0], mesh, t, threshold);
//   contourCell(node->children[1], mesh, t, threshold);
// }
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

// bool checkMinialFace(const Kdtree::FaceKd &nodes, int dir,
//                      PositionCode &faceMin, PositionCode &faceMax) {
//   faceMax = min(nodes[0]->grid.maxCode, nodes[1]->grid.maxCode);
//   faceMin = max(nodes[0]->grid.minCode, nodes[1]->grid.minCode);
//   auto offset = faceMax - faceMin;
//   return offset[(dir + 1) % 3] > 0 && offset[(dir + 2) % 3] > 0;
// }
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

// AALine constructLine(const Kdtree::FaceKd &faceNodes, int side,
//                      int originFaceDir, int axis, float threshold) {
//   AALine line;
//   line.point[originFaceDir] = axis;
//   assert(!faceNodes[side]->isContouringLeaf(threshold));
//   line.dir = 3 - originFaceDir - faceNodes[side]->planeDir;
//   line.point[faceNodes[side]->planeDir] = faceNodes[side]->axis();
//   return line;
// }
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

// ============================================================================
// Kdtree.cpp — void Kdtree::contourFace(...)
// ============================================================================

// void Kdtree::contourFace(FaceKd &nodes,
//                          const int dir,
//                          const int axis,
//                          Mesh *mesh,
//                          ScalarField *t,
//                          float threshold) {
//   if (!nodes[0] || !nodes[1]) {
//     return;
//   }
//   if (nodes[0]->isContouringLeaf(threshold) && nodes[1]->isContouringLeaf(threshold)) {
//     return;
//   }
//   PositionCode faceMin, faceMax;
//   if (!checkMinialFace(nodes, dir, faceMin, faceMax)) {
//     return;
//   }
//   for (int i = 0; i < 2; ++i) {
//     while (!nodes[i]->isContouringLeaf(threshold) && nodes[i]->planeDir == dir) {
//       nodes[i] = nodes[i]->children[1 - i];
//       if (!nodes[i]) {
//         return;
//       }
//     }
//   }
//   for (int i = 0; i < 2; ++i) {
//     if (!nodes[i]->isContouringLeaf(threshold)) {
//       for (int j = 0; j < 2; ++j) {
//         FaceKd nextFace = nodes;
//         nextFace[i] = nodes[i]->children[j];
//         contourFace(nextFace, dir, axis, mesh, t, threshold);
//       }
//       if (nodes[i]->axis() > faceMin[nodes[i]->planeDir] &&
//           nodes[i]->axis() < faceMax[nodes[i]->planeDir]) {
//         EdgeKd edgeNodes = {nodes[0], nodes[0], nodes[1], nodes[1]};
//         edgeNodes[i * 2] = nodes[i]->children[0];
//         edgeNodes[i * 2 + 1] = nodes[i]->children[1];
//         AALine line = constructLine(nodes, i, dir, axis, threshold);
//         contourEdge(edgeNodes, line, nodes[i]->planeDir, t, threshold, mesh);
//       }
//       return;
//     }
//   }
// }
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
    // if (nodes[0]->isContouringLeaf(threshold) && nodes[1]->isContouringLeaf(threshold)) return;
    if nodes[0].is_contouring_leaf(threshold) && nodes[1].is_contouring_leaf(threshold) {
        return;
    }

    // if (!checkMinialFace(nodes, dir, faceMin, faceMax)) return;
    let (face_min, face_max) = match check_minimal_face(nodes, dir) {
        Some(v) => v,
        None => return,
    };

    // for (int i = 0; i < 2; ++i) {
    //   while (!nodes[i]->isContouringLeaf(threshold) && nodes[i]->planeDir == dir) {
    //     nodes[i] = nodes[i]->children[1 - i];
    //     if (!nodes[i]) return;
    //   }
    // }
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

    // for (int i = 0; i < 2; ++i) {
    //   if (!nodes[i]->isContouringLeaf(threshold)) {
    //     for (int j = 0; j < 2; ++j) {
    //       FaceKd nextFace = nodes;
    //       nextFace[i] = nodes[i]->children[j];
    //       contourFace(nextFace, dir, axis, mesh, t, threshold);
    //     }
    //     if (nodes[i]->axis() > faceMin[nodes[i]->planeDir] &&
    //         nodes[i]->axis() < faceMax[nodes[i]->planeDir]) {
    //       EdgeKd edgeNodes = {nodes[0], nodes[0], nodes[1], nodes[1]};
    //       edgeNodes[i * 2] = nodes[i]->children[0];
    //       edgeNodes[i * 2 + 1] = nodes[i]->children[1];
    //       AALine line = constructLine(nodes, i, dir, axis, threshold);
    //       contourEdge(edgeNodes, line, nodes[i]->planeDir, t, threshold, mesh);
    //     }
    //     return;
    //   }
    // }
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
                let mut edge_nodes: [Option<&KdTreeNode>; 4] = [
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

// ============================================================================
// Kdtree.cpp — bool checkMinialEdge(...)
// ============================================================================

// bool checkMinialEdge(const Kdtree::EdgeKd &nodes, const AALine &line,
//                      PositionCode &minEnd, PositionCode &maxEnd) {
//   minEnd = maxEnd = line.point;
//   int dir = line.dir;
//   minEnd[dir] = max(max(nodes[0]->grid.minCode, nodes[1]->grid.minCode),
//                     max(nodes[2]->grid.minCode, nodes[3]->grid.minCode))[dir];
//   maxEnd[dir] = min(min(nodes[0]->grid.maxCode, nodes[1]->grid.maxCode),
//                     min(nodes[2]->grid.maxCode, nodes[3]->grid.maxCode))[dir];
//   return minEnd[dir] < maxEnd[dir];
// }
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

// int nextQuadIndex(int dir1, int dir2, int planeDir, int i) {
//   PositionCode pos;
//   pos[dir1] = 1 - i % 2;
//   pos[dir2] = 1 - i / 2;
//   return pos[planeDir];
// }
fn next_quad_index(dir1: usize, dir2: usize, plane_dir: usize, i: usize) -> usize {
    let mut pos = PositionCode::ZERO;
    pos[dir1] = 1 - (i % 2) as i32;
    pos[dir2] = 1 - (i / 2) as i32;
    pos[plane_dir] as usize
}

// ============================================================================
// Kdtree.cpp — void Kdtree::detectQuad(...)
// ============================================================================

// void Kdtree::detectQuad(EdgeKd &nodes, AALine line, float threshold) {
//   for (int i = 0; i < 2; ++i) {
//     while (
//       nodes[i * 2] && nodes[i * 2 + 1] &&
//       !nodes[i * 2]->isContouringLeaf(threshold) &&
//       nodes[2 * i] == nodes[2 * i + 1] &&
//       nodes[i * 2]->planeDir != line.dir) {
//       auto commonNode = nodes[i * 2];
//       if (nodes[i * 2]->axis() == line.point[nodes[i * 2]->planeDir]) {
//         nodes[i * 2] = commonNode->children[0];
//         nodes[i * 2 + 1] = commonNode->children[1];
//       }
//       else if (nodes[i * 2]->axis() > line.point[nodes[i * 2]->planeDir]) {
//         nodes[i * 2] = commonNode->children[0];
//         nodes[i * 2 + 1] = commonNode->children[0];
//       }
//       else {
//         nodes[i * 2] = commonNode->children[1];
//         nodes[i * 2 + 1] = commonNode->children[1];
//       }
//     }
//   }
// }
#[allow(clippy::while_let_loop)]
fn detect_quad(nodes: &mut [Option<&KdTreeNode>; 4], line: &AALine, threshold: f32) {
    for i in 0..2 {
        loop {
            let a = match nodes[i * 2] {
                Some(n) => n,
                None => break,
            };
            let b = match nodes[i * 2 + 1] {
                Some(n) => n,
                None => break,
            };
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

// void setQuadNode(Kdtree::EdgeKd &nodes, int i, Kdtree *p) {
//   if (nodes[oppositeQuadIndex(i)] == nodes[i]) {
//     nodes[oppositeQuadIndex(i)] = p;
//   }
//   nodes[i] = p;
// }
#[allow(clippy::needless_lifetimes)]
fn set_quad_node<'a>(
    nodes: &mut [Option<&'a KdTreeNode>; 4],
    i: usize,
    new_node: Option<&'a KdTreeNode>,
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

// ============================================================================
// Kdtree.cpp — void Kdtree::contourEdge(...)
// ============================================================================

// void Kdtree::contourEdge(EdgeKd &nodes,
//                          const AALine &line,
//                          const int quadDir1,
//                          ScalarField *t,
//                          float threshold,
//                          Mesh *mesh) {
//   detectQuad(nodes, line, threshold);
//   for (auto n : nodes) {
//     if (!n) {
//       return;
//     }
//   }
//   assert(quadDir1 >= 0 && quadDir1 < 3);
//   const int quadDir2 = 3 - quadDir1 - line.dir;
//   PositionCode minEndCode, maxEndCode;
//   if (!checkMinialEdge(nodes, line, minEndCode, maxEndCode)) {
//     return;
//   }
//   for (int i = 0; i < 4; ++i) {
//     if (nodes[i] != nodes[oppositeQuadIndex(i)]) {
//       while (!nodes[i]->isContouringLeaf(threshold) && nodes[i]->planeDir != line.dir) {
//         nodes[i] = nodes[i]->children[nextQuadIndex(quadDir1, quadDir2, nodes[i]->planeDir, i)];
//         if (!nodes[i]) {
//           return;
//         }
//       }
//     }
//   }
//   if (nodes[0]->isContouringLeaf(threshold) && nodes[1]->isContouringLeaf(threshold) &&
//       nodes[2]->isContouringLeaf(threshold) && nodes[3]->isContouringLeaf(threshold)) {
//     // debug check elided
//     generateQuad(nodes, quadDir1, quadDir2, mesh, t, threshold);
//     return;
//   }
//   for (int i = 0; i < 4; ++i) {
//     EdgeKd nextNodes = nodes;
//     if (!nodes[i]->isContouringLeaf(threshold) && nodes[i]->planeDir == line.dir) {
//       setQuadNode(nextNodes, i, nodes[i]->children[0]);
//       contourEdge(nextNodes, line, quadDir1, t, threshold, mesh);
//       nextNodes = nodes;
//       setQuadNode(nextNodes, i, nodes[i]->children[1]);
//       contourEdge(nextNodes, line, quadDir1, t, threshold, mesh);
//       return;
//     }
//   }
// }
#[allow(clippy::too_many_arguments)]
fn contour_edge(
    mut nodes: [Option<&KdTreeNode>; 4],
    line: &AALine,
    quad_dir1: usize,
    field: &dyn ScalarField,
    threshold: f32,
    mesh: &mut IsoMesh,
    unit_size: f32,
) {
    detect_quad(&mut nodes, line, threshold);

    // for (auto n : nodes) { if (!n) return; }
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

    // if (!checkMinialEdge(nodes, line, minEndCode, maxEndCode)) return;
    if check_minimal_edge([n0, n1, n2, n3], line).is_none() {
        return;
    }

    // for (int i = 0; i < 4; ++i) {
    //   if (nodes[i] != nodes[oppositeQuadIndex(i)]) {
    //     while (!nodes[i]->isContouringLeaf(threshold) && nodes[i]->planeDir != line.dir) {
    //       nodes[i] = nodes[i]->children[nextQuadIndex(...)];
    //       if (!nodes[i]) return;
    //     }
    //   }
    // }
    let mut unwrapped: [&KdTreeNode; 4] = [n0, n1, n2, n3];
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

    // if all 4 are contouring leaves: generateQuad
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

    // for (int i = 0; i < 4; ++i) {
    //   EdgeKd nextNodes = nodes;
    //   if (!nodes[i]->isContouringLeaf(threshold) && nodes[i]->planeDir == line.dir) {
    //     setQuadNode(nextNodes, i, nodes[i]->children[0]);
    //     contourEdge(nextNodes, line, quadDir1, t, threshold, mesh);
    //     nextNodes = nodes;
    //     setQuadNode(nextNodes, i, nodes[i]->children[1]);
    //     contourEdge(nextNodes, line, quadDir1, t, threshold, mesh);
    //     return;
    //   }
    // }
    let opt_nodes: [Option<&KdTreeNode>; 4] = [
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

// ============================================================================
// Kdtree.cpp — void Kdtree::generateQuad(...)
// ============================================================================

// void Kdtree::generateQuad(EdgeKd &nodes,
//                           int quadDir1,
//                           int quadDir2,
//                           Mesh *mesh,
//                           ScalarField *t,
//                           float threshold) {
//   RectilinearGrid::generateQuad(nodes, quadDir1, quadDir2, mesh, t, threshold);
// }
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
        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let sphere = Sphere::with_center(3.0, glam::Vec3::new(4.0, 4.0, 4.0));
                let unit_size = 1.0;
                let depth = 4;
                let size_code = PositionCode::splat(1 << (depth - 1));
                let min_code = PositionCode::splat(0);
                let max_code = size_code;

                let octree = OctreeNode::build_with_scalar_field(
                    min_code, depth, &sphere, true, unit_size,
                );
                assert!(octree.is_some(), "Octree should not be empty for a sphere");
                let octree = match octree {
                    Some(o) => o,
                    None => return,
                };

                let kdtree = KdTreeNode::build_from_octree(
                    &octree, min_code, max_code, &sphere, 0, unit_size,
                );
                assert!(kdtree.is_some(), "KdTree should not be empty for a sphere");
                let mut kdtree = match kdtree {
                    Some(k) => k,
                    None => return,
                };

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

                let mut octree_for_mesh = OctreeNode::build_with_scalar_field(
                    min_code, depth, &sphere, false, unit_size,
                );
                if let Some(ref mut oct_root) = octree_for_mesh {
                    let oct_mesh = OctreeNode::extract_mesh(oct_root, &sphere, unit_size);
                    let oct_triangles = oct_mesh.triangle_count();

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

    #[test]
    fn kdtree_box_with_hole_mesh() {
        use crate::isosurface::scalar_field::{Aabb, Cylinder, Difference};

        let result = std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let field: Box<dyn crate::isosurface::scalar_field::ScalarField> =
                    Box::new(Difference::new(
                        Aabb::new(glam::Vec3::splat(-4.0), glam::Vec3::splat(4.0)),
                        Cylinder::new(glam::Vec3::new(0.0, 0.0, 3.0)),
                    ));

                let depth = 8u32;
                let size_code = PositionCode::splat(1 << (depth - 1));
                let unit_size = 32.0 / size_code.x as f32;
                let min_code = -size_code / 2;

                // Build octree mesh (reference) at threshold=0
                let mut oct_normal = OctreeNode::build_with_scalar_field(
                    min_code, depth, field.as_ref(), false, unit_size,
                ).expect("octree should exist");
                let oct_mesh = OctreeNode::extract_mesh(&mut oct_normal, field.as_ref(), unit_size);

                // Build kd-tree mesh at threshold=0
                let oct_for_kd = OctreeNode::build_with_scalar_field(
                    min_code, depth, field.as_ref(), true, unit_size,
                ).expect("octree for kd should exist");
                let mut kdtree = KdTreeNode::build_from_octree(
                    &oct_for_kd, min_code, size_code / 2, field.as_ref(), 0, unit_size,
                ).expect("kdtree should exist");
                let kd_mesh = KdTreeNode::extract_mesh(
                    &mut kdtree, field.as_ref(), 0.0, unit_size,
                );

                let oct_tris = oct_mesh.triangle_count();
                let kd_tris = kd_mesh.triangle_count();
                eprintln!("Octree: {oct_tris} triangles, {} vertices", oct_mesh.positions.len());
                eprintln!("KdTree: {kd_tris} triangles, {} vertices", kd_mesh.positions.len());

                // Count triangles with max edge > 2.0 (long-range connections)
                let count_long = |mesh: &IsoMesh| -> usize {
                    let mut n = 0;
                    for t in 0..mesh.triangle_count() {
                        let i0 = mesh.indices[t * 3] as usize;
                        let i1 = mesh.indices[t * 3 + 1] as usize;
                        let i2 = mesh.indices[t * 3 + 2] as usize;
                        let p0 = mesh.positions[i0];
                        let p1 = mesh.positions[i1];
                        let p2 = mesh.positions[i2];
                        let max_edge = (p1-p0).length().max((p2-p1).length()).max((p0-p2).length());
                        if max_edge > 2.0 { n += 1; }
                    }
                    n
                };

                let oct_long = count_long(&oct_mesh);
                let kd_long = count_long(&kd_mesh);
                eprintln!("Octree long-edge tris: {oct_long}");
                eprintln!("KdTree long-edge tris: {kd_long}");

                assert!(kd_tris > 0, "KdTree should produce triangles");
                // KdTree should not have massively more triangles than octree
                assert!(
                    kd_tris < oct_tris * 2,
                    "KdTree has {kd_tris} tris vs octree's {oct_tris} — too many extra"
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
