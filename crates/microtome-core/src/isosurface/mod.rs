//! Isosurface extraction via dual contouring with octree and k-d tree acceleration.
//!
//! Port of the KdtreeISO library: discrete k-d tree hierarchy for isosurface extraction.

pub mod indicators;
pub mod mesh_output;
pub mod qef;
pub mod scalar_field;
pub mod vertex;

pub use indicators::PositionCode;
pub use mesh_output::IsoMesh;
pub use qef::QefSolver;
pub use scalar_field::{
    Aabb, Capsule, Cylinder, Difference, Heart, Intersection, ScalarField, SmoothUnion, Sphere,
    Torus, TransformedField, Union, UnionList,
};
pub use vertex::Vertex;
