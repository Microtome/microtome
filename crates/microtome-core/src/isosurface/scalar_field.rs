//! Scalar field trait and signed distance function (SDF) primitives.
//!
//! The [`ScalarField`] trait defines the interface for implicit functions
//! sampled during isosurface extraction. Concrete SDF primitives and CSG
//! combinators implement this trait.

use glam::{Mat4, Vec2, Vec3};

use super::indicators::{PositionCode, code_to_pos};

/// An implicit scalar field that can be sampled at any point in space.
///
/// Negative values are inside the surface, positive outside (SDF convention).
pub trait ScalarField {
    /// Evaluates the field at a world-space point.
    fn value(&self, p: Vec3) -> f32;

    /// Evaluates the field at a voxel grid position.
    ///
    /// Default implementation converts the code to world space using `unit_size`.
    fn index(&self, code: PositionCode, unit_size: f32) -> f32 {
        self.value(code_to_pos(code, unit_size))
    }

    /// Finds the zero-crossing between two points by binary search.
    ///
    /// Returns `Some(point)` if a sign change exists between `p1` and `p2`.
    fn solve(&self, p1: Vec3, p2: Vec3) -> Option<Vec3> {
        let offset = p2 - p1;
        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        let mut mid = (lo + hi) / 2.0;

        for _ in 0..16 {
            let l = self.value(p1 + offset * lo);
            mid = (lo + hi) / 2.0;
            let m = self.value(p1 + offset * mid);
            if (l >= 0.0 && m < 0.0) || (l < 0.0 && m >= 0.0) {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Some(p1 + offset * mid)
    }

    /// Step size for numerical gradient computation.
    fn gradient_offset(&self) -> f32 {
        0.01
    }

    /// Computes the surface normal at a point via central differences.
    fn normal(&self, p: Vec3) -> Vec3 {
        let h = self.gradient_offset();
        let nx = self.value(p + Vec3::new(h, 0.0, 0.0)) - self.value(p - Vec3::new(h, 0.0, 0.0));
        let ny = self.value(p + Vec3::new(0.0, h, 0.0)) - self.value(p - Vec3::new(0.0, h, 0.0));
        let nz = self.value(p + Vec3::new(0.0, 0.0, h)) - self.value(p - Vec3::new(0.0, 0.0, h));

        let n = Vec3::new(nx, ny, nz);
        if n == Vec3::ZERO {
            return Vec3::ONE.normalize();
        }
        n.normalize()
    }

    /// Computes the gradient (non-normalized) at a point via central differences.
    fn gradient(&self, p: Vec3) -> Vec3 {
        let h = self.gradient_offset();
        let nx = self.value(p + Vec3::new(h, 0.0, 0.0)) - self.value(p - Vec3::new(h, 0.0, 0.0));
        let ny = self.value(p + Vec3::new(0.0, h, 0.0)) - self.value(p - Vec3::new(0.0, h, 0.0));
        let nz = self.value(p + Vec3::new(0.0, 0.0, h)) - self.value(p - Vec3::new(0.0, 0.0, h));
        Vec3::new(nx, ny, nz) / h / 2.0
    }
}

// ---------------------------------------------------------------------------
// SDF Primitives
// ---------------------------------------------------------------------------

/// Signed distance field for a sphere.
pub struct Sphere {
    /// Sphere radius.
    pub radius: f32,
    /// Sphere center.
    pub center: Vec3,
}

impl Sphere {
    /// Creates a sphere at the origin with the given radius.
    pub fn new(radius: f32) -> Self {
        Self {
            radius,
            center: Vec3::ZERO,
        }
    }

    /// Creates a sphere at the given center with the given radius.
    pub fn with_center(radius: f32, center: Vec3) -> Self {
        Self { radius, center }
    }
}

impl ScalarField for Sphere {
    fn value(&self, p: Vec3) -> f32 {
        (p - self.center).length() - self.radius
    }
}

/// Signed distance field for an axis-aligned bounding box.
pub struct Aabb {
    /// Minimum corner.
    pub min: Vec3,
    /// Maximum corner.
    pub max: Vec3,
}

impl Aabb {
    /// Creates an AABB from min and max corners.
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }
}

impl ScalarField for Aabb {
    fn value(&self, p: Vec3) -> f32 {
        let center = (self.min + self.max) / 2.0;
        let half = (self.max - self.min) / 2.0;
        let offset = (p - center).abs() - half;
        offset.length().min(offset.x.max(offset.y.max(offset.z)))
    }
}

/// Signed distance field for a torus centered at the origin in the XZ plane.
pub struct Torus {
    /// Major radius (distance from center to tube center).
    pub r1: f32,
    /// Minor radius (tube radius).
    pub r2: f32,
}

impl Torus {
    /// Creates a torus with the given radii.
    pub fn new(r1: f32, r2: f32) -> Self {
        Self { r1, r2 }
    }
}

impl ScalarField for Torus {
    fn value(&self, p: Vec3) -> f32 {
        let q = Vec2::new(Vec2::new(p.x, p.z).length() - self.r1, p.y);
        q.length() - self.r2
    }
}

/// Signed distance field for an infinite cylinder along the Y axis.
pub struct Cylinder {
    /// Center XY position and radius: `(cx, cy, radius)`.
    pub center: Vec3,
}

impl Cylinder {
    /// Creates a cylinder with the given center (x, y) and radius (z component).
    pub fn new(center: Vec3) -> Self {
        Self { center }
    }
}

impl ScalarField for Cylinder {
    fn value(&self, p: Vec3) -> f32 {
        Vec2::new(p.x - self.center.x, p.z - self.center.y).length() - self.center.z
    }
}

/// Signed distance field for a capsule (rounded line segment).
pub struct Capsule {
    /// Start point of the line segment.
    pub a: Vec3,
    /// End point of the line segment.
    pub b: Vec3,
    /// Capsule radius.
    pub r: f32,
}

impl Capsule {
    /// Creates a capsule between two points with the given radius.
    pub fn new(a: Vec3, b: Vec3, r: f32) -> Self {
        Self { a, b, r }
    }
}

impl ScalarField for Capsule {
    fn value(&self, p: Vec3) -> f32 {
        let pa = p - self.a;
        let ba = self.b - self.a;
        let h = pa.dot(ba) / ba.dot(ba);
        let h = h.clamp(0.0, 1.0);
        (pa - ba * h).length() - self.r
    }
}

/// Signed distance field for a heart shape.
pub struct Heart {
    /// Scale factor.
    pub scale: f32,
    /// Center position.
    pub center: Vec3,
}

impl Heart {
    /// Creates a heart shape with the given scale at the origin.
    pub fn new(scale: f32) -> Self {
        Self {
            scale,
            center: Vec3::ZERO,
        }
    }

