//! Lookup tables and encoding helpers for dual contouring cell topology.
//!
//! These tables encode the relationships between corners, edges, and faces
//! of a unit cube used in the dual contouring algorithm.

use glam::{IVec3, Vec3};

/// Integer coordinate in the voxel grid.
pub type PositionCode = IVec3;

/// Converts a 3D cell coordinate (each component 0 or 1) to a linear index 0–7.
///
/// Encoding: `x * 4 + y * 2 + z`.
pub fn encode_cell(code: IVec3) -> usize {
    (code.x * 4 + code.y * 2 + code.z) as usize
}

/// Converts a linear index 0–7 back to a 3D cell coordinate.
pub fn decode_cell(i: usize) -> IVec3 {
    DECODE_CELL[i]
}

/// Unit cube corner offsets (float), indexed 0–7.
pub fn min_offset_subdivision(i: usize) -> Vec3 {
    MIN_OFFSET_SUBDIVISION[i]
}

/// Axis basis vectors: 0 → X, 1 → Y, 2 → Z.
pub fn direction_map(i: usize) -> Vec3 {
    DIRECTION_MAP[i]
}

/// Face subdivision offsets for a given axis direction and corner index.
pub fn face_subdivision(dir: usize, i: usize) -> Vec3 {
    FACE_SUBDIVISION[dir][i]
}

/// Edge processing direction vectors.
pub fn edge_proc_dir(i: usize, j: usize) -> Vec3 {
    EDGE_PROC_DIR[i][j]
}

/// Computes two cell corner indices for a quad face.
///
/// Given two quad directions and an index 0–3, returns the two corner
/// indices (p1, p2) that share the quad edge.
pub fn quad_index(quad_dir1: usize, quad_dir2: usize, i: usize) -> (usize, usize) {
    let mut code = IVec3::ZERO;
    code[quad_dir1] = (i % 2) as i32;
    code[quad_dir2] = (i / 2) as i32;
    let p1 = encode_cell(code);
    code[3 - quad_dir1 - quad_dir2] = 1;
    let p2 = encode_cell(code);
    (p1, p2)
}

/// Returns the opposite corner index in a quad (0–3).
pub const fn opposite_quad_index(i: usize) -> usize {
    (i / 2) * 2 + 1 - i % 2
}

/// Returns the symmetric corner index in a quad (0–3).
pub const fn symmetry_quad_index(i: usize) -> usize {
    (1 - i / 2) * 2 + 1 - i % 2
}

/// Converts a voxel grid code to a world-space position.
pub fn code_to_pos(code: PositionCode, cell_size: f32) -> Vec3 {
    Vec3::new(
        code.x as f32 * cell_size,
        code.y as f32 * cell_size,
        code.z as f32 * cell_size,
    )
}

/// Converts a world-space position to a voxel grid code (rounding).
pub fn pos_to_code(pos: Vec3, cell_size: f32) -> PositionCode {
    IVec3::new(
        (pos.x / cell_size).round() as i32,
        (pos.y / cell_size).round() as i32,
        (pos.z / cell_size).round() as i32,
    )
}

/// Converts a world-space position to a voxel grid code (floor).
pub fn pos_to_code_floor(pos: Vec3, cell_size: f32) -> PositionCode {
    IVec3::new(
        (pos.x / cell_size) as i32,
        (pos.y / cell_size) as i32,
        (pos.z / cell_size) as i32,
    )
}

