//! Feature- and class-aware smoothing.
//!
//! Generalises [`TaubinSmooth`](super::taubin_smooth::TaubinSmooth) with two
//! choices of inner kernel:
//!
//! - [`FeatureSmoothMethod::HcLaplacian`] — Vollmer-Mencl-Müller 1999. Adds
//!   a "push back" step after the Laplacian to undo shrinkage. Better
//!   volume preservation than Taubin on closed surfaces; the natural choice
//!   for v2's standard pipeline.
//! - [`FeatureSmoothMethod::Bilateral`] — Fleishman-Drori-Cohen-Or 2003.
//!   Weights neighbours by spatial distance *and* normal similarity, so
//!   creases (sharp normal discontinuities) are not smoothed across.
//!   The displacement is normal-blocked for Interior vertices (volume-
//!   preserving tangential motion) and tangent-constrained for
//!   Boundary / Feature.
//!
//! Class handling:
//! - `Fixed`: skipped entirely (both kernels).
//! - HC kernel: Boundary / Feature skipped (v2 first cut).
//! - Bilateral kernel: Boundary / Feature tangent-constrained via
//!   `mesh_repair::tangent`, Interior smoothed tangentially in the
//!   surface-normal frame.

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{FaceId, HalfEdgeMesh, VertexId};
use super::super::pass::{MeshRepairPass, PassOutcome, PassStage};
use super::super::tangent::{boundary_tangent, feature_tangent, project_onto_tangent};
use super::super::vertex_class::VertexClass;
use crate::mesh_repair::RepairContext;

/// Smoothing kernel choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FeatureSmoothMethod {
    /// HC-Laplacian (Vollmer 1999): Laplacian + push-back step.
    HcLaplacian {
        /// Mix factor between the original and pre-iteration positions
        /// when computing the deformation `b`. `0.0` uses pre-iteration
        /// only (same as Taubin); `1.0` uses original only (more
        /// constraining). Default `0.5`.
        alpha: f32,
        /// Mix factor between own deformation and neighbour-mean
        /// deformation in the push-back. Default `0.7`.
        beta: f32,
    },
    /// Bilateral filter (Fleishman 2003).
    Bilateral {
        /// Spatial-distance kernel scale (positive). Weight contribution
        /// `exp(-|u-v|² / 2σ_s²)`.
        sigma_spatial: f32,
        /// Normal-similarity kernel scale (positive). Weight contribution
        /// `exp(-(1 - n_v·n_u)² / 2σ_n²)`.
        sigma_normal: f32,
    },
}

impl Default for FeatureSmoothMethod {
    fn default() -> Self {
        Self::HcLaplacian {
            alpha: 0.5,
            beta: 0.7,
        }
    }
}

/// Class-aware smoothing pass.
#[derive(Debug, Clone)]
pub struct FeatureSmooth {
    /// Number of smoothing iterations.
    pub iterations: u32,
    /// Inner kernel selection.
    pub method: FeatureSmoothMethod,
}

impl Default for FeatureSmooth {
    fn default() -> Self {
        Self {
            iterations: 2,
            method: FeatureSmoothMethod::default(),
        }
    }
}

impl MeshRepairPass for FeatureSmooth {
    fn name(&self) -> &'static str {
        "feature_smooth"
    }

    fn stage(&self) -> PassStage {
        PassStage::HalfEdge
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        let mut outcome = PassOutcome::noop(self.name());

        match self.method {
            FeatureSmoothMethod::HcLaplacian { alpha, beta } => {
                if !(0.0 < alpha && alpha < 1.0) {
                    return Err(PassError::InvalidConfig(format!(
                        "feature_smooth HC alpha must be in (0, 1); got {alpha}"
                    )));
                }
                if !(0.0 < beta && beta < 1.0) {
                    return Err(PassError::InvalidConfig(format!(
                        "feature_smooth HC beta must be in (0, 1); got {beta}"
                    )));
                }
                hc_laplacian_iterations(mesh, self.iterations, alpha, beta, &mut outcome);
            }
            FeatureSmoothMethod::Bilateral {
                sigma_spatial,
                sigma_normal,
            } => {
                if sigma_spatial <= 0.0 || sigma_spatial.is_nan() {
                    return Err(PassError::InvalidConfig(format!(
                        "feature_smooth Bilateral sigma_spatial must be > 0; got {sigma_spatial}"
                    )));
                }
                if sigma_normal <= 0.0 || sigma_normal.is_nan() {
                    return Err(PassError::InvalidConfig(format!(
                        "feature_smooth Bilateral sigma_normal must be > 0; got {sigma_normal}"
                    )));
                }
                bilateral_iterations(
                    mesh,
                    self.iterations,
                    sigma_spatial,
                    sigma_normal,
                    ctx.classifier.feature_dihedral_deg,
                    &mut outcome,
                );
            }
        }

        Ok(outcome)
    }
}

