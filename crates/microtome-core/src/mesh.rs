//! Mesh loading (STL, OBJ), vertex data, and volume calculation.

use std::path::Path;

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

    /// Loads mesh data from a Wavefront OBJ file on disk.
    ///
    /// Combines all groups and objects in the file into a single triangle mesh.
    /// When the file provides vertex normals they are used as-is; otherwise
    /// area-weighted smooth normals are computed from face geometry.
    ///
    /// Material libraries (`.mtl`) are ignored — only geometry is loaded.
    pub fn from_obj(path: &Path) -> Result<Self> {
        let (models, _materials) = tobj::load_obj(
            path,
            &tobj::LoadOptions {
                single_index: true,
                triangulate: true,
                ignore_points: true,
                ignore_lines: true,
            },
        )
        .map_err(|e| MicrotomeError::ObjParse(e.to_string()))?;

        let mut vertices: Vec<MeshVertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut min = Vec3::splat(f32::MAX);
        let mut max = Vec3::splat(f32::MIN);
        let mut volume = 0.0_f64;

        for model in &models {
            let mesh = &model.mesh;
            let vertex_count = mesh.positions.len() / 3;
            if vertex_count == 0 || mesh.indices.is_empty() {
                continue;
            }

            let base = vertices.len() as u32;
            let has_normals =
                !mesh.normals.is_empty() && mesh.normals.len() == mesh.positions.len();

            for i in 0..vertex_count {
                let position = [
                    mesh.positions[i * 3],
                    mesh.positions[i * 3 + 1],
                    mesh.positions[i * 3 + 2],
                ];
                let p = Vec3::from(position);
                min = min.min(p);
                max = max.max(p);

                let normal = if has_normals {
                    [
                        mesh.normals[i * 3],
                        mesh.normals[i * 3 + 1],
                        mesh.normals[i * 3 + 2],
                    ]
                } else {
                    [0.0, 0.0, 0.0]
                };
                vertices.push(MeshVertex { position, normal });
            }

            let mut accum_normals = if has_normals {
                Vec::new()
            } else {
                vec![Vec3::ZERO; vertex_count]
            };

            let tri_count = mesh.indices.len() / 3;
            for tri in 0..tri_count {
                let li0 = mesh.indices[tri * 3] as usize;
                let li1 = mesh.indices[tri * 3 + 1] as usize;
                let li2 = mesh.indices[tri * 3 + 2] as usize;

                let v0 = Vec3::from(vertices[base as usize + li0].position);
                let v1 = Vec3::from(vertices[base as usize + li1].position);
                let v2 = Vec3::from(vertices[base as usize + li2].position);

                let cross = v1.cross(v2);
                volume += f64::from(v0.dot(cross)) / 6.0;

                if !has_normals {
                    let face_n = (v1 - v0).cross(v2 - v0);
                    accum_normals[li0] += face_n;
                    accum_normals[li1] += face_n;
                    accum_normals[li2] += face_n;
                }

                indices.push(base + li0 as u32);
                indices.push(base + li1 as u32);
                indices.push(base + li2 as u32);
            }

            if !has_normals {
                for (i, n) in accum_normals.iter().enumerate() {
                    let unit = if n.length_squared() > 0.0 {
                        n.normalize()
                    } else {
                        Vec3::Z
                    };
                    vertices[base as usize + i].normal = unit.into();
                }
            }
        }

        if vertices.is_empty() {
            min = Vec3::ZERO;
            max = Vec3::ZERO;
        }

        Ok(Self {
            vertices,
            indices,
            bbox: BoundingBox { min, max },
            volume: volume.abs(),
        })
    }
}

