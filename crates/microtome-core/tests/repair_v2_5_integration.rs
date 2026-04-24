//! v2.5 end-to-end integration tests.
//!
//! Reference-mesh tests (gear_rail feature preservation, bunny isotropic
//! remesh) were deferred — they need scan-conversion + DC at depths that
//! take many seconds in unit-test time, and the value relative to the
//! synthetic-sphere coverage here is low. SimplifyQuadric on raw DC
//! output is also deferred: link-condition rejection is dense enough
//! on cell-aligned soup that the pass needs a richer cleaning chain
//! than this slice exercises (a finer-grained welding + a feature-
//! aware re-classifier between weld and simplify). The pass itself is
//! covered by per-pass unit tests, including the new volume-tolerance
//! pre-check on the thin pyramid.
//!
//! What this file does cover end-to-end on a coarse DC sphere:
//!
//! - The Bilateral kernel as a drop-in for FeatureSmooth in a chain
//!   that ends in tangent-constrained reprojection.

use glam::Vec3;
use microtome_core::isosurface::{IsoMesh, OctreeNode, PositionCode, ScalarField, Sphere};
use microtome_core::mesh_repair::passes::{
    CleanMesh, FeatureSmooth, FeatureSmoothMethod, FillSmallHoles, ReprojectToSurface, WeldVertices,
};
use microtome_core::mesh_repair::{MeshRepairPipeline, RepairContext, ScalarFieldTarget};

fn extract_sphere(depth: u32) -> (IsoMesh, Sphere) {
    let sphere = Sphere::with_center(3.0, Vec3::new(4.0, 4.0, 4.0));
    let unit_size = 1.0;
    let min_code = PositionCode::splat(0);
    let mut root = OctreeNode::build_with_scalar_field(min_code, depth, &sphere, false, unit_size)
        .expect("octree builds");
    let mesh = OctreeNode::extract_mesh(&mut root, &sphere, unit_size);
    (mesh, sphere)
}

#[test]
fn bilateral_kernel_v2_chain_runs_to_completion_on_dc_sphere() {
    let join = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let (mesh, sphere) = extract_sphere(4);
            assert!(mesh.triangle_count() > 0);

            let target = ScalarFieldTarget::new(&sphere);
            let nf = |p: Vec3| sphere.normal(p);
            let ctx = RepairContext::new(&nf).with_target(&target);

            // Reproduce standard_v2 but swap the HC kernel for Bilateral.
            let mut pipeline = MeshRepairPipeline::new();
            pipeline.add(CleanMesh::default());
            pipeline.add(WeldVertices::default());
            pipeline.add(FillSmallHoles::default());
            pipeline.add(FeatureSmooth {
                iterations: 1,
                method: FeatureSmoothMethod::Bilateral {
                    sigma_spatial: 0.8,
                    sigma_normal: 0.5,
                },
            });
            pipeline.add(ReprojectToSurface::default());

            let (out, report) = pipeline.run_with(&mesh, &ctx).expect("run");
            assert_eq!(report.per_pass.len(), 5);
            assert_eq!(report.per_pass[3].name, "feature_smooth");
            assert_eq!(report.per_pass[4].name, "reproject_to_surface");
            assert!(out.triangle_count() > 0);
        })
        .expect("spawn worker");
    join.join().expect("worker did not panic");
}
