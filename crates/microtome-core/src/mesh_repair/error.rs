//! Error types for the mesh repair pipeline.
//!
//! Four layered error types:
//!
//! - [`TopologyError`] — input mesh is malformed; callers should treat these
//!   as "bad input" and either reject or run a cleaning pass first.
//! - [`HalfEdgeOpError`] — a half-edge operation (collapse / flip / split /
//!   face removal) could not be performed safely, typically because a
//!   precondition like the link condition was violated.
//! - [`PassError`] — a single pass failed to run. Wraps [`HalfEdgeOpError`]
//!   for op-level failures, or carries bespoke configuration / stage errors.
//! - [`RepairError`] — a pipeline run failed. Wraps [`TopologyError`] for
//!   construction failures and [`PassError`] for per-pass failures.

use thiserror::Error;

use super::half_edge::{HalfEdgeId, VertexId};

/// An input [`IsoMesh`](crate::isosurface::IsoMesh) could not be converted
/// to a [`HalfEdgeMesh`](super::half_edge::HalfEdgeMesh).
///
/// These errors describe malformed input: index buffers that cannot be
/// triangulated, indices pointing past the vertex array, or edges shared
/// by three or more triangles (non-manifold).
#[derive(Debug, Error)]
pub enum TopologyError {
    /// The index buffer length is not a multiple of 3.
    #[error("index buffer length {len} is not a multiple of 3")]
    NonTriangleFace {
        /// Length of the offending index buffer.
        len: usize,
    },

    /// A triangle has two or more equal indices.
    #[error("triangle {face_index} has duplicate indices {indices:?}")]
    DegenerateTriangle {
        /// Position of the triangle in the index buffer (0-based).
        face_index: usize,
        /// The three indices that made the triangle degenerate.
        indices: [u32; 3],
    },

    /// A triangle index is `>= positions.len()`.
    #[error("triangle {face_index} index {index} exceeds vertex count {vertex_count}")]
    IndexOutOfRange {
        /// Position of the triangle in the index buffer (0-based).
        face_index: usize,
        /// The offending index value.
        index: u32,
        /// Size of the input `positions` array.
        vertex_count: u32,
    },

    /// More than two triangles share the same undirected edge; the input is
    /// not a 2-manifold.
    #[error(
        "edge ({u:?}, {v:?}) is shared by {face_count} faces (non-manifold); first conflict at face {face_index}"
    )]
    NonManifoldEdge {
        /// One endpoint of the non-manifold edge.
        u: VertexId,
        /// The other endpoint.
        v: VertexId,
        /// How many faces reference this edge (always `> 2`).
        face_count: u32,
        /// Position in the index buffer where the conflict was detected.
        face_index: usize,
    },
}

/// A single half-edge operation refused to run safely.
///
/// Operations that would create non-manifold topology, fold a face normal
/// past 90°, or merge two distinct boundary loops return an error rather
/// than silently produce bad geometry.
#[derive(Debug, Error)]
pub enum HalfEdgeOpError {
    /// The supplied half-edge ID is the `INVALID` sentinel or references
    /// a removed / out-of-range slot.
    #[error("half-edge {0:?} is removed or invalid")]
    InvalidHandle(HalfEdgeId),

    /// Collapsing this edge would create a non-manifold pinch. Fails the
    /// link condition (Dey-Edelsbrunner): the one-rings of the two
    /// endpoints share more vertices than the 1–2 that sit opposite the
    /// collapsed edge.
    #[error("link condition fails: collapsing would create a non-manifold pinch")]
    LinkConditionFailed,

    /// The operation would cause at least one adjacent face normal to flip
    /// (dot product with its pre-op normal ≤ configured threshold).
    #[error("operation would flip at least one adjacent face normal")]
    WouldFlipNormal,

    /// Attempted to flip a boundary half-edge; the opposing triangle
    /// required for a diagonal swap does not exist.
    #[error("cannot flip a boundary edge")]
    BoundaryEdgeFlip,

    /// Flipping this edge would produce an edge that already exists between
    /// the two opposite vertices, creating a duplicated edge.
    #[error("flip would duplicate an existing edge")]
    FlipWouldDuplicateEdge,

    /// Both endpoints of the collapsed edge lie on distinct boundary loops.
    /// Merging them would collapse two holes into one.
    #[error("cannot collapse: both endpoints are on different boundary loops")]
    BoundaryMergeForbidden,

