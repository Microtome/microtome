//! Mesh repair pipeline for `IsoMesh` output.
//!
//! Post-hoc, composable repair passes that run on the output of dual-contouring
//! extraction (or any other producer of [`IsoMesh`](crate::isosurface::IsoMesh)).
//! The pipeline builds a [`HalfEdgeMesh`](half_edge::HalfEdgeMesh) on demand,
//! runs an ordered chain of [`MeshRepairPass`](pass::MeshRepairPass)
//! implementations, and re-emits an `IsoMesh`.
//!
//! v1 passes:
//! - [`WeldVertices`](passes::WeldVertices) — merges coincident vertices (pre-construction).
//! - [`FillSmallHoles`](passes::FillSmallHoles) — triangulates small boundary loops.
//! - [`RemoveSlivers`](passes::RemoveSlivers) — collapses or flips low-quality triangles.
//! - [`TaubinSmooth`](passes::TaubinSmooth) — volume-preserving Laplacian smoothing.
//!
//! See the plan at `/home/djoyce/.claude/plans/stateful-sauteeing-wave.md`
//! for the full design, including v2 passes not yet implemented.

pub mod error;
pub mod features;
pub mod half_edge;
pub mod half_edge_ops;
pub mod pass;
pub mod passes;
pub mod pipeline;
pub mod quality;
pub mod vertex_class;

pub use error::{HalfEdgeOpError, PassError, RepairError, TopologyError};
pub use features::FeatureSet;
pub use half_edge::{FaceId, HalfEdgeId, HalfEdgeMesh, VertexId};
pub use pass::{MeshRepairPass, PassOutcome, PassStage, PassStats, PassWarning, PassWarningKind};
pub use pipeline::{FailurePolicy, MeshRepairPipeline, RepairReport};
pub use quality::{MeshQualityReport, QualityThresholds, TriangleQuality};
pub use vertex_class::{VertexClass, VertexClassifier};
