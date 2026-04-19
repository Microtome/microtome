//! Isosurface extraction via dual contouring with octree and k-d tree acceleration.
//!
//! Port of the KdtreeISO library: discrete k-d tree hierarchy for isosurface extraction.

pub mod indicators;
#[allow(unused, clippy::all, clippy::unwrap_used, clippy::expect_used)]
pub mod kdtree;
pub mod kdtree_v2;
mod mesh_bvh;
pub mod mesh_output;
pub mod mesh_scan;
pub mod octree;
#[allow(unused, clippy::all, clippy::unwrap_used, clippy::expect_used)]
pub mod qef;
#[allow(unused, clippy::all, clippy::unwrap_used, clippy::expect_used)]
pub mod rectilinear_grid;
pub mod scalar_field;
mod sign_gen;
pub mod vertex;
pub mod volume_data;

pub use indicators::PositionCode;
pub use kdtree::KdTreeNode;
pub use kdtree_v2::KdTreeV2Node;
pub use mesh_output::IsoMesh;
pub use mesh_scan::{ScannedMeshField, SignMode};
pub use octree::OctreeNode;
pub use qef::QefSolver;
pub use rectilinear_grid::RectilinearGrid;
pub use scalar_field::{
    Aabb, Capsule, Cylinder, Difference, Heart, Intersection, ScalarField, SmoothUnion, Sphere,
    Torus, TransformedField, Union, UnionList,
};
pub use vertex::Vertex;
pub use volume_data::VolumeData;