    /// Creates a heart shape with the given scale and center.
    pub fn with_center(scale: f32, center: Vec3) -> Self {
        Self { scale, center }
    }
}

impl ScalarField for Heart {
    fn value(&self, p: Vec3) -> f32 {
        let offset = (p - self.center) / self.scale;
        let x = offset.x;
        let y = offset.z;
        let z = offset.y;
        let a = x * x + 9.0 / 4.0 * y * y + z * z - 1.0;
        a * a * a - x * x * z * z * z - 9.0 / 80.0 * y * y * z * z * z
    }
}

// ---------------------------------------------------------------------------
// CSG Combinators
// ---------------------------------------------------------------------------

/// CSG union: the minimum of two fields.
pub struct Union<A, B> {
    /// Left operand.
    pub left: A,
    /// Right operand.
    pub right: B,
}

impl<A: ScalarField, B: ScalarField> Union<A, B> {
    /// Creates a union of two scalar fields.
    pub fn new(left: A, right: B) -> Self {
        Self { left, right }
    }
}

impl<A: ScalarField, B: ScalarField> ScalarField for Union<A, B> {
    fn value(&self, p: Vec3) -> f32 {
        self.left.value(p).min(self.right.value(p))
    }
}

/// CSG intersection: the maximum of two fields.
pub struct Intersection<A, B> {
    /// Left operand.
    pub left: A,
    /// Right operand.
    pub right: B,
}

impl<A: ScalarField, B: ScalarField> Intersection<A, B> {
    /// Creates an intersection of two scalar fields.
    pub fn new(left: A, right: B) -> Self {
        Self { left, right }
    }
}

impl<A: ScalarField, B: ScalarField> ScalarField for Intersection<A, B> {
    fn value(&self, p: Vec3) -> f32 {
        self.left.value(p).max(self.right.value(p))
    }
}

/// CSG difference: left minus right.
pub struct Difference<A, B> {
    /// Left operand (kept).
    pub left: A,
    /// Right operand (subtracted).
    pub right: B,
}

impl<A: ScalarField, B: ScalarField> Difference<A, B> {
    /// Creates a difference (left - right).
    pub fn new(left: A, right: B) -> Self {
        Self { left, right }
    }
}

impl<A: ScalarField, B: ScalarField> ScalarField for Difference<A, B> {
    fn value(&self, p: Vec3) -> f32 {
        self.left.value(p).max(-self.right.value(p))
    }
}

/// Smooth union using exponential blending.
pub struct SmoothUnion<A, B> {
    /// Left operand.
    pub left: A,
    /// Right operand.
    pub right: B,
    /// Blending sharpness (higher = sharper, default 32).
    pub k: f32,
}

impl<A: ScalarField, B: ScalarField> SmoothUnion<A, B> {
    /// Creates a smooth union with the given sharpness parameter.
    pub fn new(left: A, right: B, k: f32) -> Self {
        Self { left, right, k }
    }
}

impl<A: ScalarField, B: ScalarField> ScalarField for SmoothUnion<A, B> {
    fn value(&self, p: Vec3) -> f32 {
        let res = (-self.k * self.left.value(p)).exp() + (-self.k * self.right.value(p)).exp();
        -res.max(0.0001).ln() / self.k
    }
}

/// Applies an affine transformation to a scalar field.
pub struct TransformedField<T> {
    /// The transformation matrix (applied as inverse to sample points).
    pub transform: Mat4,
    /// The inner scalar field.
    pub inner: T,
}

impl<T: ScalarField> TransformedField<T> {
    /// Creates a transformed field. Points are multiplied by the matrix before sampling.
    pub fn new(transform: Mat4, inner: T) -> Self {
        Self { transform, inner }
    }
}

impl<T: ScalarField> ScalarField for TransformedField<T> {
    fn value(&self, p: Vec3) -> f32 {
        let t = self.transform.transform_point3(p);
        self.inner.value(t)
    }
}

/// Union of a dynamic list of boxed scalar fields.
pub struct UnionList {
    /// The list of fields.
    pub fields: Vec<Box<dyn ScalarField>>,
}

impl UnionList {
    /// Creates a union of the given fields.
    pub fn new(fields: Vec<Box<dyn ScalarField>>) -> Self {
        Self { fields }
    }
}

impl ScalarField for UnionList {
    fn value(&self, p: Vec3) -> f32 {
        self.fields
            .iter()
            .map(|f| f.value(p))
            .fold(f32::MAX, f32::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sphere_sdf_at_surface() {
        let s = Sphere::new(1.0);
        assert!((s.value(Vec3::new(1.0, 0.0, 0.0))).abs() < 1e-6);
        assert!(s.value(Vec3::ZERO) < 0.0); // inside
        assert!(s.value(Vec3::new(2.0, 0.0, 0.0)) > 0.0); // outside
    }

    #[test]
    fn aabb_sdf_inside_outside() {
        let b = Aabb::new(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(b.value(Vec3::ZERO) < 0.0); // inside
        assert!(b.value(Vec3::new(2.0, 0.0, 0.0)) > 0.0); // outside
    }

    #[test]
    fn union_takes_minimum() {
        let a = Sphere::with_center(1.0, Vec3::new(-1.0, 0.0, 0.0));
        let b = Sphere::with_center(1.0, Vec3::new(1.0, 0.0, 0.0));
        let u = Union::new(a, b);
        // At origin, both spheres are at distance 0 (on surface at 1.0 from centers -1 and +1)
        // Actually distance from (0,0,0) to center (-1,0,0) is 1.0, minus radius 1.0 = 0.0
        assert!((u.value(Vec3::ZERO)).abs() < 1e-6);
    }

    #[test]
    fn difference_subtracts() {
        let a = Sphere::new(2.0);
        let b = Sphere::new(1.0);
        let d = Difference::new(a, b);
        // At origin: a=-2, -b=1 → max(-2, 1) = 1 (outside the shell)
        assert!(d.value(Vec3::ZERO) > 0.0);
        // At (1.5, 0, 0): a=-0.5, b=0.5, -b=-0.5 → max(-0.5, -0.5) = -0.5 (inside shell)
        assert!(d.value(Vec3::new(1.5, 0.0, 0.0)) < 0.0);
    }

    #[test]
    fn solve_finds_zero_crossing() {
        let s = Sphere::new(1.0);
        let result = s.solve(Vec3::ZERO, Vec3::new(2.0, 0.0, 0.0));
        assert!(result.is_some());
        let p = result.unwrap_or(Vec3::ZERO);
        assert!((p.x - 1.0).abs() < 0.01, "p.x = {}", p.x);
    }

    #[test]
    fn normal_points_outward_on_sphere() {
        let s = Sphere::new(1.0);
        let n = s.normal(Vec3::new(1.0, 0.0, 0.0));
        assert!((n.x - 1.0).abs() < 0.1, "n = {:?}", n);
    }

    #[test]
    fn torus_sdf() {
        let t = Torus::new(2.0, 0.5);
        // On the tube surface at (2, 0, 0)
        assert!((t.value(Vec3::new(2.5, 0.0, 0.0))).abs() < 1e-5);
        // Inside the tube
        assert!(t.value(Vec3::new(2.0, 0.0, 0.0)) < 0.0);
    }

    #[test]
    fn capsule_sdf() {
        let c = Capsule::new(Vec3::ZERO, Vec3::new(0.0, 2.0, 0.0), 0.5);
        // On the surface
        assert!((c.value(Vec3::new(0.5, 1.0, 0.0))).abs() < 1e-5);
    }
}
