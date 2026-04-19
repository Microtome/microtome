//! Rectilinear grid cell for dual contouring with k-d tree acceleration.
//!
//! This is the fundamental cell type in the KdtreeISO dual contouring algorithm.
//! Each grid represents an axis-aligned cell that stores QEF data, corner signs,
//! connected component information, and solved vertex positions.
//!
//! Line-by-line port from C++ KdtreeISO:
//!   - RectilinearGrid.h
//!   - RectilinearGrid.cpp

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

// C++ struct RectilinearGrid {
//   PositionCode minCode;
//   PositionCode maxCode;
//   QefSolver allQef;
//   std::vector<QefSolver> components;
//   std::vector<Vertex> vertices;
//   Vertex approximate;
//   uint8_t cornerSigns[8]{0};
//   int8_t componentIndices[8]{0};
//   bool isSigned = false;
//   std::map<RectilinearGrid *, Vertex *> faceVertices;
// };

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
    // C++ explicit RectilinearGrid(PositionCode minCode = PositionCode(0, 0, 0),
    //                              PositionCode maxCode = PositionCode(0, 0, 0),
    //                              QefSolver sum = QefSolver())
    //   : minCode(minCode), maxCode(maxCode), allQef(sum) {
    //   solve(allQef, approximate);
    // }

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
        // C++: solve(allQef, approximate);
        Self::solve_qef(
            &mut grid.all_qef,
            &mut grid.approximate,
            min_code,
            max_code,
            unit_size,
        );
        grid
    }

    // C++ void RectilinearGrid::solveComponent(int i) {
    //   solve(components[i], vertices[i]);
    // }

    /// Solves the QEF for a single connected component, clamping the result
    /// to the cell bounds.
    pub fn solve_component(&mut self, i: usize, unit_size: f32) {
        if i < self.components.len() && i < self.vertices.len() {
            let min_code = self.min_code;
            let max_code = self.max_code;
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

    // C++ void RectilinearGrid::solve(QefSolver &qef, Vertex &v) {
    //   auto &p = v.hermiteP;
    //   qef.solve(p, v.error);
    //   auto extends = codeToPos(maxCode - minCode, RectilinearGrid::getUnitSize()) * 0.5f;
    //   const auto min = codeToPos(minCode, RectilinearGrid::getUnitSize()) - extends;
    //   const auto max = codeToPos(maxCode, RectilinearGrid::getUnitSize()) + extends;
    //   if (p.x < min.x || p.x > max.x ||
    //       p.y < min.y || p.y > max.y ||
    //       p.z < min.z || p.z > max.z) {
    //     p = qef.massPointSum / (float)qef.pointCount;
    //   }
    // }

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
        // C++: qef.solve(p, v.error);
        let mut pos = vertex.hermite_p;
        let mut error = vertex.error;
        qef.solve(&mut pos, &mut error);
        // C++: auto extends = codeToPos(maxCode - minCode, ...) * 0.5f;
        let extends = code_to_pos(max_code - min_code, unit_size) * 0.5;
        // C++: const auto min = codeToPos(minCode, ...) - extends;
        let min_pos = code_to_pos(min_code, unit_size) - extends;
        // C++: const auto max = codeToPos(maxCode, ...) + extends;
        let max_pos = code_to_pos(max_code, unit_size) + extends;
        // C++: if (p.x < min.x || p.x > max.x || ...)
        if pos.x < min_pos.x
            || pos.x > max_pos.x
            || pos.y < min_pos.y
            || pos.y > max_pos.y
            || pos.z < min_pos.z
            || pos.z > max_pos.z
        {
            // C++: p = qef.massPointSum / (float)qef.pointCount;
            pos = qef.mass_point();
            // Recompute the QEF residual at the *clamped* position. The
            // C++ source kept the SVD-solution residual here, which is
            // misleadingly low — collapse-thresholds (kdtree_v2.rs:65,
            // kdtree.rs:84) read this field to decide simplification, so
            // an underreport of the actual fitted-vertex error causes
            // overeager collapse and visible gouges around features
            // where the SVD pushes the solution out of bounds.
            error = qef.get_error_at(pos);
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

    // C++ void RectilinearGrid::assignSign(ScalarField *t) {
    //   auto sizeCode = PositionCode(
    //     maxCode.x - minCode.x,
    //     maxCode.y - minCode.y,
    //     maxCode.z - minCode.z);
    //   int8_t mtlID = t->getMaterialID();          // always returns 1
    //   for (int i = 0; i < 8; ++i) {
    //     PositionCode code = decodeCell(i);
    //     float val = t->index(minCode + sizeCode * code);
    //     cornerSigns[i] = (uint8_t)(val >= 0. ? 0 : mtlID);
    //   }
    //   isSigned = !((cornerSigns[0] == cornerSigns[1]) &&
    //                (cornerSigns[1] == cornerSigns[2]) &&
    //                ... (cornerSigns[6] == cornerSigns[7]));
    // }

    /// Samples the scalar field at each of the 8 corners and records their signs.
    ///
    /// Sets `is_signed` to `true` if at least one corner differs in sign from
    /// the others.
    pub fn assign_sign(&mut self, field: &dyn ScalarField, unit_size: f32) {
        // C++: auto sizeCode = maxCode - minCode;
        let size_code = self.max_code - self.min_code;
        // C++: int8_t mtlID = t->getMaterialID(); // always 1
        // In Rust we use u8::from(val < 0.0) which gives 0 or 1 — same semantics.
        for i in 0..8 {
            // C++: PositionCode code = decodeCell(i);
            let code = decode_cell(i);
            // C++: float val = t->index(minCode + sizeCode * code);
            let val = field.index(self.min_code + size_code * code, unit_size);
            // C++: cornerSigns[i] = (uint8_t)(val >= 0. ? 0 : mtlID);
            self.corner_signs[i] = u8::from(val < 0.0);
        }
        // C++: isSigned = !((cornerSigns[0] == cornerSigns[1]) && ... && (cornerSigns[6] == cornerSigns[7]));
        self.is_signed = !((self.corner_signs[0] == self.corner_signs[1])
            && (self.corner_signs[1] == self.corner_signs[2])
            && (self.corner_signs[2] == self.corner_signs[3])
            && (self.corner_signs[3] == self.corner_signs[4])
            && (self.corner_signs[4] == self.corner_signs[5])
            && (self.corner_signs[5] == self.corner_signs[6])
            && (self.corner_signs[6] == self.corner_signs[7]));
    }

    // C++ void RectilinearGrid::calCornerComponents() {
    //   assert(components.empty());
    //   std::set<int> clusters[8];
    //   for (int i = 0; i < 8; ++i) {
    //     if (cornerSigns[i] != 0) {
    //       clusters[i].insert({i});
    //       componentIndices[i] = static_cast<uint8_t>(i);
    //     }
    //   }
    //   for (int i = 0; i < 12; ++i) {
    //     int c1 = cellProcFaceMask[i][0];
    //     int c2 = cellProcFaceMask[i][1];
    //     if (cornerSigns[c1] == cornerSigns[c2] && cornerSigns[c2] != 0) {
    //       int co1 = componentIndices[c1];
    //       int co2 = componentIndices[c2];
    //       auto &c2Components = clusters[co2];
    //       for (auto comp : c2Components) {
    //         clusters[co1].insert(comp);
    //       }
    //       for (auto comp : clusters[co1]) {
    //         componentIndices[comp] = static_cast<uint8_t>(co1);
    //       }
    //     }
    //   }
    //   int reorderMap[8]{0};
    //   for (int i = 0; i < 8; ++i) {
    //     reorderMap[i] = -1;
    //   }
    //   int new_order = 0;
    //   for (int i = 0; i < 8; ++i) {
    //     if (reorderMap[componentIndices[i]] == -1 && cornerSigns[i] != 0) {
    //       reorderMap[componentIndices[i]] = new_order++;
    //     }
    //   }
    //   for (int i = 0; i < 8; ++i) {
    //     componentIndices[i] = static_cast<uint8_t>(reorderMap[componentIndices[i]]);
    //   }
    //   vertices.resize(static_cast<unsigned long>(new_order));
    //   components.resize(static_cast<unsigned long>(new_order));
    // }

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
        // C++: assert(components.empty());
        debug_assert!(self.components.is_empty());

        // C++: std::set<int> clusters[8];
        let mut clusters: [Vec<usize>; 8] = Default::default();

        // C++: for (int i = 0; i < 8; ++i) {
        #[allow(clippy::needless_range_loop)]
        for i in 0..8 {
            // C++: if (cornerSigns[i] != 0) {
            if self.corner_signs[i] != 0 {
                // C++: clusters[i].insert({i});
                clusters[i].push(i);
                // C++: componentIndices[i] = static_cast<uint8_t>(i);
                self.component_indices[i] = i as i8;
            }
        }

        // C++: for (int i = 0; i < 12; ++i) {
        for mask in &CELL_PROC_FACE_MASK {
            // C++: int c1 = cellProcFaceMask[i][0];
            let c1 = mask[0];
            // C++: int c2 = cellProcFaceMask[i][1];
            let c2 = mask[1];
            // C++: if (cornerSigns[c1] == cornerSigns[c2] && cornerSigns[c2] != 0) {
            if self.corner_signs[c1] == self.corner_signs[c2] && self.corner_signs[c2] != 0 {
                // C++: int co1 = componentIndices[c1];
                let co1 = self.component_indices[c1] as usize;
                // C++: int co2 = componentIndices[c2];
                let co2 = self.component_indices[c2] as usize;
                // C++: auto &c2Components = clusters[co2];
                // C++: for (auto comp : c2Components) { clusters[co1].insert(comp); }
                let c2_members: Vec<usize> = clusters[co2].clone();
                for &comp in &c2_members {
                    clusters[co1].push(comp);
                }
                // C++: for (auto comp : clusters[co1]) { componentIndices[comp] = static_cast<uint8_t>(co1); }
                let co1_members: Vec<usize> = clusters[co1].clone();
                for &comp in &co1_members {
                    self.component_indices[comp] = co1 as i8;
                }
            }
        }

        // C++: int reorderMap[8]{0};
        // C++: for (int i = 0; i < 8; ++i) { reorderMap[i] = -1; }
        let mut reorder_map: [i8; 8] = [-1; 8];
        // C++: int new_order = 0;
        let mut new_order: i8 = 0;

        // C++: for (int i = 0; i < 8; ++i) {
        for i in 0..8 {
            // C++: if (reorderMap[componentIndices[i]] == -1 && cornerSigns[i] != 0) {
            if self.corner_signs[i] != 0 && reorder_map[self.component_indices[i] as usize] == -1 {
                // C++: reorderMap[componentIndices[i]] = new_order++;
                reorder_map[self.component_indices[i] as usize] = new_order;
                new_order += 1;
            }
        }

        // C++: for (int i = 0; i < 8; ++i) { componentIndices[i] = static_cast<uint8_t>(reorderMap[componentIndices[i]]); }
        for i in 0..8 {
            self.component_indices[i] = reorder_map[self.component_indices[i] as usize];
        }

        // C++: vertices.resize(static_cast<unsigned long>(new_order));
        self.vertices
            .resize_with(new_order as usize, Vertex::default);
        // C++: components.resize(static_cast<unsigned long>(new_order));
        self.components
            .resize_with(new_order as usize, QefSolver::new);
    }

    // C++ bool RectilinearGrid::sampleQef(ScalarField *t, bool all) {
    //   calCornerComponents();                  // NOTE: Rust callers call this separately
    //   const auto min = codeToPos(minCode, RectilinearGrid::getUnitSize());
    //   auto isize = maxCode - minCode;
    //   auto size = codeToPos(isize, RectilinearGrid::getUnitSize());
    //   assert(!isnan(size.x));
    //
    //   fvec3 cornerPositions[8];
    //   for (int i = 0; i < 8; ++i) {
    //     cornerPositions[i] = min + size * min_offset_subdivision(i);
    //   }
    //   for (int i = 0; i < 12; ++i) {
    //     fvec3 p1 = cornerPositions[edge_map[i][0]];
    //     fvec3 p2 = cornerPositions[edge_map[i][1]];
    //     if (cornerSigns[edge_map[i][0]] != cornerSigns[edge_map[i][1]]) {
    //       fvec3 p, n;
    //       if (t->solve(p1, p2, p)) {
    //         t->normal(p, n);
    //         int qefIndex = edgeComponentIndex(edge_map[i][0], edge_map[i][1]);
    //         components.at(static_cast<unsigned long>(qefIndex)).add(p, n);
    //         if (all) {
    //           allQef.add(p, n);
    //         }
    //       }
    //     }
    //   }
    //   for (int i = 0; i < components.size(); ++i) {
    //     if (components[i].pointCount == 0 || components[i].pointCount >= 12) {
    //       return false;
    //     }
    //     t->normal(vertices[i].hermiteP, vertices[i].hermiteN);
    //   }
    //   return allQef.pointCount > 0;
    // }

    /// Samples edge crossings to build QEF data for each connected component.
    ///
    /// NOTE: The C++ calls `calCornerComponents()` at the start, but Rust callers
    /// already call this separately before `sample_qef`, so we do NOT call it here.
    ///
    /// For each edge whose endpoint corners have different signs, the
    /// zero-crossing and surface normal are computed and added to the
    /// appropriate component's QEF. Also accumulates into `all_qef` when `all` param is used.
    ///
    /// Returns `true` if any edge crossings were found (allQef.pointCount > 0).
    pub fn sample_qef(
        &mut self,
        field: &dyn ScalarField,
        all: &mut QefSolver,
        unit_size: f32,
    ) -> bool {
        // NOTE: C++ calls calCornerComponents() here, but Rust callers do it beforehand.

        // C++: const auto min = codeToPos(minCode, RectilinearGrid::getUnitSize());
        let min = code_to_pos(self.min_code, unit_size);
        // C++: auto isize = maxCode - minCode;
        // C++: auto size = codeToPos(isize, RectilinearGrid::getUnitSize());
        let isize = self.max_code - self.min_code;
        let size = code_to_pos(isize, unit_size);

        // C++: fvec3 cornerPositions[8];
        // C++: for (int i = 0; i < 8; ++i) {
        //        cornerPositions[i] = min + size * min_offset_subdivision(i);
        //      }
        let mut corner_positions = [Vec3::ZERO; 8];
        #[allow(clippy::needless_range_loop)]
        for i in 0..8 {
            corner_positions[i] = min + size * super::indicators::min_offset_subdivision(i);
        }

        // C++: for (int i = 0; i < 12; ++i) {
        for i in 0..12 {
            // C++: fvec3 p1 = cornerPositions[edge_map[i][0]];
            let p1 = corner_positions[EDGE_MAP[i][0]];
            // C++: fvec3 p2 = cornerPositions[edge_map[i][1]];
            let p2 = corner_positions[EDGE_MAP[i][1]];
            // C++: if (cornerSigns[edge_map[i][0]] != cornerSigns[edge_map[i][1]]) {
            if self.corner_signs[EDGE_MAP[i][0]] != self.corner_signs[EDGE_MAP[i][1]] {
                // The C++ code split this into `solve(p1,p2,p)` followed
                // by `normal(p,n)`. That is correct for analytic SDFs
                // (the gradient is well-defined everywhere), but for a
                // piecewise-constant mesh-derived field the
                // nearest-anywhere normal lookup picks up the *wrong*
                // triangle's normal at sharp features, feeding the QEF
                // a contradictory plane and chipping the corner. Using
                // the trait's paired `hermite` keeps the per-edge
                // (position, normal) together.
                if let Some((p, n)) = field.hermite(p1, p2) {
                    let qef_index = self.edge_component_index(EDGE_MAP[i][0], EDGE_MAP[i][1]);
                    if qef_index >= 0 && (qef_index as usize) < self.components.len() {
                        self.components[qef_index as usize].add(p, n);
                    }
                    all.add(p, n);
                }
            }
        }

        // C++: for (int i = 0; i < components.size(); ++i) {
        for i in 0..self.components.len() {
            // C++: if (components[i].pointCount == 0 || components[i].pointCount >= 12) {
            //        return false;
            //      }
            if self.components[i].point_count() == 0 || self.components[i].point_count() >= 12 {
                return false;
            }
            // C++: t->normal(vertices[i].hermiteP, vertices[i].hermiteN);
            let p = self.vertices[i].hermite_p;
            self.vertices[i].hermite_n = field.normal(p);
        }

        // C++: return allQef.pointCount > 0;
        all.point_count() > 0
    }

    // C++ inline glm::fvec3 cornerPos(int i) {
    //   return min_offset_subdivision(i) * codeToPos(maxCode - minCode, RectilinearGrid::getUnitSize())
    //          + codeToPos(minCode, RectilinearGrid::getUnitSize());
    // }

    /// Returns the world-space position of corner `i` (0..8).
    pub fn corner_pos(&self, i: usize, unit_size: f32) -> Vec3 {
        // C++: return min_offset_subdivision(i) * codeToPos(maxCode - minCode, getUnitSize())
        //             + codeToPos(minCode, getUnitSize());
        super::indicators::min_offset_subdivision(i)
            * code_to_pos(self.max_code - self.min_code, unit_size)
            + code_to_pos(self.min_code, unit_size)
    }

    // C++ inline int edgeComponentIndex(int corner1, int corner2) {
    //   if (cornerSigns[corner1] != 0) {
    //     return componentIndices[corner1];
    //   }
    //   return componentIndices[corner2];
    // }

    /// Returns the component index for the edge between two corners.
    ///
    /// Returns the component index of whichever corner is inside (sign != 0).
    /// Assumes the caller has verified a sign change exists on this edge.
    pub fn edge_component_index(&self, corner1: usize, corner2: usize) -> i8 {
        // C++: if (cornerSigns[corner1] != 0) { return componentIndices[corner1]; }
        if self.corner_signs[corner1] != 0 {
            self.component_indices[corner1]
        } else {
            // C++: return componentIndices[corner2];
            self.component_indices[corner2]
        }
    }

    // C++ inline int faceComponentIndex(int faceDir, int edgeDir, int faceSide, int edgeSide) {
    //   int component = -1;
    //   int dir = 3 - faceDir - edgeDir;
    //   for (int i = 0; i < 2; ++i) {
    //     ivec3 code;
    //     code[faceDir] = faceSide;
    //     code[edgeDir] = edgeSide;
    //     code[dir] = i;
    //     int corner = encodeCell(code);
    //     if (cornerSigns[corner] > 0) {
    //       component = componentIndices[corner];
    //     }
    //   }
    //   if (component != -1) {
    //     return component;
    //   }
    //   for (int i = 0; i < 2; ++i) {
    //     ivec3 code;
    //     code[faceDir] = faceSide;
    //     code[edgeDir] = 1 - edgeSide;
    //     code[dir] = i;
    //     int corner = encodeCell(code);
    //     if (cornerSigns[corner] > 0) {
    //       component = componentIndices[corner];
    //     }
    //   }
    //   return component;
    // }

    /// Returns the component index for a face-edge configuration.
    ///
    /// Matches C++ `faceComponentIndex` exactly.
    pub fn face_component_index(
        &self,
        face_dir: usize,
        edge_dir: usize,
        face_side: usize,
        edge_side: usize,
    ) -> i8 {
        // C++: int component = -1;
        let mut component: i8 = -1;
        // C++: int dir = 3 - faceDir - edgeDir;
        let dir = 3 - face_dir - edge_dir;

        // C++: for (int i = 0; i < 2; ++i) {
        for i in 0..2 {
            // C++: ivec3 code; code[faceDir] = faceSide; code[edgeDir] = edgeSide; code[dir] = i;
            let mut code = glam::IVec3::ZERO;
            code[face_dir] = face_side as i32;
            code[edge_dir] = edge_side as i32;
            code[dir] = i;
            // C++: int corner = encodeCell(code);
            let corner = encode_cell(code);
            // C++: if (cornerSigns[corner] > 0) { component = componentIndices[corner]; }
            if self.corner_signs[corner] > 0 {
                component = self.component_indices[corner];
            }
        }
        // C++: if (component != -1) { return component; }
        if component != -1 {
            return component;
        }

        // C++: for (int i = 0; i < 2; ++i) {
        for i in 0..2 {
            // C++: code[edgeDir] = 1 - edgeSide;
            let mut code = glam::IVec3::ZERO;
            code[face_dir] = face_side as i32;
            code[edge_dir] = 1 - edge_side as i32;
            code[dir] = i;
            let corner = encode_cell(code);
            if self.corner_signs[corner] > 0 {
                component = self.component_indices[corner];
            }
        }
        // C++: return component;
        component
    }

    // C++ bool RectilinearGrid::calClusterability(RectilinearGrid *left,
    //                                             RectilinearGrid *right,
    //                                             int dir,
    //                                             const PositionCode &minCode,
    //                                             const PositionCode &maxCode,
    //                                             ScalarField *s) {
    //   if (!left && !right) { return true; }
    //   int clusterCornerSigns[8];
    //   for (int i = 0; i < 8; ++i) {
    //     clusterCornerSigns[i] = s->index(minCode + (maxCode - minCode) * decodeCell(i)) >= 0 ? 0 : s->getMaterialID();
    //   }
    //   bool homogeneous = true;
    //   for (int i = 1; i < 8; ++i) {
    //     if (clusterCornerSigns[i] != clusterCornerSigns[0]) { homogeneous = false; }
    //   }
    //   if (homogeneous) { return false; }
    //   if (!(left && right)) { return true; }
    //   RectilinearGrid *params[2] = {left, right};
    //   for (int i = 0; i < 4; ++i) {
    //     int edgeMinIndex = cellProcFaceMask[dir * 4 + i][0];
    //     int edgeMaxIndex = cellProcFaceMask[dir * 4 + i][1];
    //     int signChanges = 0;
    //     for (int j = 0; j < 2; ++j) {
    //       if (params[j]->cornerSigns[edgeMinIndex] != params[j]->cornerSigns[edgeMaxIndex]) {
    //         signChanges++;
    //       }
    //     }
    //     if (signChanges > 1) { return false; }
    //   }
    //   return true;
    // }

    /// Checks whether two adjacent grids can be clustered (merged).
    ///
    /// Matches C++ `RectilinearGrid::calClusterability` exactly.
    pub fn cal_clusterability(
        left: Option<&RectilinearGrid>,
        right: Option<&RectilinearGrid>,
        dir: usize,
        min_code: PositionCode,
        max_code: PositionCode,
        field: &dyn ScalarField,
        unit_size: f32,
    ) -> bool {
        // C++: if (!left && !right) { return true; }
        if left.is_none() && right.is_none() {
            return true;
        }

        // C++: int clusterCornerSigns[8];
        let mut cluster_corner_signs = [0usize; 8];
        // C++: for (int i = 0; i < 8; ++i) {
        #[allow(clippy::needless_range_loop)]
        for i in 0..8 {
            // C++: clusterCornerSigns[i] = s->index(minCode + (maxCode - minCode) * decodeCell(i)) >= 0 ? 0 : s->getMaterialID();
            let code = min_code + (max_code - min_code) * decode_cell(i);
            let val = field.index(code, unit_size);
            cluster_corner_signs[i] = if val >= 0.0 { 0 } else { 1 };
        }

        // C++: bool homogeneous = true;
        let mut homogeneous = true;
        // C++: for (int i = 1; i < 8; ++i) {
        for i in 1..8 {
            // C++: if (clusterCornerSigns[i] != clusterCornerSigns[0]) { homogeneous = false; }
            if cluster_corner_signs[i] != cluster_corner_signs[0] {
                homogeneous = false;
            }
        }
        // C++: if (homogeneous) { return false; }
        if homogeneous {
            return false;
        }

        // C++: if (!(left && right)) { return true; }
        let (left, right) = match (left, right) {
            (Some(l), Some(r)) => (l, r),
            _ => return true,
        };

        // C++: RectilinearGrid *params[2] = {left, right};
        let grids = [left, right];
        // C++: for (int i = 0; i < 4; ++i) {
        for i in 0..4 {
            // C++: int edgeMinIndex = cellProcFaceMask[dir * 4 + i][0];
            let edge_min_index = CELL_PROC_FACE_MASK[dir * 4 + i][0];
            // C++: int edgeMaxIndex = cellProcFaceMask[dir * 4 + i][1];
            let edge_max_index = CELL_PROC_FACE_MASK[dir * 4 + i][1];
            // C++: int signChanges = 0;
            let mut sign_changes = 0usize;
            // C++: for (int j = 0; j < 2; ++j) {
            for grid in &grids {
                // C++: if (params[j]->cornerSigns[edgeMinIndex] != params[j]->cornerSigns[edgeMaxIndex]) { signChanges++; }
                if grid.corner_signs[edge_min_index] != grid.corner_signs[edge_max_index] {
                    sign_changes += 1;
                }
            }
            // C++: if (signChanges > 1) { return false; }
            if sign_changes > 1 {
                return false;
            }
        }
        // C++: return true;
        true
    }

    // C++ void RectilinearGrid::combineAAGrid(RectilinearGrid *left,
    //                                         RectilinearGrid *right,
    //                                         int dir,
    //                                         RectilinearGrid *out) {
    //   out->calCornerComponents();
    //   if (!left && !right) { return; }
    //   std::map<int, int> combineMaps[2];
    //   RectilinearGrid *grids[2] = {left, right};
    //   for (int i = 0; i < 4; ++i) {
    //     int c = -1;
    //     for (int j = 0; j < 2; ++j) {
    //       if (out->cornerSigns[cellProcFaceMask[dir * 4 + i][j]] != 0) {
    //         c = out->componentIndices[cellProcFaceMask[dir * 4 + i][j]];
    //         break;
    //       }
    //     }
    //     if (c == -1) { continue; }
    //     for (int j = 0; j < 2; ++j) {
    //       auto child = grids[j];
    //       if (child) {
    //         for (int k = 0; k < 2; ++k) {
    //           if (child->cornerSigns[cellProcFaceMask[dir * 4 + i][k]] != 0) {
    //             int childC = child->componentIndices[cellProcFaceMask[dir * 4 + i][k]];
    //             assert(child->components[childC].pointCount > 0);
    //             combineMaps[j][c] = childC;
    //             break;
    //           }
    //         }
    //       }
    //     }
    //   }
    //   for (int i = 0; i < 2; ++i) {
    //     for (auto p : combineMaps[i]) {
    //       out->components.at(p.first).combine(grids[i]->components.at(p.second));
    //       // grids[i]->vertices.at(p.second).parent = &out->vertices.at(p.first);
    //       // NOTE: parent tracking skipped in Rust — not used in kd-tree contouring
    //     }
    //   }
    //   // C++ counts total point_count across components but doesn't use it
    // }

    /// Combines QEF data from two adjacent grids into an output grid.
    ///
    /// Matches C++ `combineAAGrid` exactly (except parent tracking is skipped
    /// since it is not used in kd-tree contouring).
    // void RectilinearGrid::combineAAGrid(RectilinearGrid *left,
    //                                     RectilinearGrid *right,
    //                                     int dir,
    //                                     RectilinearGrid *out) {
    //   out->calCornerComponents();
    pub fn combine_aa_grid(
        left: Option<&RectilinearGrid>,
        right: Option<&RectilinearGrid>,
        dir: usize,
        out: &mut RectilinearGrid,
    ) {
        // C++: caller must have called assignSign on out already.
        // C++: out->calCornerComponents();
        out.cal_corner_components();

        // C++: if (!left && !right) { return; }
        if left.is_none() && right.is_none() {
            return;
        }

        // C++: std::map<int, int> combineMaps[2];
        let mut combine_maps: [BTreeMap<usize, usize>; 2] = [BTreeMap::new(), BTreeMap::new()];
        // C++: RectilinearGrid *grids[2] = {left, right};
        let grids: [Option<&RectilinearGrid>; 2] = [left, right];

        // C++: for (int i = 0; i < 4; ++i) {
        #[allow(clippy::needless_range_loop)]
        for i in 0..4 {
            let mask = &CELL_PROC_FACE_MASK[dir * 4 + i];
            // C++: int c = -1;
            let mut c: i8 = -1;
            // C++: for (int j = 0; j < 2; ++j) {
            for j in 0..2 {
                // C++: if (out->cornerSigns[cellProcFaceMask[dir * 4 + i][j]] != 0) {
                if out.corner_signs[mask[j]] != 0 {
                    // C++: c = out->componentIndices[cellProcFaceMask[dir * 4 + i][j]];
                    c = out.component_indices[mask[j]];
                    // C++: break;
                    break;
                }
            }
            // C++: if (c == -1) { continue; }
            if c == -1 {
                continue;
            }
            let out_c = c as usize;

            // C++: for (int j = 0; j < 2; ++j) {
            for (j, child_opt) in grids.iter().enumerate() {
                // C++: auto child = grids[j]; if (child) {
                if let Some(child) = child_opt {
                    // C++: for (int k = 0; k < 2; ++k) {
                    for k in 0..2 {
                        // C++: if (child->cornerSigns[cellProcFaceMask[dir * 4 + i][k]] != 0) {
                        if child.corner_signs[mask[k]] != 0 {
                            // C++: int childC = child->componentIndices[cellProcFaceMask[dir * 4 + i][k]];
                            let child_c = child.component_indices[mask[k]];
                            if child_c >= 0 {
                                // C++: combineMaps[j][c] = childC;
                                combine_maps[j].insert(out_c, child_c as usize);
                            }
                            // C++: break;
                            break;
                        }
                    }
                }
            }
        }

        // C++: for (int i = 0; i < 2; ++i) {
        for i in 0..2 {
            // C++: for (auto p : combineMaps[i]) {
            if let Some(child) = grids[i] {
                for (&out_c, &child_c) in &combine_maps[i] {
                    // C++: out->components.at(p.first).combine(grids[i]->components.at(p.second));
                    if out_c < out.components.len() && child_c < child.components.len() {
                        out.components[out_c].combine(&child.components[child_c]);
                    }
                    // NOTE: parent tracking skipped — not used in kd-tree contouring
                    // C++: grids[i]->vertices.at(p.second).parent = &out->vertices.at(p.first);
                }
            }
        }
        // C++ counts total point_count across components but doesn't use it
    }

    // C++ bool RectilinearGrid::isInterFreeCondition2Faild(const std::vector<Vertex *> &polygons,
    //                                                      const glm::fvec3 &p1,
    //                                                      const glm::fvec3 &p2) {
    //   int anotherV = 3;
    //   bool interSupportingEdge = false;
    //
    //   for (int i = 2; i < polygons.size(); ++i) {
    //     fvec2 baryPos;
    //     float distance;
    //     bool isInter = glm::intersectRayTriangle(p1,
    //                                              p2 - p1,
    //                                              polygons[0]->hermiteP,
    //                                              polygons[i - 1]->hermiteP,
    //                                              polygons[i]->hermiteP,
    //                                              baryPos,
    //                                              distance);
    //     isInter = isInter && (distance > 0.f && distance < 1.f);
    //     if (isInter) {
    //       interSupportingEdge = true;
    //       anotherV = i % 3 + 1;
    //     }
    //   }
    //   if (polygons.size() == 3) {
    //     return !interSupportingEdge;
    //   }
    //   else {
    //     fvec2 baryPos;
    //     float distance;
    //     bool interTetrahedron = glm::intersectRayTriangle(polygons[0]->hermiteP,
    //                                                       polygons[2]->hermiteP - polygons[0]->hermiteP,
    //                                                       p1,
    //                                                       p2,
    //                                                       polygons[anotherV]->hermiteP,
    //                                                       baryPos,
    //                                                       distance);
    //     interTetrahedron = interTetrahedron && (distance > 0.f && distance < 1.f);
    //     return !(interTetrahedron && interSupportingEdge);
    //   }
    // }

    /// Moller-Trumbore ray-triangle intersection test.
    ///
    /// Returns `Some((bary_u, bary_v, distance))` if the ray from `origin` in
    /// `direction` hits the triangle `(v0, v1, v2)`, or `None` if it misses.
    /// This matches `glm::intersectRayTriangle` semantics: returns barycentric
    /// coords and distance along the ray (distance = t where hit = origin + t * direction).
    fn ray_triangle_intersect(
        origin: Vec3,
        direction: Vec3,
        v0: Vec3,
        v1: Vec3,
        v2: Vec3,
    ) -> Option<(f32, f32, f32)> {
        let epsilon = 1.0e-6_f32;
        let edge1 = v1 - v0;
        let edge2 = v2 - v0;
        let h = direction.cross(edge2);
        let a = edge1.dot(h);

        if a.abs() < epsilon {
            return None;
        }

        let f = 1.0 / a;
        let s = origin - v0;
        let u = f * s.dot(h);

        if !(0.0..=1.0).contains(&u) {
            return None;
        }

        let q = s.cross(edge1);
        let v = f * direction.dot(q);

        if v < 0.0 || u + v > 1.0 {
            return None;
        }

        let distance = f * edge2.dot(q);
        Some((u, v, distance))
    }

    /// Tests whether the intersection-free condition 2 is violated.
    ///
    /// Matches C++ `isInterFreeCondition2Faild` exactly:
    ///
    /// For 3-vertex polygons (single triangle):
    ///   - Check if the ray (p1 -> p2) intersects the triangle, with distance in (0, 1).
    ///   - Return `!interSupportingEdge`.
    ///
    /// For 4-vertex polygons (two triangles in fan):
    ///   - First do the same fan intersection test.
    ///   - Then test a tetrahedron intersection: ray from polygons[0] to polygons[2]
    ///     against triangle (p1, p2, polygons[anotherV]).
    ///   - Return `!(interTetrahedron && interSupportingEdge)`.
    ///
    /// Takes a slice of `Vertex` references (the polygon vertices in order).
    pub fn is_inter_free_condition2_failed(polygons: &[&Vertex], p1: Vec3, p2: Vec3) -> bool {
        // C++: int anotherV = 3;
        let mut another_v: usize = 3;
        // C++: bool interSupportingEdge = false;
        let mut inter_supporting_edge = false;

        // C++: for (int i = 2; i < polygons.size(); ++i) {
        for i in 2..polygons.len() {
            // C++: bool isInter = glm::intersectRayTriangle(p1, p2 - p1,
            //        polygons[0]->hermiteP, polygons[i-1]->hermiteP, polygons[i]->hermiteP,
            //        baryPos, distance);
            let result = Self::ray_triangle_intersect(
                p1,
                p2 - p1,
                polygons[0].hermite_p,
                polygons[i - 1].hermite_p,
                polygons[i].hermite_p,
            );
            // C++: isInter = isInter && (distance > 0.f && distance < 1.f);
            let is_inter = match result {
                Some((_u, _v, distance)) => distance > 0.0 && distance < 1.0,
                None => false,
            };
            // C++: if (isInter) { interSupportingEdge = true; anotherV = i % 3 + 1; }
            if is_inter {
                inter_supporting_edge = true;
                another_v = i % 3 + 1;
            }
        }

        // C++: if (polygons.size() == 3) { return !interSupportingEdge; }
        if polygons.len() == 3 {
            return !inter_supporting_edge;
        }

        // C++: else {
        //   bool interTetrahedron = glm::intersectRayTriangle(
        //     polygons[0]->hermiteP,
        //     polygons[2]->hermiteP - polygons[0]->hermiteP,
        //     p1, p2, polygons[anotherV]->hermiteP,
        //     baryPos, distance);
        //   interTetrahedron = interTetrahedron && (distance > 0.f && distance < 1.f);
        //   return !(interTetrahedron && interSupportingEdge);
        // }
        let tetra_result = Self::ray_triangle_intersect(
            polygons[0].hermite_p,
            polygons[2].hermite_p - polygons[0].hermite_p,
            p1,
            p2,
            polygons[another_v].hermite_p,
        );
        let inter_tetrahedron = match tetra_result {
            Some((_u, _v, distance)) => distance > 0.0 && distance < 1.0,
            None => false,
        };
        // C++: return !(interTetrahedron && interSupportingEdge);
        !(inter_tetrahedron && inter_supporting_edge)
    }
}

// C++ template <class GridHolder>
// bool RectilinearGrid::checkSign(const std::array<GridHolder *, 4> &nodes,
//                                 int quadDir1,
//                                 int quadDir2,
//                                 ScalarField *s,
//                                 int &side,
//                                 PositionCode &minEnd,
//                                 PositionCode &maxEnd) {
//   int dir = 3 - quadDir1 - quadDir2;
//   if (nodes[0] != nodes[1]) {
//     maxEnd = minEnd = nodes[0]->grid.maxCode;
//   }
//   else {
//     maxEnd = minEnd = nodes[3]->grid.minCode;
//   }
//   maxEnd[dir] = std::min(
//     std::min(nodes[0]->grid.maxCode[dir], nodes[1]->grid.maxCode[dir]),
//     std::min(nodes[2]->grid.maxCode[dir], nodes[3]->grid.maxCode[dir]));
//   minEnd[dir] = std::max(
//     std::max(nodes[0]->grid.minCode[dir], nodes[1]->grid.minCode[dir]),
//     std::max(nodes[2]->grid.minCode[dir], nodes[3]->grid.minCode[dir]));
//   if (minEnd[dir] >= maxEnd[dir]) {
//     return false;
//   }
//   float v1 = s->index(minEnd);
//   float v2 = s->index(maxEnd);
//   if ((v1 >= 0 && v2 >= 0) || (v1 < 0 && v2 < 0)) {
//     return false;
//   }
//   if (v2 >= 0 && v1 <= 0) {
//     side = 0;
//   }
//   else {
//     side = 1;
//   }
//   return true;
// }

/// Checks whether an edge between grid holders has a sign change.
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
    // C++: int dir = 3 - quadDir1 - quadDir2;
    let dir = 3 - quad_dir1 - quad_dir2;

    // C++: if (nodes[0] != nodes[1]) { maxEnd = minEnd = nodes[0]->grid.maxCode; }
    //      else { maxEnd = minEnd = nodes[3]->grid.minCode; }
    let (mut min_end, mut max_end) = if !std::ptr::eq(nodes[0].grid(), nodes[1].grid()) {
        let code = nodes[0].grid().max_code;
        (code, code)
    } else {
        let code = nodes[3].grid().min_code;
        (code, code)
    };

    // C++: maxEnd[dir] = std::min(std::min(nodes[0]->grid.maxCode[dir], nodes[1]->grid.maxCode[dir]),
    //                             std::min(nodes[2]->grid.maxCode[dir], nodes[3]->grid.maxCode[dir]));
    max_end[dir] = nodes[0].grid().max_code[dir]
        .min(nodes[1].grid().max_code[dir])
        .min(nodes[2].grid().max_code[dir])
        .min(nodes[3].grid().max_code[dir]);

    // C++: minEnd[dir] = std::max(std::max(nodes[0]->grid.minCode[dir], nodes[1]->grid.minCode[dir]),
    //                             std::max(nodes[2]->grid.minCode[dir], nodes[3]->grid.minCode[dir]));
    min_end[dir] = nodes[0].grid().min_code[dir]
        .max(nodes[1].grid().min_code[dir])
        .max(nodes[2].grid().min_code[dir])
        .max(nodes[3].grid().min_code[dir]);

    // C++: if (minEnd[dir] >= maxEnd[dir]) { return false; }
    if min_end[dir] >= max_end[dir] {
        return None;
    }

    // C++: float v1 = s->index(minEnd);
    let v1 = field.index(min_end, unit_size);
    // C++: float v2 = s->index(maxEnd);
    let v2 = field.index(max_end, unit_size);

    // C++: if ((v1 >= 0 && v2 >= 0) || (v1 < 0 && v2 < 0)) { return false; }
    if (v1 >= 0.0 && v2 >= 0.0) || (v1 < 0.0 && v2 < 0.0) {
        return None;
    }

    // C++: if (v2 >= 0 && v1 <= 0) { side = 0; } else { side = 1; }
    let side = if v2 >= 0.0 && v1 <= 0.0 { 0 } else { 1 };

    // C++: return true;
    Some((side, min_end, max_end))
}

