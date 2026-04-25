//! Mesh repair pipeline for `IsoMesh` output.
//!
//! Post-hoc, composable repair passes that run on the output of dual-contouring
//! extraction (or any other producer of [`IsoMesh`](crate::isosurface::IsoMesh)).
//! The pipeline builds a [`HalfEdgeMesh`](half_edge::HalfEdgeMesh) on demand,
//! runs an ordered chain of [`MeshRepairPass`](pass::MeshRepairPass)
//! implementations, and re-emits an `IsoMesh`.
//!
//! ## Pre-construction passes
//!
//! Run on the [`IsoMesh`] before half-edge construction:
//!
//! - [`WeldVertices`](passes::WeldVertices) — merges coincident vertices.
//! - [`CleanMesh`](passes::CleanMesh) — duplicate-face dedup, orphan removal,
//!   T-junction split, surplus-face drop on non-manifold edges, winding
//!   propagation, and (with a target) outward-normal winding fix.
//!
//! ## Half-edge passes
//!
//! Run after construction; receive the [`HalfEdgeMesh`] and a
//! [`RepairContext`]:
//!
//! - [`FillSmallHoles`](passes::FillSmallHoles) — triangulates small boundary loops.
//! - [`RemoveSlivers`](passes::RemoveSlivers) — collapses or flips low-quality triangles.
//! - [`TaubinSmooth`](passes::TaubinSmooth) — volume-preserving Laplacian smoothing.
//! - [`FeatureSmooth`](passes::FeatureSmooth) — class-aware HC-Laplacian /
//!   Bilateral smoothing that respects [`VertexClass`].
//! - [`AngleRelax`](passes::AngleRelax) — tangential angle-equalising relaxation.
//! - [`ReprojectToSurface`](passes::ReprojectToSurface) — pulls vertices back
//!   onto a [`ReprojectionTarget`], tangent-constrained for `Feature` /
//!   `Boundary` classes.
//! - [`SimplifyQuadric`](passes::SimplifyQuadric) — Garland-Heckbert quadric
//!   edge-collapse simplification with normal-flip + volume-tolerance pre-checks.
//! - [`IsotropicRemesh`](passes::IsotropicRemesh) — composite split / collapse /
//!   flip / relax / reproject pass for uniform triangle size.
//! - [`DetectSelfIntersections`](passes::DetectSelfIntersections) — query-only
//!   self-intersection detection (BVH-accelerated).
//! - [`RepairSelfIntersections`](passes::RepairSelfIntersections) — drops faces
//!   participating in any self-intersection so [`FillSmallHoles`] can patch.
//!
//! ## Cross-cutting types
//!
//! - [`RepairContext`] — per-run shared state (normal_fn, target, classifier,
//!   features) passed to every pass.
//! - [`VertexClassifier`] / [`VertexClass`] — feature / boundary detection by
//!   dihedral threshold + caller-supplied [`FeatureSet`] creases.
//! - [`MeshRepairPipeline`] — orchestrates passes, runs the classifier between
//!   connectivity-changing passes, emits a [`RepairReport`].

pub mod context;
pub mod error;
pub mod features;
pub mod half_edge;
pub mod half_edge_ops;
pub mod pass;
pub mod passes;
pub mod pipeline;
pub mod quality;
pub mod reprojection;
pub mod spatial;
pub mod tangent;
pub mod vertex_class;
pub mod vertex_quadric;

pub use context::RepairContext;
pub use error::{HalfEdgeOpError, PassError, RepairError, TopologyError};
pub use features::FeatureSet;
pub use half_edge::{FaceId, HalfEdgeId, HalfEdgeMesh, VertexId};
pub use pass::{MeshRepairPass, PassOutcome, PassStage, PassStats, PassWarning, PassWarningKind};
pub use pipeline::{FailurePolicy, MeshRepairPipeline, RepairReport};
pub use quality::{MeshQualityReport, QualityThresholds, TriangleQuality};
pub use reprojection::{MeshTarget, ProjectionResult, ReprojectionTarget, ScalarFieldTarget};
pub use vertex_class::{VertexClass, VertexClassifier};
pub use vertex_quadric::{QuadricWeights, VertexQuadric, accumulate_for_mesh};
