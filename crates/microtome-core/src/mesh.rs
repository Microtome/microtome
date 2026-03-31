//! STL mesh loading, vertex data, and volume calculation.

use glam::Vec3;

use crate::error::{MicrotomeError, Result};

/// A single mesh vertex suitable for GPU upload.
///
/// Uses `#[repr(C)]` layout for direct mapping to wgpu vertex buffers.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MeshVertex {
    /// Position in model space.
    pub position: [f32; 3],
    /// Surface normal.
    pub normal: [f32; 3],
}

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy)]
pub struct BoundingBox {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl BoundingBox {
    /// Returns the center of the bounding box.
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Returns the size (extent) of the bounding box along each axis.
    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }
}

/// Raw mesh data loaded from an STL file.
///
/// Contains vertices, indices, bounding box, and precomputed volume.
#[derive(Debug, Clone)]
pub struct MeshData {
    /// Vertex data (position + normal per vertex).
    pub vertices: Vec<MeshVertex>,
    /// Triangle indices (3 per face).
    pub indices: Vec<u32>,
    /// Axis-aligned bounding box.
    pub bbox: BoundingBox,
    /// Signed volume of the mesh in model-space cubic units.
    pub volume: f64,
}

impl MeshData {
    /// Loads mesh data from an STL file (binary or ASCII).
    pub fn from_stl(reader: &mut (impl std::io::Read + std::io::Seek)) -> Result<Self> {
        let stl = stl_io::read_stl(reader).map_err(|e| MicrotomeError::StlParse(e.to_string()))?;

        let mut vertices = Vec::with_capacity(stl.faces.len() * 3);
        let mut indices = Vec::with_capacity(stl.faces.len() * 3);

        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut volume = 0.0_f64;

        for (face_idx, face) in stl.faces.iter().enumerate() {
            let sv1 = stl.vertices[face.vertices[0]];
            let mut sv2 = stl.vertices[face.vertices[1]];
            let mut sv3 = stl.vertices[face.vertices[2]];

            let v1 = Vec3::from(<[f32; 3]>::from(sv1));
            let mut v2 = Vec3::from(<[f32; 3]>::from(sv2));
            let mut v3 = Vec3::from(<[f32; 3]>::from(sv3));

            // The STL file's stored normal is the authoritative outward direction.
            // stl_io's IndexedMesh reindexing can reverse vertex order within faces,
            // flipping the winding. Check if the geometric normal (from cross product)
            // agrees with the stored normal; if not, swap two vertices to fix winding.
            let stl_normal = Vec3::from(<[f32; 3]>::from(face.normal));
            let edge1 = v2 - v1;
            let edge2 = v3 - v1;
            let geometric_normal = edge1.cross(edge2);

            if geometric_normal.dot(stl_normal) < 0.0 {
                // Winding disagrees with stored normal — swap v2 and v3
                std::mem::swap(&mut sv2, &mut sv3);
                std::mem::swap(&mut v2, &mut v3);
            }

            // Use the STL file's normal for lighting (it's the correct outward direction)
            let normal: [f32; 3] = if stl_normal.length_squared() > 0.0 {
                stl_normal.normalize().into()
            } else {
                // Zero normal in file — compute from winding (now corrected)
                let e1 = v2 - v1;
                let e2 = v3 - v1;
                let cn = e1.cross(e2);
                if cn.length_squared() > 0.0 {
                    cn.normalize().into()
                } else {
                    [0.0, 0.0, 1.0]
                }
            };

            // Update bounding box
            min = min.min(v1).min(v2).min(v3);
            max = max.max(v1).max(v2).max(v3);

            // Signed tetrahedron volume: V = v1 · (v2 × v3) / 6
            let cross = v2.cross(v3);
            volume += f64::from(v1.dot(cross)) / 6.0;

            let base = (face_idx * 3) as u32;
            for sv in &[sv1, sv2, sv3] {
                vertices.push(MeshVertex {
                    position: (*sv).into(),
                    normal,
                });
            }
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
        }

        Ok(Self {
            vertices,
            indices,
            bbox: BoundingBox { min, max },
            volume: volume.abs(),
        })
    }

    /// Loads mesh data from a byte slice containing STL data.
    pub fn from_stl_bytes(data: &[u8]) -> Result<Self> {
        let mut cursor = std::io::Cursor::new(data);
        Self::from_stl(&mut cursor)
    }
}

/// A positioned mesh within the print scene.
///
/// Wraps [`MeshData`] with transform properties (position, rotation, scale).
#[derive(Debug, Clone)]
pub struct PrintMesh {
    /// The underlying mesh geometry.
    pub mesh_data: MeshData,
    /// Position offset in world space (mm).
    pub position: Vec3,
    /// Rotation in radians (Euler angles, XYZ order).
    pub rotation: Vec3,
    /// Scale factors per axis.
    pub scale: Vec3,
}

