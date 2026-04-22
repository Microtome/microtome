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

use crate::isosurface::ScalarField;

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
    fn scalar_field_target_signed_distance_is_field_value() {
        let sphere = Sphere::with_center(2.0, Vec3::ZERO);
        let target = ScalarFieldTarget::new(&sphere);
        let outside = Vec3::new(5.0, 0.0, 0.0);
        let sd = target.signed_distance(outside).expect("has sd");
        // Sphere SDF: |p| - r = 5 - 2 = 3.
        assert!((sd - 3.0).abs() < 1e-3);
    }
}
