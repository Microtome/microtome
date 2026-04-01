//! CPU ray casting for mesh picking in the 3D viewport.

use glam::{Mat4, Vec2, Vec3};
use microtome_core::PrintMesh;

/// Casts a ray from screen coordinates through the scene and returns the index
/// of the closest hit mesh, if any.
///
/// `screen_pos` is in egui logical coordinates within the viewport rect.
/// `rect` is the viewport rect in logical coordinates.
pub fn pick_mesh(
    screen_pos: Vec2,
    rect_min: Vec2,
    rect_size: Vec2,
    view: Mat4,
    proj: Mat4,
    meshes: &[PrintMesh],
    model_matrices: &[Mat4],
) -> Option<usize> {
    let ray = screen_to_ray(screen_pos, rect_min, rect_size, view, proj);

    let mut closest_t = f32::MAX;
    let mut closest_idx = None;

    for (i, (mesh, model)) in meshes.iter().zip(model_matrices.iter()).enumerate() {
        if let Some(t) = ray_mesh_intersection(&ray, mesh, model)
            && t < closest_t
        {
            closest_t = t;
            closest_idx = Some(i);
        }
    }

    closest_idx
}

/// A ray defined by an origin and direction.
struct Ray {
    origin: Vec3,
    direction: Vec3,
}

/// Converts screen coordinates to a world-space ray.
fn screen_to_ray(screen_pos: Vec2, rect_min: Vec2, rect_size: Vec2, view: Mat4, proj: Mat4) -> Ray {
    // Normalize to [-1, 1] clip space
    let ndc_x = 2.0 * (screen_pos.x - rect_min.x) / rect_size.x - 1.0;
    let ndc_y = 1.0 - 2.0 * (screen_pos.y - rect_min.y) / rect_size.y;

    let inv_vp = (proj * view).inverse();

    let near = inv_vp.project_point3(Vec3::new(ndc_x, ndc_y, 0.0));
    let far = inv_vp.project_point3(Vec3::new(ndc_x, ndc_y, 1.0));

    Ray {
        origin: near,
        direction: (far - near).normalize(),
    }
}

/// Tests a ray against all triangles in a mesh (in world space).
/// Returns the closest hit distance, or None.
fn ray_mesh_intersection(ray: &Ray, mesh: &PrintMesh, model: &Mat4) -> Option<f32> {
    let verts = &mesh.mesh_data.vertices;
    let indices = &mesh.mesh_data.indices;

    let mut closest_t = f32::MAX;
    let mut hit = false;

    let tri_count = indices.len() / 3;
    for tri in 0..tri_count {
        let i0 = indices[tri * 3] as usize;
        let i1 = indices[tri * 3 + 1] as usize;
        let i2 = indices[tri * 3 + 2] as usize;

        let v0 = model.transform_point3(Vec3::from(verts[i0].position));
        let v1 = model.transform_point3(Vec3::from(verts[i1].position));
        let v2 = model.transform_point3(Vec3::from(verts[i2].position));

        if let Some(t) = ray_triangle(ray, v0, v1, v2)
            && t > 0.0
            && t < closest_t
        {
            closest_t = t;
            hit = true;
        }
    }

    if hit { Some(closest_t) } else { None }
}

/// Möller–Trumbore ray-triangle intersection.
/// Returns the distance along the ray, or None if no intersection.
fn ray_triangle(ray: &Ray, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = ray.direction.cross(edge2);
    let a = edge1.dot(h);

    if a.abs() < 1e-7 {
        return None; // Ray parallel to triangle
    }

    let f = 1.0 / a;
    let s = ray.origin - v0;
    let u = f * s.dot(h);

    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = s.cross(edge1);
    let v = f * ray.direction.dot(q);

    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * edge2.dot(q);
    Some(t)
}