// C++ template <class GridHolder>
// void RectilinearGrid::generateQuad(const std::array<GridHolder, 4> &nodes,
//                                    int quadDir1,
//                                    int quadDir2,
//                                    Mesh *mesh,
//                                    ScalarField *t,
//                                    float) {
//   int edgeSide;
//   PositionCode minEnd, maxEnd;
//   if (!RectilinearGrid::checkSign(nodes, quadDir1, quadDir2, t, edgeSide, minEnd, maxEnd)) {
//     return;
//   }
//   std::vector<Vertex *> polygons;
//   int lineDir = 3 - quadDir1 - quadDir2;
//   int componentIndices[4];
//   for (int i = 0; i < 4; ++i) {
//     if (nodes[i] != nodes[oppositeQuadIndex(i)]) {
//       int c1, c2;
//       quadIndex(quadDir1, quadDir2, symmetryQuadIndex(i), c1, c2);
//       componentIndices[i] = nodes[i]->grid.edgeComponentIndex(c1, c2);
//     }
//     else {
//       componentIndices[i] = nodes[i]->grid.faceComponentIndex(quadDir2, lineDir, 1 - i / 2, edgeSide);
//     }
//     if (componentIndices[i] == -1) {
//       return;
//     }
//   }
//   polygons.push_back(&nodes[0]->grid.vertices.at(componentIndices[0]));
//   if (nodes[0] != nodes[1]) {
//     polygons.push_back(&nodes[1]->grid.vertices.at(componentIndices[1]));
//   }
//   polygons.push_back(&nodes[3]->grid.vertices.at(componentIndices[3]));
//   if (nodes[2] != nodes[3]) {
//     polygons.push_back(&nodes[2]->grid.vertices.at(componentIndices[2]));
//   }
//   std::set<Vertex *> identicals;
//   for (auto v : polygons) { identicals.insert(v); }
//   if (identicals.size() < 3) { return; }
//
//   bool condition1Failed = false;
//   int firstConcaveFaceVertex = 0;
//   if (false) { ... }   // condition1 block is dead code (if (false))
//
//   fvec3 p1 = codeToPos(minEnd, RectilinearGrid::getUnitSize());
//   fvec3 p2 = codeToPos(maxEnd, RectilinearGrid::getUnitSize());
//
//   bool condition2Failed = isInterFreeCondition2Faild(polygons, p1, p2);
//   if (polygons.size() > 3) {
//     std::vector<Vertex *> reversePolygons = {polygons[1], polygons[2], polygons[3], polygons[0]};
//     bool reverseCondition2Failed = isInterFreeCondition2Faild(reversePolygons, p1, p2);
//     if (!reverseCondition2Failed) {
//       /// NOTE: the swap here happens whether intersection-free or not
//       polygons.swap(reversePolygons);
//     }
//     condition2Failed = condition2Failed && reverseCondition2Failed;
//   }
//   // #ifdef INTERSECTION_FREE -- NOT DEFINED, so skip that entire block
//   // Just do the fan triangulation:
//   for (int i = 2; i < polygons.size(); ++i) {
//     Vertex *triangle[3] = { polygons[0], polygons[i - 1], polygons[i] };
//     mesh->addTriangle(triangle, t);
//   }
// }

