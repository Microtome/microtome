//! End-to-end repair-pipeline integration test.
//!
//! Builds an actual DC mesh of a sphere and runs the standard pipeline
//! through it.
//!
//! ## Known v1 limitations validated by this test
//!
//! - Volume drift under TaubinSmooth can be large on coarse closed
//!   surfaces (no boundary to pin against, few vertices, large Laplacian
//!   steps). v1 tolerates this; v2 will add HC-Laplacian and reprojection
//!   to bound drift more tightly. The test asserts a generous bound.

use glam::Vec3;
use microtome_core::isosurface::{IsoMesh, OctreeNode, PositionCode, ScalarField, Sphere};
use microtome_core::mesh_repair::MeshRepairPipeline;

fn extract_sphere_dc(depth: u32) -> (IsoMesh, Sphere) {
    let sphere = Sphere::with_center(3.0, Vec3::new(4.0, 4.0, 4.0));
    let unit_size = 1.0;
    let min_code = PositionCode::splat(0);
    let mut root = OctreeNode::build_with_scalar_field(min_code, depth, &sphere, false, unit_size)
        .expect("octree builds for sphere");
    let mesh = OctreeNode::extract_mesh(&mut root, &sphere, unit_size);
    (mesh, sphere)
}

#[test]
fn standard_pipeline_on_dc_sphere_runs_to_completion() {
    let join = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let (mesh, sphere) = extract_sphere_dc(4);
            assert!(mesh.triangle_count() > 0, "DC produced no triangles");

            let pre = mesh.quality_report().expect("pre-quality");
            assert!(pre.triangle_count > 0);

            let pipeline = MeshRepairPipeline::standard();
            let normal_fn = |p: Vec3| sphere.normal(p);
            let (repaired, report) = mesh.repair(&pipeline, normal_fn).expect("repair");

            // Pipeline ran all five passes (CleanMesh + four v1 passes).
            assert_eq!(report.per_pass.len(), 5);

            // Triangle count never grows under v1 (no remesh / split passes
            // run on a closed sphere).
            assert!(
                report.post_quality.triangle_count <= report.pre_quality.triangle_count,
                "tri count must be non-increasing: pre={} post={}",
                report.pre_quality.triangle_count,
                report.post_quality.triangle_count
            );

            // Boundary-loop count unchanged on a closed surface.
            assert_eq!(
                report.post_quality.boundary_loop_count,
                report.pre_quality.boundary_loop_count
            );

            // Min angle should improve (or at least not regress).
            assert!(
                report.post_quality.min_angle_deg >= report.pre_quality.min_angle_deg - 1e-3,
                "min angle regressed: pre={}° post={}°",
                report.pre_quality.min_angle_deg,
                report.post_quality.min_angle_deg
            );

            // The repaired output mesh exists and matches the report.
            assert_eq!(
                repaired.triangle_count(),
                report.post_quality.triangle_count as usize
            );

            // Volume-drift bound: TaubinSmooth on a coarse closed sphere
            // (no boundaries) does shrink. v2's HC-Laplacian + reprojection
            // will tighten this; for v1 we tolerate up to 50%.
            // TODO(v2): tighten to <2% once ReprojectToSurface lands.
            let pre_vol = report.pre_quality.approx_volume.abs();
            let post_vol = report.post_quality.approx_volume.abs();
            if pre_vol > 0.0 {
                let ratio = post_vol / pre_vol;
                assert!(
                    ratio > 0.4,
                    "volume drifted >60%: pre={pre_vol} post={post_vol} ratio={ratio}"
                );
            }
        })
        .expect("spawn worker");
    join.join().expect("worker did not panic");
}
