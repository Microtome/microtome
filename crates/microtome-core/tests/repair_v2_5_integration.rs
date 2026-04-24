//! v2.5 end-to-end integration tests.
//!
//! Reference-mesh tests (gear_rail feature preservation, bunny isotropic
//! remesh) were deferred — they need scan-conversion + DC at depths that
//! take many seconds in unit-test time.

use glam::Vec3;
use microtome_core::isosurface::{IsoMesh, OctreeNode, PositionCode, ScalarField, Sphere};
#[allow(unused_imports)]
use microtome_core::mesh_repair::half_edge::HalfEdgeId;
use microtome_core::mesh_repair::half_edge::{HalfEdgeMesh, VertexId};
use microtome_core::mesh_repair::passes::{
    CleanMesh, FeatureSmooth, FeatureSmoothMethod, FillSmallHoles, ReprojectToSurface,
    SimplifyQuadric, WeldVertices,
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
fn simplify_then_reproject_keeps_vertices_near_sphere() {
    let join = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let (mesh, sphere) = extract_sphere(4);
            let pre_count = mesh.triangle_count();
            assert!(pre_count > 0);

            let target = ScalarFieldTarget::new(&sphere);
            let nf = |p: Vec3| sphere.normal(p);
            let ctx = RepairContext::new(&nf).with_target(&target);

            // WeldVertices first to canonicalise the vertex set, then
            // CleanMesh (with topological winding propagation + non-
            // manifold-face drop) gives the half-edge mesh a properly
            // closed 2-manifold to operate on. Without this prefix, DC's
            // inconsistent winding leaves phantom-boundary edges that
            // drive the link-condition rejection rate to 100 %.
            let half = (pre_count as u32 / 2).max(1);
            let mut pipeline = MeshRepairPipeline::new();
            pipeline.add(WeldVertices::default());
            pipeline.add(CleanMesh {
                resolve_t_junctions: false,
                ..CleanMesh::default()
            });
            pipeline.add(SimplifyQuadric {
                target_triangle_count: Some(half),
                volume_tolerance: 0.0,
                forbid_normal_flip: false,
                ..SimplifyQuadric::default()
            });
            pipeline.add(ReprojectToSurface::default());

            let (out, _report) = pipeline.run_with(&mesh, &ctx).expect("run");

            // Simplification reduces triangle count.
            assert!(
                out.triangle_count() < pre_count,
                "simplify should reduce face count: pre={pre_count} post={}",
                out.triangle_count()
            );

            // Vertices remain close to the sphere surface after reproject.
            let centre = Vec3::new(4.0, 4.0, 4.0);
            let mut max_dist = 0.0f32;
            for p in &out.positions {
                let d = ((*p - centre).length() - 3.0).abs();
                if d > max_dist {
                    max_dist = d;
                }
            }
            assert!(
                max_dist < 0.6,
                "max vertex-to-sphere distance after simplify + reproject: {max_dist}"
            );
        })
        .expect("spawn worker");
    join.join().expect("worker did not panic");
}

