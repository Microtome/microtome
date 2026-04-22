//! Surface reprojection target abstraction.
//!
//! After smoothing or simplification, repaired vertices drift off the
//! "true" isosurface. A [`ReprojectionTarget`] is the abstraction repair
//! passes use to snap vertices back. Two implementations ship in v2:
//!
//! - [`ScalarFieldTarget`] — Newton iteration on a [`ScalarField`].
//! - `MeshTarget` — closest point on a reference triangle mesh, lands
//!   alongside the BVH closest-point query (task #31).

use glam::Vec3;

use crate::isosurface::{IsoMesh, ScalarField};

/// Result of projecting a point onto a target surface.
#[derive(Debug, Clone, Copy)]
pub struct ProjectionResult {
    /// The projected world-space position.
    pub position: Vec3,
    /// Surface normal at the projected point.
    pub normal: Vec3,
    /// Distance from the input point to `position`.
    pub distance: f32,
}

/// Abstraction over "the surface a vertex should lie on" for repair passes
/// that move geometry.
///
/// Implementations either use an analytic / sampled scalar field (the
/// common DC case) or a reference triangle mesh + BVH (the
/// scan-conversion case). Both must respect the contract:
///
/// - `project(p, hint_normal)` returns the projection or `None` if the
///   target can't reach `p` (too far, degenerate gradient, etc.).
/// - `signed_distance(p)` is optional but useful for passes that want to
///   measure drift before committing to a projection.
/// - `normal(p)` returns the surface normal near `p`.
pub trait ReprojectionTarget: Send + Sync {
    /// Projects `p` onto the target surface. `hint_normal` is the caller's
    /// best guess (e.g. the moved vertex's prior normal), which can speed
    /// up search; ignore if your implementation has no use for it.
    fn project(&self, p: Vec3, hint_normal: Option<Vec3>) -> Option<ProjectionResult>;

    /// Optional signed distance from `p` to the surface. Default `None` —
    /// implementations that can compute it cheaply override.
    fn signed_distance(&self, p: Vec3) -> Option<f32> {
        let _ = p;
        None
    }

    /// Surface normal near `p`.
    fn normal(&self, p: Vec3) -> Vec3;
}

/// Newton-iteration projection onto a [`ScalarField`]'s zero level set.
///
/// At each iteration: `p ← p − f(p) · ∇f(p) / |∇f(p)|²`. Converges when
/// `|f(p)| < tolerance` or after `max_iters`. Bails to `None` if the
/// gradient vanishes (the field is locally flat at `p`).
///
/// Defaults: tolerance `1e-4`, `max_iters = 8`.
///
/// The wrapped `ScalarField` must be `Send + Sync` so the target satisfies
/// the [`ReprojectionTarget`] bounds. Concrete primitives (`Sphere`,
/// `Aabb`, `Cylinder`, etc.) all qualify.
pub struct ScalarFieldTarget<'a> {
    field: &'a (dyn ScalarField + Send + Sync),
    tolerance: f32,
    max_iters: u32,
}

impl<'a> ScalarFieldTarget<'a> {
    /// Wraps a scalar field with default tolerance / iteration count.
    pub fn new(field: &'a (dyn ScalarField + Send + Sync)) -> Self {
        Self {
            field,
            tolerance: 1e-4,
            max_iters: 8,
        }
    }

    /// Sets the convergence tolerance.
    pub fn with_tolerance(mut self, tol: f32) -> Self {
        self.tolerance = tol;
        self
    }

    /// Sets the maximum Newton iteration count.
    pub fn with_max_iters(mut self, n: u32) -> Self {
        self.max_iters = n;
        self
    }
}

impl ReprojectionTarget for ScalarFieldTarget<'_> {
    fn project(&self, p: Vec3, _hint_normal: Option<Vec3>) -> Option<ProjectionResult> {
        let mut current = p;
        for _ in 0..self.max_iters {
            let value = self.field.value(current);
            if value.abs() < self.tolerance {
                let normal = self.field.normal(current);
                return Some(ProjectionResult {
                    position: current,
                    normal,
                    distance: (current - p).length(),
                });
            }
            let grad = self.field.gradient(current);
            let grad_sq = grad.length_squared();
            if grad_sq < 1e-12 {
                // Vanishing gradient; can't continue.
                return None;
            }
            current -= grad * (value / grad_sq);
        }
        // Final value check: accept the result if we got close enough.
        let final_value = self.field.value(current);
        if final_value.abs() < self.tolerance * 10.0 {
            let normal = self.field.normal(current);
            Some(ProjectionResult {
                position: current,
                normal,
                distance: (current - p).length(),
            })
        } else {
            None
        }
    }

    fn signed_distance(&self, p: Vec3) -> Option<f32> {
        Some(self.field.value(p))
    }

    fn normal(&self, p: Vec3) -> Vec3 {
        self.field.normal(p)
    }
}

/// Reprojection onto a reference triangle mesh via brute-force closest
/// point search.
///
/// v2 first cut is O(n) per query (no BVH). For repair-time use cases
/// this is acceptable up to a few thousand reference triangles. A
/// BVH-accelerated variant lands with v2.5; the closest-point query
/// would need a closest-point BVH separate from
/// [`mesh_bvh`](crate::isosurface), which is specialised for winding
/// queries.
pub struct MeshTarget<'a> {
    triangles: Vec<[Vec3; 3]>,
    _mesh: std::marker::PhantomData<&'a IsoMesh>,
}

