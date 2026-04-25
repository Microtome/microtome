//! Per-run shared state for the mesh-repair pipeline.
//!
//! `RepairContext` bundles the things many passes need but most don't own:
//! the writeback normal closure, an optional reprojection target, the vertex
//! classifier, and an optional explicit feature set. All references live for
//! the duration of a single [`MeshRepairPipeline::run_with`](super::pipeline::MeshRepairPipeline::run_with)
//! call, so callers typically build the context inline.

use glam::Vec3;

use super::features::FeatureSet;
use super::reprojection::ReprojectionTarget;
use super::vertex_class::VertexClassifier;

/// Shared per-run state available to every pass.
///
/// Construct via [`new`](Self::new) with a normal closure, then chain
/// [`with_target`](Self::with_target), [`with_classifier`](Self::with_classifier),
/// and [`with_features`](Self::with_features) to attach the optional pieces.
/// Tests that don't care about any of it can use [`noop`](Self::noop) for
/// a static-lifetime zero-normal context.
pub struct RepairContext<'a> {
    /// Closure invoked at writeback time to populate per-vertex normals.
    /// `|p| Vec3::ZERO` is fine if the caller follows up with
    /// [`IsoMesh::generate_flat_normals`](crate::isosurface::IsoMesh::generate_flat_normals).
    pub normal_fn: &'a dyn Fn(Vec3) -> Vec3,
    /// Optional reference surface for reprojection passes. `None` causes
    /// [`ReprojectToSurface`](super::passes::reproject::ReprojectToSurface)
    /// (once implemented) to error with `PassError::ReprojectionRequired`.
    pub target: Option<&'a dyn ReprojectionTarget>,
    /// Classifier used to populate per-vertex classes. Defaults provide
    /// v2-typical behaviour (45° dihedral, pin_boundary=true, pin_features=false).
    pub classifier: VertexClassifier,
    /// Optional caller-supplied creases / pinned vertices. Consulted before
    /// dihedral detection.
    pub features: Option<&'a FeatureSet>,
}

impl RepairContext<'static> {
    /// Builds a no-op static-lifetime context: zero-normal closure, no
    /// target, default classifier. Convenient for tests and for callers
    /// that plan to follow up with
    /// [`IsoMesh::generate_flat_normals`](crate::isosurface::IsoMesh::generate_flat_normals).
    pub fn noop() -> Self {
        // A non-capturing closure coerces to `fn(...)`, which can live in a
        // `static`; `&NF` then coerces to `&'static dyn Fn(...)`.
        static NF: fn(Vec3) -> Vec3 = |_p| Vec3::ZERO;
        Self {
            normal_fn: &NF,
            target: None,
            classifier: VertexClassifier::default(),
            features: None,
        }
    }
}

impl<'a> RepairContext<'a> {
    /// Canonical constructor. The returned context has no target and no
    /// feature set, and uses the default [`VertexClassifier`]. Chain
    /// [`with_target`](Self::with_target) / [`with_classifier`](Self::with_classifier)
    /// / [`with_features`](Self::with_features) to populate the rest.
    pub fn new(normal_fn: &'a dyn Fn(Vec3) -> Vec3) -> Self {
        Self {
            normal_fn,
            target: None,
            classifier: VertexClassifier::default(),
            features: None,
        }
    }

    /// Attaches a reprojection target.
    pub fn with_target(mut self, target: &'a dyn ReprojectionTarget) -> Self {
        self.target = Some(target);
        self
    }

    /// Replaces the classifier.
    pub fn with_classifier(mut self, classifier: VertexClassifier) -> Self {
        self.classifier = classifier;
        self
    }

    /// Attaches an explicit feature set.
    pub fn with_features(mut self, features: &'a FeatureSet) -> Self {
        self.features = Some(features);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_target_and_features_to_none() {
        let nf = |_p: Vec3| Vec3::Z;
        let ctx = RepairContext::new(&nf);
        assert!(ctx.target.is_none());
        assert!(ctx.features.is_none());
    }

    #[test]
    fn builder_chain_sets_fields() {
        let nf = |_p: Vec3| Vec3::Z;
        let features = FeatureSet::new();
        let ctx = RepairContext::new(&nf)
            .with_classifier(VertexClassifier {
                feature_dihedral_deg: 30.0,
                ..VertexClassifier::default()
            })
            .with_features(&features);
        assert_eq!(ctx.classifier.feature_dihedral_deg, 30.0);
        assert!(ctx.features.is_some());
    }
}