fn hc_laplacian_iterations(
    mesh: &mut HalfEdgeMesh,
    iterations: u32,
    alpha: f32,
    beta: f32,
    outcome: &mut PassOutcome,
) {
    let n_verts = mesh.vertices.len();
    let original: Vec<Vec3> = mesh.vertices.iter().map(|v| v.pos).collect();

    for _ in 0..iterations {
        let pre_iter: Vec<Vec3> = mesh.vertices.iter().map(|v| v.pos).collect();
        let mut p: Vec<Vec3> = pre_iter.clone();

        // Laplacian step (Interior vertices only).
        for (vi, p_slot) in p.iter_mut().enumerate().take(n_verts) {
            let vid = VertexId(vi as u32);
            if !mesh.vertex_is_live(vid) {
                continue;
            }
            if mesh.vertex_class(vid) != VertexClass::Interior {
                continue;
            }
            let (sum, count) = sum_one_ring(mesh, vid, &pre_iter);
            if count > 0 {
                *p_slot = sum / count as f32;
            }
        }

        // Compute deformation b for *every* live vertex (not just Interior;
        // boundary/feature vertex deformations factor into Interior neighbours'
        // push-back).
        let mut b: Vec<Vec3> = vec![Vec3::ZERO; n_verts];
        for (vi, b_slot) in b.iter_mut().enumerate().take(n_verts) {
            let vid = VertexId(vi as u32);
            if !mesh.vertex_is_live(vid) {
                continue;
            }
            *b_slot = p[vi] - (alpha * original[vi] + (1.0 - alpha) * pre_iter[vi]);
        }

        // HC push-back (Interior only).
        for vi in 0..n_verts {
            let vid = VertexId(vi as u32);
            if !mesh.vertex_is_live(vid) {
                continue;
            }
            if mesh.vertex_class(vid) != VertexClass::Interior {
                continue;
            }
            let (sum_b, count) = sum_one_ring(mesh, vid, &b);
            if count == 0 {
                continue;
            }
            let mean_b = sum_b / count as f32;
            mesh.vertices[vi].pos = p[vi] - (beta * b[vi] + (1.0 - beta) * mean_b);
            outcome.stats.vertices_smoothed = outcome.stats.vertices_smoothed.saturating_add(1);
        }
    }
}

fn sum_one_ring(mesh: &HalfEdgeMesh, v: VertexId, source: &[Vec3]) -> (Vec3, u32) {
    let mut sum = Vec3::ZERO;
    let mut count = 0u32;
    for n in mesh.vertex_one_ring(v) {
        sum += source[n.index()];
        count += 1;
    }
    (sum, count)
}

/// Bilateral smoothing: weights one-ring contributions by both spatial
/// distance and normal similarity, then applies the displacement with the
/// surface-normal component blocked (Interior) or tangent-projected
/// (Boundary / Feature). Matches Fleishman 2003 in spirit, simplified to
/// position-domain bilateral on the one-ring.
fn bilateral_iterations(
    mesh: &mut HalfEdgeMesh,
    iterations: u32,
    sigma_spatial: f32,
    sigma_normal: f32,
    dihedral_threshold_deg: f32,
    outcome: &mut PassOutcome,
) {
    let n_verts = mesh.vertices.len();
    let two_sigma_s_sq = 2.0 * sigma_spatial * sigma_spatial;
    let two_sigma_n_sq = 2.0 * sigma_normal * sigma_normal;

    for _ in 0..iterations {
        let positions: Vec<Vec3> = mesh.vertices.iter().map(|v| v.pos).collect();
        let normals = vertex_normals_area_weighted(mesh, &positions);

        for vi in 0..n_verts {
            let vid = VertexId(vi as u32);
            if !mesh.vertex_is_live(vid) {
                continue;
            }
            let class = mesh.vertex_class(vid);
            if class == VertexClass::Fixed {
                continue;
            }
            let pos_v = positions[vi];
            let n_v = normals[vi];

            let mut sum_pos = Vec3::ZERO;
            let mut sum_w = 0.0_f32;
            for u in mesh.vertex_one_ring(vid) {
                let pos_u = positions[u.index()];
                let n_u = normals[u.index()];
                let dist_sq = (pos_u - pos_v).length_squared();
                let normal_diff = 1.0 - n_v.dot(n_u);
                let w = (-dist_sq / two_sigma_s_sq).exp()
                    * (-(normal_diff * normal_diff) / two_sigma_n_sq).exp();
                sum_pos += w * pos_u;
                sum_w += w;
            }
            if sum_w <= 0.0 {
                continue;
            }
            let target = sum_pos / sum_w;
            let raw_disp = target - pos_v;

            let constrained = match class {
                VertexClass::Fixed => continue,
                VertexClass::Boundary => match boundary_tangent(mesh, vid) {
                    Some(t) => project_onto_tangent(raw_disp, t),
                    None => continue,
                },
                VertexClass::Feature => match feature_tangent(mesh, vid, dihedral_threshold_deg) {
                    Some(t) => project_onto_tangent(raw_disp, t),
                    None => continue,
                },
                VertexClass::Interior => {
                    // Block the surface-normal component → tangential motion only.
                    if n_v == Vec3::ZERO {
                        raw_disp
                    } else {
                        raw_disp - n_v * raw_disp.dot(n_v)
                    }
                }
            };
            mesh.vertices[vi].pos = pos_v + constrained;
            outcome.stats.vertices_smoothed = outcome.stats.vertices_smoothed.saturating_add(1);
        }
    }
}

