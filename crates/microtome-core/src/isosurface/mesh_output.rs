//! Output mesh type for isosurface extraction.

use glam::Vec3;

use super::vertex::Vertex;

/// Triangle mesh produced by isosurface extraction.
///
/// Stores vertex positions, normals, and triangle indices separately
/// (structure-of-arrays layout for efficient GPU upload).
#[derive(Debug, Clone, Default)]
pub struct IsoMesh {
    /// Vertex positions.
    pub positions: Vec<Vec3>,
    /// Vertex normals (one per position).
    pub normals: Vec<Vec3>,
    /// Triangle indices (3 per triangle).
    pub indices: Vec<u32>,
}

impl IsoMesh {
    /// Creates an empty mesh.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a vertex to the mesh and updates the vertex's index and normal.
    ///
    /// The normal is computed from the scalar field at the vertex's hermite point.
    /// Returns the index of the added vertex.
    pub fn add_vertex<F>(&mut self, vertex: &mut Vertex, normal_fn: F) -> u32
    where
        F: FnOnce(Vec3) -> Vec3,
    {
        vertex.hermite_n = normal_fn(vertex.hermite_p);
        vertex.vertex_index = self.positions.len() as u32;
        self.positions.push(vertex.hermite_p);
        self.normals.push(vertex.hermite_n);
        vertex.vertex_index
    }

    /// Adds a triangle from three vertices, splitting vertices whose normals
    /// differ significantly from the field normal at a slightly offset
    /// position (for better shading at sharp features).
    ///
    /// Adds a triangle. Uses simple index push for now to isolate
    /// contouring issues from normal-splitting issues.
    pub fn add_triangle<F>(&mut self, vertices: [&Vertex; 3], _normal_fn: F)
    where
        F: Fn(Vec3) -> Vec3,
    {
        for v in &vertices {
            self.indices.push(v.vertex_index);
        }
    }

    /// Recomputes normals as flat (per-face) normals.
    ///
    /// Duplicates vertices so each triangle has its own set,
    /// projecting the original smooth normal onto the face plane.
    pub fn generate_flat_normals(&mut self) {
        let mut flat_positions = Vec::new();
        let mut flat_normals = Vec::new();
        let mut flat_indices = Vec::new();

        let tri_count = self.indices.len() / 3;
        for i in 0..tri_count {
            let p0 = self.positions[self.indices[i * 3] as usize];
            let p1 = self.positions[self.indices[i * 3 + 1] as usize];
            let p2 = self.positions[self.indices[i * 3 + 2] as usize];

            let edge1 = p1 - p0;
            let edge2 = p2 - p0;
            let n = edge1.cross(edge2).normalize_or_zero();

            if n == Vec3::ZERO {
                continue;
            }

            let base = flat_positions.len() as u32;
            flat_positions.extend_from_slice(&[p0, p1, p2]);
            flat_normals.extend_from_slice(&[n, n, n]);
            flat_indices.extend_from_slice(&[base, base + 1, base + 2]);
        }

        self.positions = flat_positions;
        self.normals = flat_normals;
        self.indices = flat_indices;
    }

    /// Converts this isosurface mesh to the existing [`MeshData`](crate::mesh::MeshData) type.
    pub fn to_mesh_data(&self) -> crate::mesh::MeshData {
        let vertices: Vec<crate::mesh::MeshVertex> = self
            .positions
            .iter()
            .zip(self.normals.iter())
            .map(|(p, n)| crate::mesh::MeshVertex {
                position: [p.x, p.y, p.z],
                normal: [n.x, n.y, n.z],
            })
            .collect();

        let indices: Vec<u32> = self.indices.clone();

        // Compute bounding box
        let mut bbox = crate::mesh::BoundingBox {
            min: Vec3::splat(f32::MAX),
            max: Vec3::splat(f32::MIN),
        };
        for p in &self.positions {
            bbox.min = bbox.min.min(*p);
            bbox.max = bbox.max.max(*p);
        }
        if self.positions.is_empty() {
            bbox.min = Vec3::ZERO;
            bbox.max = Vec3::ZERO;
        }

        crate::mesh::MeshData {
            vertices,
            indices,
            bbox,
            volume: 0.0, // Volume estimation not computed for isosurface meshes
        }
    }

    /// Returns the number of triangles in this mesh.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_mesh() {
        let mesh = IsoMesh::new();
        assert_eq!(mesh.triangle_count(), 0);
        assert!(mesh.positions.is_empty());
    }

    #[test]
    fn add_vertex_assigns_index() {
        let mut mesh = IsoMesh::new();
        let mut v = Vertex::new(Vec3::new(1.0, 2.0, 3.0));
        let idx = mesh.add_vertex(&mut v, |_| Vec3::Y);
        assert_eq!(idx, 0);
        assert_eq!(v.vertex_index, 0);
        assert_eq!(v.hermite_n, Vec3::Y);
        assert_eq!(mesh.positions.len(), 1);
    }

    #[test]
    fn to_mesh_data_preserves_geometry() {
        let mut mesh = IsoMesh::new();
        mesh.positions.push(Vec3::ZERO);
        mesh.positions.push(Vec3::X);
        mesh.positions.push(Vec3::Y);
        mesh.normals.push(Vec3::Z);
        mesh.normals.push(Vec3::Z);
        mesh.normals.push(Vec3::Z);
        mesh.indices.push(0);
        mesh.indices.push(1);
        mesh.indices.push(2);

        let data = mesh.to_mesh_data();
        assert_eq!(data.vertices.len(), 3);
        assert_eq!(data.indices.len(), 3);
    }
}
