//! Mesh repair pipeline driver.
//!
//! Composes a chain of [`MeshRepairPass`] into a runnable pipeline. The
//! pipeline takes an [`IsoMesh`], runs pre-construction passes on it,
//! builds a [`HalfEdgeMesh`], runs half-edge passes, then re-emits an
//! `IsoMesh`. Per-pass outcomes are aggregated into a [`RepairReport`].

use std::time::{Duration, Instant};

use glam::Vec3;

use super::error::RepairError;
use super::half_edge::HalfEdgeMesh;
use super::pass::{MeshRepairPass, PassOutcome, PassStage, PassWarning, PassWarningKind};
use super::quality::{MeshQualityReport, QualityThresholds};
use crate::isosurface::IsoMesh;

/// How the pipeline reacts when a pass fails or leaves the mesh
/// non-manifold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailurePolicy {
    /// Abort the run on the first pass error or mid-pipeline non-manifold
    /// state.
    StopOnFirstError,
    /// Record a warning and continue with subsequent passes that do not
    /// require manifoldness.
    #[default]
    ContinueOnError,
}

/// Aggregate report returned by [`MeshRepairPipeline::run`].
#[derive(Debug, Clone)]
pub struct RepairReport {
    /// One entry per pass that actually ran. Skipped passes are recorded
    /// as outcomes with a [`PassWarningKind::Skipped`] warning rather than
    /// being omitted, so the caller sees the full pass-chain timeline.
    pub per_pass: Vec<PassOutcome>,
    /// Mesh quality before the first pass.
    pub pre_quality: MeshQualityReport,
    /// Mesh quality after the final pass.
    pub post_quality: MeshQualityReport,
    /// Total wall-clock time across the run.
    pub total_elapsed: Duration,
}

/// A composable mesh repair pipeline.
///
/// Passes are stored in registration order and run in that order. Passes
/// that require half-edge topology run after pre-construction passes have
/// finished.
#[derive(Default)]
pub struct MeshRepairPipeline {
    passes: Vec<Box<dyn MeshRepairPass>>,
    policy: FailurePolicy,
    quality_thresholds: QualityThresholds,
}

impl std::fmt::Debug for MeshRepairPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshRepairPipeline")
            .field(
                "passes",
                &self.passes.iter().map(|p| p.name()).collect::<Vec<_>>(),
            )
            .field("policy", &self.policy)
            .field("quality_thresholds", &self.quality_thresholds)
            .finish()
    }
}

impl MeshRepairPipeline {
    /// Creates an empty pipeline with default failure policy
    /// ([`FailurePolicy::ContinueOnError`]).
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a pass to the pipeline.
    pub fn add(&mut self, pass: impl MeshRepairPass + 'static) -> &mut Self {
        self.passes.push(Box::new(pass));
        self
    }

    /// Sets the failure policy.
    pub fn with_policy(mut self, p: FailurePolicy) -> Self {
        self.policy = p;
        self
    }

    /// Sets the quality thresholds used for pre/post quality reports.
    pub fn with_quality_thresholds(mut self, t: QualityThresholds) -> Self {
        self.quality_thresholds = t;
        self
    }

    /// Returns the current failure policy.
    pub fn policy(&self) -> FailurePolicy {
        self.policy
    }

    /// Returns the number of passes in the pipeline.
    pub fn len(&self) -> usize {
        self.passes.len()
    }

    /// Returns `true` if the pipeline has no passes.
    pub fn is_empty(&self) -> bool {
        self.passes.is_empty()
    }