impl PrintMesh {
    /// Creates a new `PrintMesh` from loaded mesh data at the origin.
    pub fn new(mesh_data: MeshData) -> Self {
        Self {
            mesh_data,
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }

    /// Returns the volume of the mesh accounting for scale factors.
    pub fn volume(&self) -> f64 {
        self.mesh_data.volume * f64::from(self.scale.x * self.scale.y * self.scale.z)
    }

    /// Returns the axis-aligned bounding box in world space (position + scale only).
    pub fn world_bbox(&self) -> BoundingBox {
        let scaled_min = self.mesh_data.bbox.min * self.scale + self.position;
        let scaled_max = self.mesh_data.bbox.max * self.scale + self.position;
        BoundingBox {
            min: scaled_min.min(scaled_max),
            max: scaled_max.max(scaled_min),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a binary STL of a unit cube (1x1x1) with min at origin.
    ///
    /// All faces use CCW winding when viewed from outside (outward normals).
    fn unit_cube_stl() -> Vec<u8> {
        let triangles: Vec<([f32; 3], [[f32; 3]; 3])> = vec![
            // Front face (z=1), normal +Z, CCW from outside
            (
                [0.0, 0.0, 1.0],
                [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0]],
            ),
            (
                [0.0, 0.0, 1.0],
                [[0.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
            ),
            // Back face (z=0), normal -Z, CCW from outside
            (
                [0.0, 0.0, -1.0],
                [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            ),
            (
                [0.0, 0.0, -1.0],
                [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            ),
            // Right face (x=1), normal +X, CCW from outside
            (
                [1.0, 0.0, 0.0],
                [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0]],
            ),
            (
                [1.0, 0.0, 0.0],
                [[1.0, 0.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]],
            ),
            // Left face (x=0), normal -X, CCW from outside
            (
                [-1.0, 0.0, 0.0],
                [[0.0, 1.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            ),
            (
                [-1.0, 0.0, 0.0],
                [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0]],
            ),
            // Top face (y=1), normal +Y, CCW from outside
            (
                [0.0, 1.0, 0.0],
                [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0]],
            ),
            (
                [0.0, 1.0, 0.0],
                [[0.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
            ),
            // Bottom face (y=0), normal -Y, CCW from outside
            (
                [0.0, -1.0, 0.0],
                [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0]],
            ),
            (
                [0.0, -1.0, 0.0],
                [[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
            ),
        ];

        let mut buf = Vec::new();
        // 80-byte header
        buf.extend_from_slice(&[0u8; 80]);
        // Triangle count (u32 LE)
        buf.extend_from_slice(&(triangles.len() as u32).to_le_bytes());

        for (normal, verts) in &triangles {
            // Normal
            for c in normal {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            // 3 vertices
            for v in verts {
                for c in v {
                    buf.extend_from_slice(&c.to_le_bytes());
                }
            }
            // Attribute byte count
            buf.extend_from_slice(&0u16.to_le_bytes());
        }

        buf
    }

    #[test]
    fn load_unit_cube_stl() {
        let stl_data = unit_cube_stl();
        let mesh = MeshData::from_stl_bytes(&stl_data).unwrap();
        assert_eq!(mesh.indices.len(), 36); // 12 triangles * 3
        assert_eq!(mesh.vertices.len(), 36); // 12 triangles * 3 vertices
    }

    #[test]
    fn unit_cube_bounding_box() {
        let stl_data = unit_cube_stl();
        let mesh = MeshData::from_stl_bytes(&stl_data).unwrap();

        let eps = 1e-6;
        assert!((mesh.bbox.min.x - 0.0).abs() < eps);
        assert!((mesh.bbox.min.y - 0.0).abs() < eps);
        assert!((mesh.bbox.min.z - 0.0).abs() < eps);
        assert!((mesh.bbox.max.x - 1.0).abs() < eps);
        assert!((mesh.bbox.max.y - 1.0).abs() < eps);
        assert!((mesh.bbox.max.z - 1.0).abs() < eps);

        let size = mesh.bbox.size();
        assert!((size.x - 1.0).abs() < eps);
        assert!((size.y - 1.0).abs() < eps);
        assert!((size.z - 1.0).abs() < eps);

        let center = mesh.bbox.center();
        assert!((center.x - 0.5).abs() < eps);
        assert!((center.y - 0.5).abs() < eps);
        assert!((center.z - 0.5).abs() < eps);
    }

    #[test]
    fn unit_cube_volume() {
        let stl_data = unit_cube_stl();
        let mesh = MeshData::from_stl_bytes(&stl_data).unwrap();
        // Volume of a 1x1x1 cube should be ~1.0
        assert!((mesh.volume - 1.0).abs() < 1e-4);
    }

    #[test]
    fn print_mesh_scaled_volume() {
        let stl_data = unit_cube_stl();
        let mesh_data = MeshData::from_stl_bytes(&stl_data).unwrap();
        let mut print_mesh = PrintMesh::new(mesh_data);
        print_mesh.scale = Vec3::new(2.0, 3.0, 4.0);
        // Volume should be 1.0 * 2 * 3 * 4 = 24.0
        assert!((print_mesh.volume() - 24.0).abs() < 1e-3);
    }

    #[test]
    fn print_mesh_world_bbox() {
        let stl_data = unit_cube_stl();
        let mesh_data = MeshData::from_stl_bytes(&stl_data).unwrap();
        let mut print_mesh = PrintMesh::new(mesh_data);
        print_mesh.position = Vec3::new(10.0, 20.0, 30.0);
        print_mesh.scale = Vec3::new(2.0, 2.0, 2.0);

        let bbox = print_mesh.world_bbox();
        let eps = 1e-6;
        assert!((bbox.min.x - 10.0).abs() < eps);
        assert!((bbox.min.y - 20.0).abs() < eps);
        assert!((bbox.min.z - 30.0).abs() < eps);
        assert!((bbox.max.x - 12.0).abs() < eps);
        assert!((bbox.max.y - 22.0).abs() < eps);
        assert!((bbox.max.z - 32.0).abs() < eps);
    }

    #[test]
    fn stl_io_preserves_winding_and_normals() {
        // Single triangle: normal +Z, vertices CCW from +Z
        let triangles: Vec<([f32; 3], [[f32; 3]; 3])> = vec![(
            [0.0, 0.0, 1.0],
            [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0]],
        )];

        let mut buf = Vec::new();
        buf.extend_from_slice(&[0u8; 80]);
        buf.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for (normal, verts) in &triangles {
            for c in normal {
                buf.extend_from_slice(&c.to_le_bytes());
            }
            for v in verts {
                for c in v {
                    buf.extend_from_slice(&c.to_le_bytes());
                }
            }
            buf.extend_from_slice(&0u16.to_le_bytes());
        }

        let mesh = MeshData::from_stl_bytes(&buf).unwrap();

        // Check the loaded normal for the first vertex
        let n = Vec3::from(mesh.vertices[0].normal);
        eprintln!("Loaded normal: {n:?}");

        // Normal should point +Z (outward)
        assert!(n.z > 0.9, "Normal should point +Z, got {n:?}");

        // Check winding: cross product of first triangle edges should agree with normal
        let p0 = Vec3::from(mesh.vertices[0].position);
        let p1 = Vec3::from(mesh.vertices[1].position);
        let p2 = Vec3::from(mesh.vertices[2].position);
        eprintln!("Positions: p0={p0:?}, p1={p1:?}, p2={p2:?}");

        let geo_normal = (p1 - p0).cross(p2 - p0);
        eprintln!("Geometric normal from winding: {geo_normal:?}");
        eprintln!("Dot with stored normal: {}", geo_normal.dot(n));

        assert!(
            geo_normal.dot(n) > 0.0,
            "Winding-derived normal should agree with stored normal. \
             Geometric={geo_normal:?}, Stored={n:?}"
        );
    }

    #[test]
    fn all_cube_faces_have_correct_winding() {
        let stl_data = unit_cube_stl();
        let mesh = MeshData::from_stl_bytes(&stl_data).unwrap();

        let face_count = mesh.indices.len() / 3;
        for face in 0..face_count {
            let i0 = mesh.indices[face * 3] as usize;
            let i1 = mesh.indices[face * 3 + 1] as usize;
            let i2 = mesh.indices[face * 3 + 2] as usize;

            let p0 = Vec3::from(mesh.vertices[i0].position);
            let p1 = Vec3::from(mesh.vertices[i1].position);
            let p2 = Vec3::from(mesh.vertices[i2].position);
            let n = Vec3::from(mesh.vertices[i0].normal);

            let geo = (p1 - p0).cross(p2 - p0);
            let dot = geo.dot(n);

            eprintln!("Face {face}: n={n:?}, geo={geo:?}, dot={dot:.4}");
            assert!(
                dot > 0.0,
                "Face {face} winding disagrees with normal: geo={geo:?}, normal={n:?}"
            );
        }
    }
}