/// Per-vertex normal estimate: area-weighted sum of incident face normals,
/// then unit-normalised. `positions` is a snapshot taken before this
/// iteration so the caller can mutate `mesh.vertices` mid-loop without
/// invalidating normals.
fn vertex_normals_area_weighted(mesh: &HalfEdgeMesh, positions: &[Vec3]) -> Vec<Vec3> {
    let mut normals = vec![Vec3::ZERO; positions.len()];
    for fi in 0..mesh.faces.len() {
        if mesh.faces[fi].removed {
            continue;
        }
        let fid = FaceId(fi as u32);
        let [a, b, c] = mesh.face_triangle(fid);
        let p0 = positions[a.index()];
        let p1 = positions[b.index()];
        let p2 = positions[c.index()];
        // (p1-p0) × (p2-p0) has length 2× triangle area, direction = face normal.
        let weighted = (p1 - p0).cross(p2 - p0);
        normals[a.index()] += weighted;
        normals[b.index()] += weighted;
        normals[c.index()] += weighted;
    }
    for n in normals.iter_mut() {
        *n = n.normalize_or_zero();
    }
    normals
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::IsoMesh;

    fn iso(positions: Vec<Vec3>, indices: Vec<u32>) -> IsoMesh {
        let n = positions.len();
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; n],
            indices,
        }
    }

    /// Octahedron with one vertex perturbed inward.
    fn octahedron_perturbed() -> HalfEdgeMesh {
        let positions = vec![
            Vec3::new(1.5, 0.0, 0.0),
            Vec3::new(-1.5, 0.0, 0.0),
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(0.0, -1.5, 0.0),
            Vec3::new(0.0, 0.0, 0.5), // perturbed inward (should be 1.5)
            Vec3::new(0.0, 0.0, -1.5),
        ];
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 4,  2, 1, 4,  1, 3, 4,  3, 0, 4,
            2, 0, 5,  1, 2, 5,  3, 1, 5,  0, 3, 5,
        ];
        HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("octa")
    }

    #[test]
    fn hc_smooth_moves_perturbed_vertex_toward_one_ring_centroid() {
        // Vertex 4 is perturbed inward at z=0.5; its 4 equatorial neighbours
        // are at z=0. Laplacian smoothing pulls vertex 4 toward z=0; HC
        // partially pushes back toward the original z=0.5. Net: post.z is
        // strictly between 0 and 0.5.
        let mut mesh = octahedron_perturbed();
        let pre = mesh.vertex_position(VertexId(4));
        let pass = FeatureSmooth::default();
        let ctx = RepairContext::noop();
        pass.apply(&mut mesh, &ctx).expect("smooth");
        let post = mesh.vertex_position(VertexId(4));
        assert!(
            post.z < pre.z && post.z > 0.0,
            "vertex 4 should sink: pre={pre:?} post={post:?}"
        );
    }

    #[test]
    fn hc_smooth_rejects_invalid_alpha() {
        let mut mesh = octahedron_perturbed();
        let pass = FeatureSmooth {
            method: FeatureSmoothMethod::HcLaplacian {
                alpha: -0.1,
                beta: 0.5,
            },
            ..FeatureSmooth::default()
        };
        let ctx = RepairContext::noop();
        assert!(pass.apply(&mut mesh, &ctx).is_err());
    }

    #[test]
    fn hc_smooth_rejects_invalid_beta() {
        let mut mesh = octahedron_perturbed();
        let pass = FeatureSmooth {
            method: FeatureSmoothMethod::HcLaplacian {
                alpha: 0.5,
                beta: 1.5,
            },
            ..FeatureSmooth::default()
        };
        let ctx = RepairContext::noop();
        assert!(pass.apply(&mut mesh, &ctx).is_err());
    }

    #[test]
    fn bilateral_rejects_non_positive_sigma() {
        let mut mesh = octahedron_perturbed();
        let pass = FeatureSmooth {
            method: FeatureSmoothMethod::Bilateral {
                sigma_spatial: 0.0,
                sigma_normal: 0.5,
            },
            ..FeatureSmooth::default()
        };
        let ctx = RepairContext::noop();
        assert!(matches!(
            pass.apply(&mut mesh, &ctx),
            Err(PassError::InvalidConfig(_))
        ));
        let pass2 = FeatureSmooth {
            method: FeatureSmoothMethod::Bilateral {
                sigma_spatial: 1.0,
                sigma_normal: -0.1,
            },
            ..FeatureSmooth::default()
        };
        assert!(matches!(
            pass2.apply(&mut mesh, &ctx),
            Err(PassError::InvalidConfig(_))
        ));
    }

    #[test]
    fn bilateral_smooths_tangential_perturbation_on_flat_patch() {
        // Flat 5-vertex patch in z=0; vertex 0 at center perturbed in +x
        // by 0.3. Vertex 0 is interior, surrounded by 4 boundary
        // neighbours. Bilateral target ≈ centroid of 4 outer = (0,0,0);
        // displacement is purely tangential (z-component zero), so
        // normal-block leaves it intact and the vertex moves toward the
        // origin.
        let positions = vec![
            Vec3::new(0.3, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        ];
        let indices = vec![0, 1, 3, 0, 3, 2, 0, 2, 4, 0, 4, 1];
        let iso_mesh = IsoMesh {
            positions,
            normals: vec![Vec3::Z; 5],
            indices,
        };
        let mut mesh = HalfEdgeMesh::from_iso_mesh(&iso_mesh).expect("patch");
        // Pin nothing, let outer ring stay Boundary so vertex 0 stays Interior.
        let classifier = crate::mesh_repair::vertex_class::VertexClassifier {
            pin_boundary: false,
            ..crate::mesh_repair::vertex_class::VertexClassifier::default()
        };
        classifier.classify(&mut mesh);
        assert_eq!(mesh.vertex_class(VertexId(0)), VertexClass::Interior);
        let pre = mesh.vertex_position(VertexId(0));
        let pass = FeatureSmooth {
            iterations: 3,
            method: FeatureSmoothMethod::Bilateral {
                sigma_spatial: 1.5,
                sigma_normal: 0.5,
            },
        };
        let nf = |_p: Vec3| Vec3::Z;
        let ctx = RepairContext::new(&nf).with_classifier(classifier);
        pass.apply(&mut mesh, &ctx).expect("smooth");
        let post = mesh.vertex_position(VertexId(0));
        assert!(
            post.x.abs() < pre.x.abs() * 0.7,
            "tangential perturbation reduced: pre={pre:?} post={post:?}"
        );
        assert!(post.z.abs() < 1e-3, "should stay in z=0 plane");
    }

    #[test]
    fn bilateral_preserves_normal_direction_perturbation_on_apex() {
        // Octahedron with vertex 4 perturbed inward along its normal.
        // Bilateral with normal-blocking should leave the z position
        // approximately unchanged (Fleishman: noise along normal is the
        // structure we explicitly preserve).
        let mut mesh = octahedron_perturbed();
        let pre = mesh.vertex_position(VertexId(4));
        let pass = FeatureSmooth {
            iterations: 2,
            method: FeatureSmoothMethod::Bilateral {
                sigma_spatial: 2.0,
                sigma_normal: 0.5,
            },
        };
        let ctx = RepairContext::noop();
        pass.apply(&mut mesh, &ctx).expect("smooth");
        let post = mesh.vertex_position(VertexId(4));
        assert!(
            (post.z - pre.z).abs() < 0.05,
            "normal-direction perturbation preserved: pre={pre:?} post={post:?}"
        );
    }

    #[test]
    fn bilateral_skips_fixed_vertices() {
        let mut mesh = octahedron_perturbed();
        mesh.set_vertex_class(VertexId(4), VertexClass::Fixed);
        let pre = mesh.vertex_position(VertexId(4));
        let pass = FeatureSmooth {
            iterations: 2,
            method: FeatureSmoothMethod::Bilateral {
                sigma_spatial: 1.0,
                sigma_normal: 0.5,
            },
        };
        let ctx = RepairContext::noop();
        pass.apply(&mut mesh, &ctx).expect("smooth");
        let post = mesh.vertex_position(VertexId(4));
        assert_eq!(pre, post);
    }

    #[test]
    fn hc_smooth_skips_fixed_vertices() {
        let mut mesh = octahedron_perturbed();
        mesh.set_vertex_class(VertexId(4), VertexClass::Fixed);
        let pre = mesh.vertex_position(VertexId(4));
        let pass = FeatureSmooth::default();
        let ctx = RepairContext::noop();
        pass.apply(&mut mesh, &ctx).expect("smooth");
        let post = mesh.vertex_position(VertexId(4));
        assert_eq!(pre, post);
    }
}