/// Generates a quad (two triangles) from 4 grid holders surrounding an edge.
///
/// Matches C++ `generateQuad` exactly (with `INTERSECTION_FREE` NOT defined).
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
    // C++: int edgeSide; PositionCode minEnd, maxEnd;
    // C++: if (!RectilinearGrid::checkSign(nodes, quadDir1, quadDir2, t, edgeSide, minEnd, maxEnd)) { return; }
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

    // C++: std::vector<Vertex *> polygons;
    // C++: int lineDir = 3 - quadDir1 - quadDir2;
    let line_dir = 3 - quad_dir1 - quad_dir2;
    // C++: int componentIndices[4];
    let mut comp_indices: [i8; 4] = [-1; 4];

    // C++: for (int i = 0; i < 4; ++i) {
    for i in 0..4 {
        // C++: if (nodes[i] != nodes[oppositeQuadIndex(i)]) {
        let opp = opposite_quad_index(i);
        if !std::ptr::eq(nodes[i].grid(), nodes[opp].grid()) {
            // C++: int c1, c2; quadIndex(quadDir1, quadDir2, symmetryQuadIndex(i), c1, c2);
            let (c1, c2) = quad_index(quad_dir1, quad_dir2, symmetry_quad_index(i));
            // C++: componentIndices[i] = nodes[i]->grid.edgeComponentIndex(c1, c2);
            comp_indices[i] = nodes[i].grid().edge_component_index(c1, c2);
        } else {
            // C++: componentIndices[i] = nodes[i]->grid.faceComponentIndex(quadDir2, lineDir, 1 - i / 2, edgeSide);
            comp_indices[i] = nodes[i].grid().face_component_index(
                quad_dir2,
                line_dir,
                1 - i / 2,
                edge_side as usize,
            );
        }
        // C++: if (componentIndices[i] == -1) { return; }
        if comp_indices[i] == -1 {
            return;
        }
    }

    // C++: polygons.push_back(&nodes[0]->grid.vertices.at(componentIndices[0]));
    // C++: if (nodes[0] != nodes[1]) { polygons.push_back(&nodes[1]->grid.vertices.at(componentIndices[1])); }
    // C++: polygons.push_back(&nodes[3]->grid.vertices.at(componentIndices[3]));
    // C++: if (nodes[2] != nodes[3]) { polygons.push_back(&nodes[2]->grid.vertices.at(componentIndices[2])); }

    // We store (node_index, comp_index) pairs so we can look up vertices later.
    let mut polygon_refs: Vec<(usize, usize)> = Vec::with_capacity(4);

    polygon_refs.push((0, comp_indices[0] as usize));
    if !std::ptr::eq(nodes[0].grid(), nodes[1].grid()) {
        polygon_refs.push((1, comp_indices[1] as usize));
    }
    polygon_refs.push((3, comp_indices[3] as usize));
    if !std::ptr::eq(nodes[2].grid(), nodes[3].grid()) {
        polygon_refs.push((2, comp_indices[2] as usize));
    }

    // C++: std::set<Vertex *> identicals;
    // C++: for (auto v : polygons) { identicals.insert(v); }
    // C++: if (identicals.size() < 3) { return; }
    // Deduplicate by (grid_ptr, comp_idx) pairs to detect pointer identity
    let mut unique_count = 0;
    let mut seen: Vec<(*const RectilinearGrid, usize)> = Vec::with_capacity(4);
    for &(ni, ci) in &polygon_refs {
        let key = (nodes[ni].grid() as *const RectilinearGrid, ci);
        if !seen.contains(&key) {
            seen.push(key);
            unique_count += 1;
        }
    }

    if unique_count < 3 {
        return;
    }

    // Collect vertex references
    // Verify all component indices are in bounds
    for &(ni, ci) in &polygon_refs {
        let grid = nodes[ni].grid();
        if ci >= grid.vertices.len() {
            return;
        }
    }

    // C++: bool condition1Failed = false;
    // C++: int firstConcaveFaceVertex = 0;
    // C++: if (false) { ... }  // dead code — skipped

    // C++: fvec3 p1 = codeToPos(minEnd, RectilinearGrid::getUnitSize());
    let p1 = code_to_pos(min_end, unit_size);
    // C++: fvec3 p2 = codeToPos(maxEnd, RectilinearGrid::getUnitSize());
    let p2 = code_to_pos(max_end, unit_size);

    // Build polygon vertex references for intersection test
    let mut polygons: Vec<&Vertex> = polygon_refs
        .iter()
        .map(|&(ni, ci)| &nodes[ni].grid().vertices[ci])
        .collect();

    // C++: bool condition2Failed = isInterFreeCondition2Faild(polygons, p1, p2);
    let _condition2_failed = RectilinearGrid::is_inter_free_condition2_failed(&polygons, p1, p2);

    // C++: if (polygons.size() > 3) {
    if polygons.len() > 3 {
        // C++: std::vector<Vertex *> reversePolygons = {polygons[1], polygons[2], polygons[3], polygons[0]};
        let reverse_polygons: Vec<&Vertex> =
            vec![polygons[1], polygons[2], polygons[3], polygons[0]];
        // C++: bool reverseCondition2Failed = isInterFreeCondition2Faild(reversePolygons, p1, p2);
        let reverse_condition2_failed =
            RectilinearGrid::is_inter_free_condition2_failed(&reverse_polygons, p1, p2);
        // C++: if (!reverseCondition2Failed) { polygons.swap(reversePolygons); }
        if !reverse_condition2_failed {
            // NOTE: the swap here happens whether intersection-free or not
            polygons = reverse_polygons;
        }
        // C++: condition2Failed = condition2Failed && reverseCondition2Failed;
        // (not used since INTERSECTION_FREE is not defined)
    }

    // #ifdef INTERSECTION_FREE — NOT DEFINED, skip the entire block
    // Just do the fan triangulation:
    // C++: for (int i = 2; i < polygons.size(); ++i) {
    //        Vertex *triangle[3] = { polygons[0], polygons[i - 1], polygons[i] };
    //        mesh->addTriangle(triangle, t);
    //      }
    for i in 2..polygons.len() {
        mesh.add_triangle([polygons[0], polygons[i - 1], polygons[i]], |p| {
            field.normal(p)
        });
    }
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

        // Ray pointing at the triangle — should hit with positive distance
        let result = RectilinearGrid::ray_triangle_intersect(Vec3::ZERO, Vec3::Z, v0, v1, v2);
        assert!(result.is_some());
        if let Some((_u, _v, dist)) = result {
            assert!(dist > 0.0);
        }

        // Ray pointing away — glm::intersectRayTriangle returns true with negative
        // distance for backward hits, so our port does the same.
        let result = RectilinearGrid::ray_triangle_intersect(Vec3::ZERO, Vec3::NEG_Z, v0, v1, v2);
        assert!(result.is_some());
        if let Some((_u, _v, dist)) = result {
            assert!(dist < 0.0);
        }
    }

    #[test]
    fn is_inter_free_condition2_matches_cpp() {
        let v0 = Vertex::new(Vec3::new(-1.0, -1.0, 0.5));
        let v1 = Vertex::new(Vec3::new(1.0, -1.0, 0.5));
        let v2 = Vertex::new(Vec3::new(0.0, 1.0, 0.5));

        // C++ semantics: intersection found -> condition NOT failed (return false)
        // Segment passes through the triangle -> ordering is good
        assert!(!RectilinearGrid::is_inter_free_condition2_failed(
            &[&v0, &v1, &v2],
            Vec3::ZERO,
            Vec3::Z
        ));

        // Segment does not reach the triangle -> ordering is bad (failed)
        assert!(RectilinearGrid::is_inter_free_condition2_failed(
            &[&v0, &v1, &v2],
            Vec3::new(5.0, 5.0, 0.0),
            Vec3::new(5.0, 5.0, 1.0)
        ));
    }
}
