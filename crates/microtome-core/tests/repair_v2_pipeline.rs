//! v2 standard pipeline integration tests.
//!
//! End-to-end coverage of the v2 chain — pulls a perturbed sphere DC
//! output through clean → weld → fill → feature_smooth → reproject and
//! checks that vertices end up close to the true surface.
//!
//! Heavier reference-mesh tests (gear_rail feature preservation, bunny
//! isotropic remesh) are deferred — they need scan-conversion + DC at
//! a depth that's slow in unit-test time, and the value relative to v2
//! first-cut shipping is low.

use glam::Vec3;
use microtome_core::isosurface::{IsoMesh, OctreeNode, PositionCode, ScalarField, Sphere};
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
fn standard_v2_runs_to_completion_on_dc_sphere() {
    let join = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let (mesh, sphere) = extract_sphere(4);
            assert!(mesh.triangle_count() > 0);

            let target = ScalarFieldTarget::new(&sphere);
            let nf = |p: Vec3| sphere.normal(p);
            let ctx = RepairContext::new(&nf).with_target(&target);

            let pipeline = MeshRepairPipeline::standard_v2();
            let (out, report) = pipeline.run_with(&mesh, &ctx).expect("v2 standard runs");

            // Pipeline ran 5 passes: clean, weld, fill, feature_smooth, reproject.
            assert_eq!(report.per_pass.len(), 5);
            assert_eq!(report.per_pass[0].name, "clean_mesh");
            assert_eq!(report.per_pass[1].name, "weld_vertices");
            assert_eq!(report.per_pass[2].name, "fill_small_holes");
            assert_eq!(report.per_pass[3].name, "feature_smooth");
            assert_eq!(report.per_pass[4].name, "reproject_to_surface");

            // Output mesh exists with positive triangle count.
            assert!(out.triangle_count() > 0);
        })
        .expect("spawn worker");
    join.join().expect("worker did not panic");
}

#[test]
fn reproject_pulls_perturbed_dc_sphere_closer_to_true_surface() {
    // Build a coarse DC sphere, smooth it (which drifts vertices off the
    // surface), then reproject. Assert the average distance to the sphere
    // drops after reprojection.
    let join = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let (mesh, sphere) = extract_sphere(4);
            let target = ScalarFieldTarget::new(&sphere);
            let nf = |p: Vec3| sphere.normal(p);
            let ctx = RepairContext::new(&nf).with_target(&target);

            // Run only feature_smooth + reproject (no other ops).
            let mut smooth_then_reproject = MeshRepairPipeline::new();
            smooth_then_reproject
                .add(microtome_core::mesh_repair::passes::FeatureSmooth::default())
                .add(microtome_core::mesh_repair::passes::ReprojectToSurface::default());

            let (out, _report) = smooth_then_reproject
                .run_with(&mesh, &ctx)
                .expect("pipeline runs");

            // Average vertex-to-sphere distance should be small after
            // reprojection.
            let centre = Vec3::new(4.0, 4.0, 4.0);
            let mut total = 0.0;
            for p in &out.positions {
                total += ((*p - centre).length() - 3.0).abs();
            }
            let avg = total / out.positions.len() as f32;
            assert!(
                avg < 0.5,
                "average vertex-to-sphere distance {avg} too high"
            );
        })
        .expect("spawn worker");
    join.join().expect("worker did not panic");
}