#[test]
#[ignore = "diagnostic helper for v3 link-condition investigation; run with --run-ignored ignored-only diagnose_link_condition_on_dc_sphere -- --nocapture"]
fn diagnose_link_condition_on_dc_sphere() {
    use microtome_core::mesh_repair::pass::MeshRepairPass;
    let join = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            let (iso, _sphere) = extract_sphere(4);
            let weld = WeldVertices::default();
            let ctx = RepairContext::noop();
            let (welded, _) = weld.pre_construction(iso, &ctx).expect("weld");
            eprintln!(
                "post-weld: positions={} indices={} faces={}",
                welded.positions.len(),
                welded.indices.len(),
                welded.indices.len() / 3
            );

            let mesh = HalfEdgeMesh::from_iso_mesh(&welded).expect("half-edge build");
            eprintln!(
                "half-edge mesh: V={} F={} HE={} manifold={} non_manifold_edges={}",
                mesh.vertex_count(),
                mesh.face_count(),
                mesh.half_edge_count(),
                mesh.is_manifold(),
                mesh.count_non_manifold_edges(),
            );

            // For the first ~10 edges, dump link-condition data.
            let mut samples = 0;
            for h in mesh.edge_iter() {
                if samples >= 10 {
                    break;
                }
                samples += 1;
                let twin = mesh.he_twin(h);
                let u = mesh.he_tail(h);
                let v = mesh.he_head(h);
                let u_ring: Vec<u32> = mesh.vertex_one_ring(u).map(|x| x.0).collect();
                let v_ring: Vec<u32> = mesh.vertex_one_ring(v).map(|x| x.0).collect();

                let mut u_sorted = u_ring.clone();
                u_sorted.sort_unstable();
                let u_dup = u_sorted.windows(2).any(|w| w[0] == w[1]);
                let mut v_sorted = v_ring.clone();
                v_sorted.sort_unstable();
                let v_dup = v_sorted.windows(2).any(|w| w[0] == w[1]);

                let u_set: std::collections::HashSet<u32> = u_ring.iter().copied().collect();
                let v_set: std::collections::HashSet<u32> = v_ring.iter().copied().collect();
                let shared: Vec<u32> = u_set.intersection(&v_set).copied().collect();

                let apex_l = mesh.he_head(mesh.he_next(h)).0;
                let apex_r = if twin.is_valid() {
                    Some(mesh.he_head(mesh.he_next(twin)).0)
                } else {
                    None
                };
                let mut allowed: Vec<u32> = vec![apex_l];
                if let Some(b) = apex_r {
                    allowed.push(b);
                }
                allowed.sort_unstable();

                let mut shared_sorted = shared.clone();
                shared_sorted.sort_unstable();
                let link_ok = shared_sorted == allowed;

                eprintln!(
                    "edge[{samples}] u={} v={} |u_ring|={} (dup={u_dup}) |v_ring|={} (dup={v_dup}) |shared|={} shared={shared_sorted:?} expected={allowed:?} ok={link_ok}",
                    u.0,
                    v.0,
                    u_ring.len(),
                    v_ring.len(),
                    shared.len(),
                );
                if !link_ok {
                    eprintln!("    u_ring full: {u_ring:?}");
                    eprintln!("    v_ring full: {v_ring:?}");
                }
            }

            // Step-by-step trace of vertex 4's walk: print each visited HE
            // with its tail/head/next/twin so we can spot the divergence.
            eprintln!("\n=== Detailed walk for vertex 4 ===");
            let v4 = VertexId(4);
            let start = mesh.vertex_he_out(v4);
            eprintln!("vertex 4 he_out = {:?}", start);
            let mut h = start;
            for step in 0..20 {
                let tail = mesh.he_tail(h);
                let head = mesh.he_head(h);
                let next_h = mesh.he_next(h);
                let prev_h = mesh.he_prev(h);
                let twin_h = mesh.he_twin(h);
                let prev_tail = mesh.he_tail(prev_h);
                let prev_head = mesh.he_head(prev_h);
                let prev_twin = mesh.he_twin(prev_h);
                eprintln!(
                    "step[{step}] h={:?} tail={} head={} next={:?} prev={:?} twin={:?} prev=({prev_tail:?}→{prev_head:?}) prev_twin={:?}",
                    h, tail.0, head.0, next_h, prev_h, twin_h, prev_twin,
                );
                if !prev_twin.is_valid() {
                    eprintln!("  (boundary; would terminate)");
                    break;
                }
                if step > 0 && prev_twin == start {
                    eprintln!("  (cycle back to start; would terminate)");
                    break;
                }
                h = prev_twin;
            }
        })
        .expect("spawn worker");
    join.join().expect("worker did not panic");
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
            pipeline.add(WeldVertices::default());
            pipeline.add(CleanMesh::default());
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