/// A positioned mesh within the print scene.
///
/// Wraps [`MeshData`] with transform properties (position, rotation, scale).
#[derive(Debug, Clone)]
pub struct PrintMesh {
    /// Display name (typically the source filename).
    pub name: String,
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
    pub fn new(name: impl Into<String>, mesh_data: MeshData) -> Self {
        Self {
            name: name.into(),
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
        let mut print_mesh = PrintMesh::new("test", mesh_data);
        print_mesh.scale = Vec3::new(2.0, 3.0, 4.0);
        // Volume should be 1.0 * 2 * 3 * 4 = 24.0
        assert!((print_mesh.volume() - 24.0).abs() < 1e-3);
    }

    #[test]
    fn print_mesh_world_bbox() {
        let stl_data = unit_cube_stl();
        let mesh_data = MeshData::from_stl_bytes(&stl_data).unwrap();
        let mut print_mesh = PrintMesh::new("test", mesh_data);
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

    /// Writes an OBJ text file to a unique temp path and returns it.
    fn write_temp_obj(contents: &str, tag: &str) -> std::path::PathBuf {
        use std::io::Write;
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("microtome_obj_test_{pid}_{tag}_{n}.obj"));
        let mut file = std::fs::File::create(&path).expect("create temp obj");
        file.write_all(contents.as_bytes()).expect("write obj");
        path
    }

    /// OBJ source for a unit cube at the origin with explicit vertex normals.
    ///
    /// OBJ indices are 1-based. The cube spans [0,1]³ and each face is
    /// triangulated with CCW winding from outside.
    const UNIT_CUBE_OBJ: &str = "\
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
v 1 0 1
v 1 1 1
v 0 1 1
vn 0 0 -1
vn 0 0 1
vn -1 0 0
vn 1 0 0
vn 0 -1 0
vn 0 1 0
f 4//1 3//1 2//1
f 4//1 2//1 1//1
f 5//2 6//2 7//2
f 5//2 7//2 8//2
f 1//3 5//3 8//3
f 1//3 8//3 4//3
f 2//4 3//4 7//4
f 2//4 7//4 6//4
f 1//5 2//5 6//5
f 1//5 6//5 5//5
f 4//6 8//6 7//6
f 4//6 7//6 3//6
";

    #[test]
    fn load_unit_cube_obj() {
        let path = write_temp_obj(UNIT_CUBE_OBJ, "unit_cube");
        let mesh = MeshData::from_obj(&path).expect("load obj");
        let _ = std::fs::remove_file(&path);

        assert_eq!(mesh.indices.len(), 36);
        assert!(!mesh.vertices.is_empty());

        let eps = 1e-5;
        assert!((mesh.bbox.min.x - 0.0).abs() < eps);
        assert!((mesh.bbox.min.y - 0.0).abs() < eps);
        assert!((mesh.bbox.min.z - 0.0).abs() < eps);
        assert!((mesh.bbox.max.x - 1.0).abs() < eps);
        assert!((mesh.bbox.max.y - 1.0).abs() < eps);
        assert!((mesh.bbox.max.z - 1.0).abs() < eps);

        assert!((mesh.volume - 1.0).abs() < 1e-4);
    }

    #[test]
    fn obj_without_normals_generates_smooth_normals() {
        // Same cube, no vn directives.
        let obj_src = "\
v 0 0 0
v 1 0 0
v 1 1 0
v 0 1 0
v 0 0 1
v 1 0 1
v 1 1 1
v 0 1 1
f 4 3 2
f 4 2 1
f 5 6 7
f 5 7 8
f 1 5 8
f 1 8 4
f 2 3 7
f 2 7 6
f 1 2 6
f 1 6 5
f 4 8 7
f 4 7 3
";
        let path = write_temp_obj(obj_src, "no_normals");
        let mesh = MeshData::from_obj(&path).expect("load obj");
        let _ = std::fs::remove_file(&path);

        assert_eq!(mesh.indices.len(), 36);

        // All vertex normals should be unit length.
        for v in &mesh.vertices {
            let n = Vec3::from(v.normal);
            assert!((n.length() - 1.0).abs() < 1e-4, "non-unit normal: {n:?}");
        }
    }

    #[test]
    fn obj_volume_matches_scale() {
        // A cube scaled to 2x2x2 via explicit vertex positions.
        let obj_src = "\
v 0 0 0
v 2 0 0
v 2 2 0
v 0 2 0
v 0 0 2
v 2 0 2
v 2 2 2
v 0 2 2
f 4 3 2
f 4 2 1
f 5 6 7
f 5 7 8
f 1 5 8
f 1 8 4
f 2 3 7
f 2 7 6
f 1 2 6
f 1 6 5
f 4 8 7
f 4 7 3
";
        let path = write_temp_obj(obj_src, "scale_cube");
        let mesh = MeshData::from_obj(&path).expect("load obj");
        let _ = std::fs::remove_file(&path);

        // 2×2×2 cube → volume 8
        assert!((mesh.volume - 8.0).abs() < 1e-3);
    }

    #[test]
    fn obj_missing_file_errors() {
        let bogus = std::path::PathBuf::from("/nonexistent/absolutely/missing.obj");
        let res = MeshData::from_obj(&bogus);
        assert!(matches!(res, Err(MicrotomeError::ObjParse(_))));
    }
}
