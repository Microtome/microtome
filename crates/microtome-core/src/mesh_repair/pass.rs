//! Pass trait and shared pass types.
//!
//! A [`MeshRepairPass`] is a named, composable operation on a mesh. Passes
//! either run pre-construction (on an [`IsoMesh`] before the half-edge mesh
//! is built) or post-construction (mutating a [`HalfEdgeMesh`] directly).
//! The default trait implementations cover both stages as no-ops so concrete
//! passes override only the one they need.

use std::time::Duration;

use super::context::RepairContext;
use super::error::PassError;
use super::half_edge::HalfEdgeMesh;
use crate::isosurface::IsoMesh;

/// Which stage of the pipeline a pass runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassStage {
    /// Runs on the [`IsoMesh`] before the half-edge mesh is built. Only
    /// vertex welding uses this in v1; later passes that want pre-construction
    /// cleanup declare it too.
    PreConstruction,
    /// Runs on an already-built [`HalfEdgeMesh`]. The default.
    HalfEdge,
}

/// A composable mesh repair operation.
///
/// Implementors override exactly one of [`apply`](Self::apply) or
/// [`pre_construction`](Self::pre_construction), depending on their stage.
/// Both have no-op defaults.
pub trait MeshRepairPass: Send + Sync {
    /// Short, stable identifier used for diagnostics and error messages.
    /// Must be `'static` so it can flow through errors and reports cheaply.
    fn name(&self) -> &'static str;

    /// Runs the pass on a half-edge mesh. Default: no-op.
    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        let _ = (mesh, ctx);
        Ok(PassOutcome::noop(self.name()))
    }

    /// Runs the pass on an `IsoMesh` before half-edge construction. Default: no-op.
    fn pre_construction(
        &self,
        iso: IsoMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<(IsoMesh, PassOutcome), PassError> {
        let _ = ctx;
        Ok((iso, PassOutcome::noop(self.name())))
    }

    /// Returns which pipeline stage this pass runs in. Implementations
    /// must declare this explicitly: a missing override used to default to
    /// `HalfEdge` and silently mismatch with a `pre_construction` override,
    /// which produced runtime no-ops instead of compile-time errors.
    fn stage(&self) -> PassStage;

    /// Whether this pass requires its input to be manifold.
    ///
    /// Pipeline validation skips manifold-requiring passes after a preceding
    /// pass has corrupted manifoldness (under `ContinueOnError` policy).
    fn requires_manifold(&self) -> bool {
        true
    }

    /// `true` if this pass mutates connectivity in a way that invalidates
    /// the per-vertex class. The pipeline reruns the classifier after such
    /// passes when a [`RepairContext`] supplies one. Default `false`.
    fn reclassifies(&self) -> bool {
        false
    }
}

/// Reason a pass skipped part of its work or clamped a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassWarningKind {
    /// A candidate operation could not be performed (e.g. sliver collapse
    /// rejected by the link condition).
    Skipped,
    /// A parameter was clamped to a valid range.
    Clamped,
    /// A budget (e.g. maximum hole perimeter) was exceeded; the operation
    /// was skipped rather than producing large results.
    BudgetExceeded,
}

/// A non-fatal event recorded by a pass.
#[derive(Debug, Clone)]
pub struct PassWarning {
    /// Classification of the event.
    pub kind: PassWarningKind,
    /// Human-readable detail.
    pub message: String,
}

/// Per-pass counters recorded in a [`PassOutcome`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStats {
    /// Vertices merged via welding.
    pub vertices_merged: u32,
    /// New vertices added (e.g. hole centroids, edge splits).
    pub vertices_added: u32,
    /// Vertices whose position moved.
    pub vertices_smoothed: u32,
    /// Edges flipped.
    pub edges_flipped: u32,
    /// Edges collapsed.
    pub edges_collapsed: u32,
    /// Edges split.
    pub edges_split: u32,
    /// Faces removed.
    pub faces_removed: u32,
    /// Faces added.
    pub faces_added: u32,
    /// Holes (boundary loops) filled.
    pub holes_filled: u32,
}

/// The result of running a single pass.
#[derive(Debug, Clone)]
pub struct PassOutcome {
    /// The pass's name (from [`MeshRepairPass::name`]).
    pub name: &'static str,
    /// Counter summary.
    pub stats: PassStats,
    /// Soft warnings collected during the pass.
    pub warnings: Vec<PassWarning>,
    /// Wall-clock time the pass took. Populated by the pipeline wrapper.
    pub elapsed: Duration,
}

impl PassOutcome {
    /// Builds a no-op outcome with zero stats and no warnings. The pipeline
    /// fills in `elapsed` after timing the pass invocation.
    pub fn noop(name: &'static str) -> Self {
        Self {
            name,
            stats: PassStats::default(),
            warnings: Vec::new(),
            elapsed: Duration::ZERO,
        }
    }

    /// Pushes a warning onto the outcome.
    pub fn warn(&mut self, kind: PassWarningKind, message: impl Into<String>) {
        self.warnings.push(PassWarning {
            kind,
            message: message.into(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoopPass;
    impl MeshRepairPass for NoopPass {
        fn name(&self) -> &'static str {
            "noop"
        }
        fn stage(&self) -> PassStage {
            PassStage::HalfEdge
        }
    }

    #[test]
    fn declared_stage_is_halfedge() {
        let pass = NoopPass;
        assert_eq!(pass.stage(), PassStage::HalfEdge);
    }

    #[test]
    fn default_requires_manifold_true() {
        let pass = NoopPass;
        assert!(pass.requires_manifold());
    }

    #[test]
    fn pass_outcome_noop_has_zero_stats() {
        let o = PassOutcome::noop("x");
        assert_eq!(o.stats, PassStats::default());
        assert!(o.warnings.is_empty());
        assert_eq!(o.elapsed, Duration::ZERO);
    }

    #[test]
    fn pass_outcome_warn_appends_warning() {
        let mut o = PassOutcome::noop("x");
        o.warn(PassWarningKind::Skipped, "could not collapse");
        assert_eq!(o.warnings.len(), 1);
        assert_eq!(o.warnings[0].kind, PassWarningKind::Skipped);
    }

    #[test]
    fn default_apply_returns_noop() {
        let pass = NoopPass;
        let mut mesh = HalfEdgeMesh::new();
        let nf = |_p: glam::Vec3| glam::Vec3::Z;
        let ctx = RepairContext::new(&nf);
        let outcome = pass.apply(&mut mesh, &ctx).expect("noop");
        assert_eq!(outcome.name, "noop");
        assert_eq!(outcome.stats, PassStats::default());
    }

    #[test]
    fn default_pre_construction_returns_iso_unchanged() {
        let pass = NoopPass;
        let iso = IsoMesh::new();
        let pre_len = iso.indices.len();
        let nf = |_p: glam::Vec3| glam::Vec3::Z;
        let ctx = RepairContext::new(&nf);
        let (out_iso, outcome) = pass.pre_construction(iso, &ctx).expect("noop");
        assert_eq!(out_iso.indices.len(), pre_len);
        assert_eq!(outcome.name, "noop");
    }

    #[test]
    fn default_reclassifies_is_false() {
        let pass = NoopPass;
        assert!(!pass.reclassifies());
    }
}
