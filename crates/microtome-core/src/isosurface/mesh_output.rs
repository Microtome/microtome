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

    /// Adds a triangle from three vertices, potentially splitting vertices
    /// whose normals differ significantly from the field normal at a
    /// slightly offset position (for better shading at sharp features).
    pub fn add_triangle<F>(&mut self, vertices: [&Vertex; 3], normal_fn: F)
    where
        F: Fn(Vec3) -> Vec3,
    {
        let cos_threshold = (15.0_f32).to_radians().cos();

        for j in 0..3 {
            let target = vertices[j];
            let adj0 = vertices[(j + 1) % 3];
            let adj1 = vertices[(j + 2) % 3];

            let offset =
                (adj1.hermite_p - target.hermite_p + adj0.hermite_p - target.hermite_p) * 0.05;
            let normal = normal_fn(target.hermite_p + offset);

            if target.hermite_n.dot(normal) < cos_threshold {
                // Sharp feature: emit a new vertex with the offset normal
                let idx = self.positions.len() as u32;
                self.positions.push(target.hermite_p);
                self.normals.push(normal);
                self.indices.push(idx);
            } else {
                // Smooth: reuse existing vertex
                self.indices.push(target.vertex_index);
            }
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
            let i0 = self.indices[i * 3] as usize;
            let i1 = self.indices[i * 3 + 1] as usize;
            let i2 = self.indices[i * 3 + 2] as usize;

            let normal = self.normals[i0] + self.normals[i1] + self.normals[i2];

            let c1_raw = self.positions[i0] - self.positions[i1];
            let c2 = (self.positions[i0] - self.positions[i2]).normalize_or_zero();
            let mut c1 = c1_raw.normalize_or_zero();

            // Orthogonalize c1 against c2
            c1 -= c1.dot(c2) * c2;
            c1 = c1.normalize_or_zero();

            // Project normal onto the plane perpendicular to c1 and c2
            let mut n = normal;
            n -= n.dot(c1) * c1;
            n -= n.dot(c2) * c2;

            if n == Vec3::ZERO {
                continue;
            }
            n = n.normalize();

            for j in 0..3 {
                flat_normals.push(n);
                flat_positions.push(self.positions[self.indices[i * 3 + j] as usize]);
                flat_indices.push((3 * i + j) as u32);
            }
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