    /// A pre-check rejected the operation because the change in local
    /// signed volume exceeded the configured fractional tolerance.
    #[error(
        "operation would change local volume by {delta} (fraction exceeds tolerance {tolerance})"
    )]
    WouldExceedVolumeTolerance {
        /// Magnitude of the volume change relative to total local volume.
        delta: f32,
        /// Fraction-of-volume threshold the operation exceeded.
        tolerance: f32,
    },
}

/// A mesh repair pass failed to run.
///
/// Pipeline policy determines whether this aborts the run or is recorded
/// as a warning and the next pass is tried (see
/// [`FailurePolicy`](super::pipeline::FailurePolicy)).
#[derive(Debug, Error)]
pub enum PassError {
    /// The pass configuration is malformed (e.g. `lambda` out of range).
    #[error("pass configuration invalid: {0}")]
    InvalidConfig(String),

    /// A pre-construction pass rejected the `IsoMesh` before half-edge
    /// construction could be attempted.
    #[error("pre-construction mesh rejected: {0}")]
    PreConstruction(String),

    /// A half-edge operation performed by the pass failed.
    #[error("half-edge mesh operation failed: {0}")]
    HalfEdge(#[from] HalfEdgeOpError),

    /// Pass-specific abort (e.g. "target error not achievable within budget").
    #[error("pass '{pass}' aborted: {detail}")]
    Aborted {
        /// Name of the aborting pass (from `MeshRepairPass::name`).
        pass: &'static str,
        /// Human-readable detail.
        detail: String,
    },
}

/// A pipeline run failed.
#[derive(Debug, Error)]
pub enum RepairError {
    /// The input `IsoMesh` could not be converted to a `HalfEdgeMesh`.
    #[error("failed to build half-edge topology: {0}")]
    Topology(#[from] TopologyError),

    /// A specific pass returned an error and pipeline policy is
    /// [`StopOnFirstError`](super::pipeline::FailurePolicy::StopOnFirstError).
    #[error("pass '{pass}' failed: {source}")]
    Pass {
        /// Name of the failing pass.
        pass: &'static str,
        /// The underlying pass error.
        #[source]
        source: PassError,
    },

    /// A half-edge pass produced a non-manifold mesh and policy is
    /// [`StopOnFirstError`](super::pipeline::FailurePolicy::StopOnFirstError).
    #[error("mesh became non-manifold after pass '{after_pass}' (policy=StopOnFirstError)")]
    NonManifoldMidPipeline {
        /// Name of the last pass that ran before the manifoldness check failed.
        after_pass: &'static str,
    },

    /// Converting the final `HalfEdgeMesh` back to an `IsoMesh` failed.
    #[error("final writeback failed: {0}")]
    Writeback(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_errors_display_correctly() {
        let e = TopologyError::NonTriangleFace { len: 5 };
        let s = format!("{e}");
        assert!(s.contains("not a multiple of 3"));
    }

    #[test]
    fn halfedge_op_errors_display_correctly() {
        let e = HalfEdgeOpError::LinkConditionFailed;
        let s = format!("{e}");
        assert!(s.contains("link condition"));
    }

    #[test]
    fn pass_error_wraps_halfedge_op_error() {
        let halfedge_err = HalfEdgeOpError::BoundaryEdgeFlip;
        let pass_err: PassError = halfedge_err.into();
        // Variant check via Display: should mention boundary.
        assert!(format!("{pass_err}").contains("boundary"));
    }

    #[test]
    fn repair_error_wraps_topology_error_with_source() {
        let topology_err = TopologyError::IndexOutOfRange {
            face_index: 0,
            index: 99,
            vertex_count: 10,
        };
        let repair_err: RepairError = topology_err.into();
        // error::source() should return the inner topology error.
        use std::error::Error;
        assert!(repair_err.source().is_some());
    }

    #[test]
    fn repair_error_pass_variant_exposes_inner_source() {
        let pass_err = PassError::InvalidConfig("lambda out of range".into());
        let repair_err = RepairError::Pass {
            pass: "taubin_smooth",
            source: pass_err,
        };
        use std::error::Error;
        let s = repair_err.source();
        assert!(s.is_some(), "RepairError::Pass should expose its source");
    }
}
