//! Concrete mesh repair passes.

pub mod angle_relax;
pub mod clean_mesh;
pub mod detect_self_intersect;
pub mod feature_smooth;
pub mod fill_holes;
pub mod isotropic_remesh;
pub mod remove_slivers;
pub mod repair_self_intersect;
pub mod reproject;
pub mod simplify_quadric;
pub mod taubin_smooth;
pub mod weld_vertices;

pub use angle_relax::AngleRelax;
pub use clean_mesh::CleanMesh;
pub use detect_self_intersect::DetectSelfIntersections;
pub use feature_smooth::{FeatureSmooth, FeatureSmoothMethod};
pub use fill_holes::FillSmallHoles;
pub use isotropic_remesh::IsotropicRemesh;
pub use remove_slivers::RemoveSlivers;
pub use repair_self_intersect::RepairSelfIntersections;
pub use reproject::ReprojectToSurface;
pub use simplify_quadric::SimplifyQuadric;
pub use taubin_smooth::TaubinSmooth;
pub use weld_vertices::{EpsilonMode, WeldVertices};