impl<'a> MeshTarget<'a> {
    /// Wraps a reference [`IsoMesh`]; precomputes triangle vertex tuples.
    pub fn new(mesh: &'a IsoMesh) -> Self {
        let mut triangles = Vec::with_capacity(mesh.indices.len() / 3);
        for tri in mesh.indices.chunks_exact(3) {
            triangles.push([
                mesh.positions[tri[0] as usize],
                mesh.positions[tri[1] as usize],
                mesh.positions[tri[2] as usize],
            ]);
        }
        Self {
            triangles,
            _mesh: std::marker::PhantomData,
        }
    }
}

impl ReprojectionTarget for MeshTarget<'_> {
    fn project(&self, p: Vec3, _hint_normal: Option<Vec3>) -> Option<ProjectionResult> {
        if self.triangles.is_empty() {
            return None;
        }
        let mut best: Option<(Vec3, f32, [Vec3; 3])> = None;
        for tri in &self.triangles {
            let q = closest_point_on_triangle(p, tri);
            let d = (q - p).length();
            if best.is_none_or(|(_, bd, _)| d < bd) {
                best = Some((q, d, *tri));
            }
        }
        let (position, distance, tri) = best?;
        let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        Some(ProjectionResult {
            position,
            normal: n,
            distance,
        })
    }

    fn normal(&self, p: Vec3) -> Vec3 {
        self.project(p, None).map(|r| r.normal).unwrap_or(Vec3::Z)
    }
}

/// Closest point on a triangle (Ericson, Real-Time Collision Detection §5.1.5).
fn closest_point_on_triangle(p: Vec3, tri: &[Vec3; 3]) -> Vec3 {
    let a = tri[0];
    let b = tri[1];
    let c = tri[2];
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + v * ab;
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + w * ac;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + w * (c - b);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::Sphere;

    #[test]
    fn scalar_field_target_newton_converges_on_sphere() {
        let sphere = Sphere::with_center(3.0, Vec3::ZERO);
        let target = ScalarFieldTarget::new(&sphere);
        // A point well outside the sphere should project to the surface.
        let p = Vec3::new(5.0, 0.0, 0.0);
        let result = target.project(p, None).expect("converges");
        assert!(
            (result.position.length() - 3.0).abs() < 1e-3,
            "projected position {:?} not on sphere of radius 3",
            result.position
        );
        // Normal should point outward (along +x).
        assert!(result.normal.x > 0.5);
        // Distance roughly 5 - 3 = 2.
        assert!((result.distance - 2.0).abs() < 0.1);
    }

    #[test]
    fn scalar_field_target_returns_zero_distance_on_surface_point() {
        let sphere = Sphere::with_center(3.0, Vec3::ZERO);
        let target = ScalarFieldTarget::new(&sphere);
        let on_surface = Vec3::new(3.0, 0.0, 0.0);
        let result = target.project(on_surface, None).expect("converges");
        assert!(result.distance < 1e-3);
    }

    #[test]
    fn scalar_field_target_bails_on_flat_field() {
        // A "field" that returns 1.0 everywhere has zero gradient and a
        // non-zero value — Newton can't make progress.
        struct ConstField;
        impl ScalarField for ConstField {
            fn value(&self, _p: Vec3) -> f32 {
                1.0
            }
        }
        let f = ConstField;
        let target = ScalarFieldTarget::new(&f);
        let result = target.project(Vec3::ZERO, None);
        assert!(result.is_none(), "flat field should bail");
    }

    #[test]
    fn mesh_target_returns_nearest_point_on_triangle() {
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        };
        let target = MeshTarget::new(&iso);
        // Point above the triangle: closest is the perpendicular foot.
        let p = Vec3::new(0.25, 0.25, 5.0);
        let result = target.project(p, None).expect("projects");
        assert!((result.position.z).abs() < 1e-4);
        assert!((result.distance - 5.0).abs() < 1e-4);
    }

    #[test]
    fn mesh_target_returns_corner_for_far_outside_point() {
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        };
        let target = MeshTarget::new(&iso);
        // Point far in the -x, -y quadrant: closest is the (0,0,0) corner.
        let p = Vec3::new(-5.0, -5.0, 0.0);
        let result = target.project(p, None).expect("projects");
        assert!((result.position - Vec3::ZERO).length() < 1e-4);
    }

    #[test]
    fn mesh_target_empty_returns_none() {
        let iso = IsoMesh::new();
        let target = MeshTarget::new(&iso);
        assert!(target.project(Vec3::ONE, None).is_none());
    }

    #[test]
    fn scalar_field_target_signed_distance_is_field_value() {
        let sphere = Sphere::with_center(2.0, Vec3::ZERO);
        let target = ScalarFieldTarget::new(&sphere);
        let outside = Vec3::new(5.0, 0.0, 0.0);
        let sd = target.signed_distance(outside).expect("has sd");
        // Sphere SDF: |p| - r = 5 - 2 = 3.
        assert!((sd - 3.0).abs() < 1e-3);
    }
}