/// Tests whether a line segment intersects an axis-aligned face.
pub fn segment_face_intersection(va: Vec3, vb: Vec3, min: Vec3, max: Vec3, dir: usize) -> bool {
    let l = (vb - va)[dir];
    if l.abs() < f32::EPSILON {
        return false;
    }
    let p = (min - va)[dir] / l * vb + (vb - min)[dir] / l * va;
    for i in 0..3 {
        if dir != i && (p[i] < min[i] || p[i] > max[i]) {
            return false;
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Static lookup tables
// ---------------------------------------------------------------------------

const DECODE_CELL: [IVec3; 8] = [
    IVec3::new(0, 0, 0),
    IVec3::new(0, 0, 1),
    IVec3::new(0, 1, 0),
    IVec3::new(0, 1, 1),
    IVec3::new(1, 0, 0),
    IVec3::new(1, 0, 1),
    IVec3::new(1, 1, 0),
    IVec3::new(1, 1, 1),
];

const MIN_OFFSET_SUBDIVISION: [Vec3; 8] = [
    Vec3::new(0.0, 0.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 1.0, 1.0),
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(1.0, 0.0, 1.0),
    Vec3::new(1.0, 1.0, 0.0),
    Vec3::new(1.0, 1.0, 1.0),
];

const DIRECTION_MAP: [Vec3; 3] = [
    Vec3::new(1.0, 0.0, 0.0),
    Vec3::new(0.0, 1.0, 0.0),
    Vec3::new(0.0, 0.0, 1.0),
];

const FACE_SUBDIVISION: [[Vec3; 4]; 3] = [
    [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 1.0, 1.0),
    ],
    [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 1.0),
    ],
    [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
    ],
];

const EDGE_PROC_DIR: [[Vec3; 4]; 3] = [
    [
        Vec3::new(0.0, -1.0, -1.0),
        Vec3::new(0.0, -1.0, 1.0),
        Vec3::new(0.0, 1.0, 1.0),
        Vec3::new(0.0, 1.0, -1.0),
    ],
    [
        Vec3::new(-1.0, 0.0, -1.0),
        Vec3::new(-1.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, 1.0),
        Vec3::new(1.0, 0.0, -1.0),
    ],
    [
        Vec3::new(-1.0, -1.0, 0.0),
        Vec3::new(1.0, -1.0, 0.0),
        Vec3::new(1.0, 1.0, 0.0),
        Vec3::new(-1.0, 1.0, 0.0),
    ],
];

/// Edge endpoint corner indices. 12 edges, each connecting 2 of 8 corners.
/// Edges 0–3: X-axis, 4–7: Y-axis, 8–11: Z-axis.
pub const EDGE_MAP: [[usize; 2]; 12] = [
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7], // x-axis
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7], // y-axis
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7], // z-axis
];

/// Cell-to-face processing mask: [12][child1, child2, face_dir].
pub const CELL_PROC_FACE_MASK: [[usize; 3]; 12] = [
    [0, 4, 0],
    [1, 5, 0],
    [2, 6, 0],
    [3, 7, 0],
    [0, 2, 1],
    [4, 6, 1],
    [1, 3, 1],
    [5, 7, 1],
    [0, 1, 2],
    [2, 3, 2],
    [4, 5, 2],
    [6, 7, 2],
];

/// Cell-to-edge processing mask: [6][4 children + edge_dir].
pub const CELL_PROC_EDGE_MASK: [[usize; 5]; 6] = [
    [0, 2, 1, 3, 0],
    [4, 6, 5, 7, 0],
    [0, 1, 4, 5, 1],
    [2, 3, 6, 7, 1],
    [0, 4, 2, 6, 2],
    [1, 5, 3, 7, 2],
];

/// Face-to-face processing mask: [3 dirs][4 sub-faces][child, child, face_dir].
pub const FACE_PROC_FACE_MASK: [[[usize; 3]; 4]; 3] = [
    [[4, 0, 0], [5, 1, 0], [6, 2, 0], [7, 3, 0]],
    [[2, 0, 1], [6, 4, 1], [3, 1, 1], [7, 5, 1]],
    [[1, 0, 2], [3, 2, 2], [5, 4, 2], [7, 6, 2]],
];

/// Face-to-edge processing mask: [3 dirs][4 sub-edges][6 values].
pub const FACE_PROC_EDGE_MASK: [[[usize; 6]; 4]; 3] = [
    [
        [1, 4, 5, 0, 1, 1],
        [1, 6, 7, 2, 3, 1],
        [0, 4, 6, 0, 2, 2],
        [0, 5, 7, 1, 3, 2],
    ],
    [
        [0, 2, 3, 0, 1, 0],
        [0, 6, 7, 4, 5, 0],
        [1, 2, 6, 0, 4, 2],
        [1, 3, 7, 1, 5, 2],
    ],
    [
        [1, 1, 3, 0, 2, 0],
        [1, 5, 7, 4, 6, 0],
        [0, 1, 5, 0, 4, 1],
        [0, 3, 7, 2, 6, 1],
    ],
];

/// Edge-to-edge processing mask: [3 dirs][2 halves][4 children].
pub const EDGE_PROC_EDGE_MASK: [[[usize; 4]; 2]; 3] = [
    [[3, 1, 2, 0], [7, 5, 6, 4]],
    [[5, 4, 1, 0], [7, 6, 3, 2]],
    [[6, 2, 4, 0], [7, 3, 5, 1]],
];