    /// Builds the v1 "standard" repair chain:
    ///
    /// 1. [`WeldVertices`](super::passes::WeldVertices) (pre-construction)
    ///    with default bbox-relative epsilon of `1e-6 × diag`.
    /// 2. [`FillSmallHoles`](super::passes::FillSmallHoles) with a budget of
    ///    8 boundary half-edges.
    /// 3. [`RemoveSlivers`](super::passes::RemoveSlivers) with min-angle 5°
    ///    and max-aspect-ratio 50.
    /// 4. [`TaubinSmooth`](super::passes::TaubinSmooth) with λ=0.33, μ=-0.34,
    ///    two iterations, boundary pinned.
    pub fn standard() -> Self {
        let mut pipeline = Self::new();
        pipeline
            .add(super::passes::WeldVertices::default())
            .add(super::passes::FillSmallHoles::default())
            .add(super::passes::RemoveSlivers::default())
            .add(super::passes::TaubinSmooth::default());
        pipeline
    }

    /// Builds an empty pipeline for callers that want only the pre/post
    /// quality reports without mutating geometry.
    pub fn inspect_only() -> Self {
        Self::new()
    }

    /// Runs the pipeline on an `IsoMesh`, returning a new `IsoMesh`.
    ///
    /// `normal_fn` is invoked during writeback to compute per-vertex
    /// normals. Callers that have a [`ScalarField`](crate::isosurface::ScalarField)
    /// typically pass `|p| field.normal(p)`.
    pub fn run(
        &self,
        iso: &IsoMesh,
        normal_fn: impl Fn(Vec3) -> Vec3,
    ) -> Result<(IsoMesh, RepairReport), RepairError> {
        let total_start = Instant::now();

        // Pre-construction passes mutate the IsoMesh.
        let mut current_iso = iso.clone();
        let mut per_pass: Vec<PassOutcome> = Vec::with_capacity(self.passes.len());

        for pass in self
            .passes
            .iter()
            .filter(|p| p.stage() == PassStage::PreConstruction)
        {
            let pass_start = Instant::now();
            match pass.pre_construction(current_iso) {
                Ok((iso_out, mut outcome)) => {
                    outcome.elapsed = pass_start.elapsed();
                    current_iso = iso_out;
                    per_pass.push(outcome);
                }
                Err(err) => {
                    if matches!(self.policy, FailurePolicy::StopOnFirstError) {
                        return Err(RepairError::Pass {
                            pass: pass.name(),
                            source: err,
                        });
                    }
                    // ContinueOnError: record as warning outcome and skip. We
                    // have no IsoMesh from the failure (pass consumed the input
                    // but didn't return one), so we can't proceed.
                    return Err(RepairError::Pass {
                        pass: pass.name(),
                        source: err,
                    });
                }
            }
        }

        // Build the half-edge mesh. The quality report on the IsoMesh must be
        // computed via a half-edge build too, so just build once here.
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&current_iso)?;
        let pre_quality = mesh.quality_report(&self.quality_thresholds);
        let mut manifold_broken = false;

        // Half-edge passes.
        for pass in self
            .passes
            .iter()
            .filter(|p| p.stage() == PassStage::HalfEdge)
        {
            if manifold_broken && pass.requires_manifold() {
                let mut skipped = PassOutcome::noop(pass.name());
                skipped.warn(
                    PassWarningKind::Skipped,
                    "skipped: previous pass left mesh non-manifold",
                );
                per_pass.push(skipped);
                continue;
            }
            let pass_start = Instant::now();
            match pass.apply(&mut mesh) {
                Ok(mut outcome) => {
                    outcome.elapsed = pass_start.elapsed();
                    per_pass.push(outcome);
                    if !mesh.is_manifold() {
                        manifold_broken = true;
                        if matches!(self.policy, FailurePolicy::StopOnFirstError) {
                            return Err(RepairError::NonManifoldMidPipeline {
                                after_pass: pass.name(),
                            });
                        }
                        // Record a warning for diagnostics.
                        if let Some(last) = per_pass.last_mut() {
                            last.warnings.push(PassWarning {
                                kind: PassWarningKind::Skipped,
                                message: "pass left mesh non-manifold".into(),
                            });
                        }
                    }
                }
                Err(err) => {
                    if matches!(self.policy, FailurePolicy::StopOnFirstError) {
                        return Err(RepairError::Pass {
                            pass: pass.name(),
                            source: err,
                        });
                    }
                    let mut failed = PassOutcome::noop(pass.name());
                    failed.warn(PassWarningKind::Skipped, format!("pass failed: {err}"));
                    failed.elapsed = pass_start.elapsed();
                    per_pass.push(failed);
                }
            }
        }

