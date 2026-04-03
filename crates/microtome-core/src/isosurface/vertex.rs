//! Vertex type for hermite data used in dual contouring.

use glam::Vec3;

/// A vertex produced by QEF solving during dual contouring.
///
/// Stores the hermite interpolation point (position on the isosurface),
/// the surface normal, the QEF error, and an index into the output mesh.
#[derive(Debug, Clone)]
pub struct Vertex {
    /// Position on the isosurface (hermite point).
    pub hermite_p: Vec3,
    /// Surface normal at the hermite point.
    pub hermite_n: Vec3,
    /// QEF error for this vertex. Negative means uninitialized.
    pub error: f32,
    /// Index into the output mesh vertex array.
    pub vertex_index: u32,
    /// Index of the parent vertex (for hierarchical simplification).
    pub parent: Option<usize>,
}

impl Vertex {
    /// Creates a new vertex at the given position with default normal and error.
    pub fn new(hermite_p: Vec3) -> Self {
        Self {
            hermite_p,
            hermite_n: Vec3::ZERO,
            error: -1.0,
            vertex_index: 0,
            parent: None,
        }
    }
}

impl Default for Vertex {
    fn default() -> Self {
        Self {
            hermite_p: Vec3::ZERO,
            hermite_n: Vec3::ZERO,
            error: -1.0,
            vertex_index: 0,
            parent: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_vertex_has_negative_error() {
        let v = Vertex::default();
        assert!(v.error < 0.0);
        assert_eq!(v.vertex_index, 0);
        assert!(v.parent.is_none());
    }

    #[test]
    fn new_vertex_stores_position() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        let v = Vertex::new(p);
        assert_eq!(v.hermite_p, p);
        assert_eq!(v.hermite_n, Vec3::ZERO);
    }
}