/// Node ordering for edge sign testing.
pub const EDGE_TEST_NODE_ORDER: [[usize; 2]; 4] = [[0, 1], [3, 2], [1, 2], [0, 3]];

/// Node ordering within faces.
pub const FACE_NODE_ORDER: [usize; 4] = [0, 0, 1, 1];

/// Direction-related edge lookup: [8 corners][8 corners][3 values].
/// -1 indicates no relation.
pub const DIR_RELATED_EDGE: [[[i32; 3]; 8]; 8] = [
    [
        [-1, -1, -1],
        [-1, 2, 6],
        [-1, 1, 10],
        [-1, -1, 0],
        [-1, 5, 9],
        [-1, -1, 4],
        [-1, -1, 8],
        [0, 4, 8],
    ],
    [
        [-1, 3, 11],
        [-1, -1, -1],
        [-1, -1, 1],
        [-1, 0, 10],
        [-1, -1, 5],
        [-1, 4, 9],
        [1, 5, 8],
        [-1, -1, 8],
    ],
    [
        [-1, 11, 3],
        [-1, -1, 2],
        [-1, -1, -1],
        [-1, 0, 6],
        [-1, -1, 9],
        [2, 4, 9],
        [-1, 5, 8],
        [-1, -1, 4],
    ],
    [
        [-1, -1, 3],
        [-1, 2, 11],
        [-1, 1, 7],
        [-1, -1, -1],
        [3, 5, 9],
        [-1, -1, 9],
        [-1, -1, 5],
        [-1, 4, 8],
    ],
    [
        [-1, 7, 11],
        [-1, -1, 5],
        [-1, -1, 10],
        [0, 6, 10],
        [-1, -1, -1],
        [-1, 2, 4],
        [-1, 1, 8],
        [-1, -1, 0],
    ],
    [
        [-1, -1, 7],
        [-1, 0, 11],
        [1, 7, 10],
        [-1, -1, 10],
        [-1, 3, 5],
        [-1, -1, -1],
        [-1, -1, 1],
        [-1, 1, 8],
    ],
    [
        [-1, -1, 11],
        [2, 6, 11],
        [-1, 7, 10],
        [-1, -1, 6],
        [-1, 3, 9],
        [-1, -1, 2],
        [-1, -1, -1],
        [-1, 1, 4],
    ],
    [
        [3, 7, 11],
        [-1, -1, 11],
        [-1, -1, 7],
        [-1, 6, 10],
        [-1, -1, 3],
        [-1, 2, 9],
        [-1, 1, 5],
        [-1, -1, -1],
    ],
];

/// Plane spreading direction lookup: [3 dirs][2 halves][4 corners].
pub const PLANE_SPREADING_DIR: [[[usize; 4]; 2]; 3] = [
    [[0, 2, 3, 1], [4, 6, 7, 5]],
    [[0, 1, 5, 4], [2, 3, 7, 6]],
    [[0, 4, 6, 2], [1, 5, 7, 2]],
];

/// Integration traversal order for octree corners.
pub const INTEGRAL_ORDER: [usize; 8] = [0, 1, 2, 4, 3, 5, 6, 7];

/// Edge processing mask for quad generation: [3 dirs][4 edges].
pub const PROCESS_EDGE_MASK: [[usize; 4]; 3] = [[3, 2, 1, 0], [7, 5, 6, 4], [11, 10, 9, 8]];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip() {
        for i in 0..8 {
            let code = decode_cell(i);
            assert_eq!(encode_cell(code), i);
        }
    }

    #[test]
    fn encode_cell_values() {
        assert_eq!(encode_cell(IVec3::new(0, 0, 0)), 0);
        assert_eq!(encode_cell(IVec3::new(0, 0, 1)), 1);
        assert_eq!(encode_cell(IVec3::new(1, 1, 1)), 7);
    }

    #[test]
    fn direction_map_is_unit_vectors() {
        for i in 0..3 {
            let d = direction_map(i);
            assert!((d.length() - 1.0).abs() < 1e-6);
        }
    }

    #[test]
    fn opposite_quad_index_is_involutory() {
        for i in 0..4 {
            assert_eq!(opposite_quad_index(opposite_quad_index(i)), i);
        }
    }

    #[test]
    fn code_to_pos_round_trip() {
        let code = IVec3::new(3, 5, 7);
        let cell_size = 0.5;
        let pos = code_to_pos(code, cell_size);
        let back = pos_to_code(pos, cell_size);
        assert_eq!(code, back);
    }
}
