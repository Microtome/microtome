//! Isosurface extraction via dual contouring with octree and k-d tree acceleration.
//!
//! Port of the KdtreeISO library: discrete k-d tree hierarchy for isosurface extraction.

pub mod indicators;
// KD-tree DC simplification is gated behind the `kdtree_simplification`
// feature: the algorithm produces long-edge sliver triangles on flat faces,
// matching the C++ reference but unusable without post-cleanup.
#[cfg(feature = "kdtree_simplification")]
#[allow(unused, clippy::all, clippy::unwrap_used, clippy::expect_used)]
pub mod kdtree;
#[cfg(feature = "kdtree_simplification")]
pub mod kdtree_v2;
mod mesh_bvh;
pub mod mesh_output;
pub mod mesh_scan;
pub mod octree;
mod polymender;
pub mod qef;
pub mod rectilinear_grid;
pub mod scalar_field;
// sign_gen.rs was a previous, unfinished attempt at PolyMender-style
// repair. The working implementation now lives in `polymender.rs`;
// keep this around as a reference but relax strict lints so the dead
// code doesn't block the workspace build.
#[allow(unused, clippy::all, clippy::unwrap_used, clippy::expect_used)]
mod sign_gen;
pub mod vertex;
pub mod volume_data;

pub use indicators::PositionCode;
#[cfg(feature = "kdtree_simplification")]
pub use kdtree::KdTreeNode;
#[cfg(feature = "kdtree_simplification")]
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