        let post_quality = mesh.quality_report(&self.quality_thresholds);
        let out_iso = mesh.to_iso_mesh(normal_fn);

        Ok((
            out_iso,
            RepairReport {
                per_pass,
                pre_quality,
                post_quality,
                total_elapsed: total_start.elapsed(),
            },
        ))
    }

    /// Runs only the half-edge passes on an already-built [`HalfEdgeMesh`].
    ///
    /// Useful for callers who want to apply repair steps to a mesh they
    /// constructed programmatically (tests, caching).
    pub fn run_in_place(&self, mesh: &mut HalfEdgeMesh) -> Result<RepairReport, RepairError> {
        let total_start = Instant::now();
        let pre_quality = mesh.quality_report(&self.quality_thresholds);
        let mut per_pass: Vec<PassOutcome> = Vec::with_capacity(self.passes.len());
        let mut manifold_broken = false;

        for pass in self
            .passes
            .iter()
            .filter(|p| p.stage() == PassStage::HalfEdge)
        {
            if manifold_broken && pass.requires_manifold() {
                let mut skipped = PassOutcome::noop(pass.name());
                skipped.warn(
                    PassWarningKind::Skipped,
                    "skipped: previous pass left mesh non-manifold",
                );
                per_pass.push(skipped);
                continue;
            }
            let pass_start = Instant::now();
            match pass.apply(mesh) {
                Ok(mut outcome) => {
                    outcome.elapsed = pass_start.elapsed();
                    per_pass.push(outcome);
                    if !mesh.is_manifold() {
                        manifold_broken = true;
                        if matches!(self.policy, FailurePolicy::StopOnFirstError) {
                            return Err(RepairError::NonManifoldMidPipeline {
                                after_pass: pass.name(),
                            });
                        }
                    }
                }
                Err(err) => {
                    if matches!(self.policy, FailurePolicy::StopOnFirstError) {
                        return Err(RepairError::Pass {
                            pass: pass.name(),
                            source: err,
                        });
                    }
                    let mut failed = PassOutcome::noop(pass.name());
                    failed.warn(PassWarningKind::Skipped, format!("pass failed: {err}"));
                    failed.elapsed = pass_start.elapsed();
                    per_pass.push(failed);
                }
            }
        }

        let post_quality = mesh.quality_report(&self.quality_thresholds);
        Ok(RepairReport {
            per_pass,
            pre_quality,
            post_quality,
            total_elapsed: total_start.elapsed(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_repair::error::PassError;
    use crate::mesh_repair::pass::PassStats;

    fn single_triangle() -> IsoMesh {
        IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        }
    }

    #[test]
    fn empty_pipeline_is_identity() {
        let pipeline = MeshRepairPipeline::new();
        let iso = single_triangle();
        let (out, report) = pipeline.run(&iso, |_| Vec3::Z).expect("run");
        assert_eq!(out.positions.len(), iso.positions.len());
        assert_eq!(out.indices.len(), iso.indices.len());
        assert_eq!(report.per_pass.len(), 0);
        assert_eq!(report.pre_quality.triangle_count, 1);
        assert_eq!(report.post_quality.triangle_count, 1);
    }

    struct FailingPass;
    impl MeshRepairPass for FailingPass {
        fn name(&self) -> &'static str {
            "failing"
        }
        fn apply(&self, _mesh: &mut HalfEdgeMesh) -> Result<PassOutcome, PassError> {
            Err(PassError::Aborted {
                pass: "failing",
                detail: "synthetic failure".into(),
            })
        }
    }

    struct CountingPass;
    impl MeshRepairPass for CountingPass {
        fn name(&self) -> &'static str {
            "counting"
        }
        fn apply(&self, _mesh: &mut HalfEdgeMesh) -> Result<PassOutcome, PassError> {
            let mut outcome = PassOutcome::noop("counting");
            outcome.stats = PassStats {
                vertices_smoothed: 1,
                ..PassStats::default()
            };
            Ok(outcome)
        }
    }

    #[test]
    fn stop_on_first_error_aborts() {
        let mut pipeline = MeshRepairPipeline::new().with_policy(FailurePolicy::StopOnFirstError);
        pipeline.add(FailingPass);
        pipeline.add(CountingPass);
        let err = pipeline
            .run(&single_triangle(), |_| Vec3::Z)
            .expect_err("should fail");
        assert!(matches!(
            err,
            RepairError::Pass {
                pass: "failing",
                ..
            }
        ));
    }

    #[test]
    fn continue_on_error_records_warning_and_runs_next() {
        let mut pipeline = MeshRepairPipeline::new();
        pipeline.add(FailingPass);
        pipeline.add(CountingPass);
        let (_iso, report) = pipeline.run(&single_triangle(), |_| Vec3::Z).expect("run");
        assert_eq!(report.per_pass.len(), 2);
        assert_eq!(report.per_pass[0].name, "failing");
        assert!(!report.per_pass[0].warnings.is_empty());
        assert_eq!(report.per_pass[1].name, "counting");
        assert_eq!(report.per_pass[1].stats.vertices_smoothed, 1);
    }

    #[test]
    fn builder_pattern_chains_passes() {
        let mut pipeline = MeshRepairPipeline::new();
        pipeline.add(CountingPass).add(CountingPass);
        assert_eq!(pipeline.len(), 2);
    }

    #[test]
    fn standard_pipeline_runs_on_single_triangle_without_error() {
        // Single triangle: welder+hole fill (triangle is its own hole, 3 edges,
        // within budget 8) + sliver removal (equilateral, not a sliver) + Taubin
        // (boundary pinned → no-op). End-to-end smoke test for the standard chain.
        let pipeline = MeshRepairPipeline::standard();
        let iso = single_triangle();
        let (_out, report) = pipeline.run(&iso, |_| Vec3::Z).expect("run");
        assert_eq!(report.per_pass.len(), 4);
        assert_eq!(report.per_pass[0].name, "weld_vertices");
        assert_eq!(report.per_pass[1].name, "fill_small_holes");
        assert_eq!(report.per_pass[2].name, "remove_slivers");
        assert_eq!(report.per_pass[3].name, "taubin_smooth");
    }

    #[test]
    fn standard_pipeline_reduces_sliver_count() {
        // Three-triangle strip with one needle in the middle — the same
        // fixture RemoveSlivers is tested against, now run through the
        // full standard chain.
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
                Vec3::new(2.0, 1.0, 0.0),
                Vec3::new(3.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(1.001, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
            ],
            normals: vec![Vec3::Z; 8],
            indices: vec![0, 1, 4, 1, 5, 4, 1, 2, 5, 2, 6, 5, 2, 3, 6, 3, 7, 6],
        };
        let pre = iso.quality_report().expect("pre");
        let pipeline = MeshRepairPipeline::standard();
        let (out, _report) = pipeline.run(&iso, |_| Vec3::Z).expect("run");
        let post = out.quality_report().expect("post");
        assert!(
            post.sliver_count < pre.sliver_count,
            "standard pipeline should reduce slivers: pre={} post={}",
            pre.sliver_count,
            post.sliver_count
        );
    }

    #[test]
    fn run_in_place_reports_pre_and_post_quality() {
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&single_triangle()).expect("build");
        let mut pipeline = MeshRepairPipeline::new();
        pipeline.add(CountingPass);
        let report = pipeline.run_in_place(&mut mesh).expect("in-place");
        assert_eq!(report.pre_quality.triangle_count, 1);
        assert_eq!(report.post_quality.triangle_count, 1);
        assert_eq!(report.per_pass.len(), 1);
    }
}
