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
//!   Reserved for v2.5; today this variant returns
//!   `PassError::InvalidConfig` when selected.
//!
//! Class handling (v2 first cut):
//! - `Fixed`, `Boundary`, `Feature` are all skipped.
//! - Tangent-constrained smoothing for `Boundary` / `Feature` lands with
//!   v2.5 alongside the bilateral kernel.

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{HalfEdgeMesh, VertexId};
use super::super::pass::{MeshRepairPass, PassOutcome};
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
    /// Bilateral filter (Fleishman 2003). Not implemented in v2 first cut.
    Bilateral {
        /// Spatial-distance kernel scale.
        sigma_spatial: f32,
        /// Normal-similarity kernel scale.
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

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        _ctx: &RepairContext<'_>,
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
            FeatureSmoothMethod::Bilateral { .. } => {
                return Err(PassError::InvalidConfig(
                    "feature_smooth Bilateral is reserved for v2.5; not implemented yet".into(),
                ));
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
    fn bilateral_returns_invalid_config_for_now() {
        let mut mesh = octahedron_perturbed();
        let pass = FeatureSmooth {
            method: FeatureSmoothMethod::Bilateral {
                sigma_spatial: 1.0,
                sigma_normal: 0.5,
            },
            ..FeatureSmooth::default()
        };
        let ctx = RepairContext::noop();
        assert!(matches!(
            pass.apply(&mut mesh, &ctx),
            Err(PassError::InvalidConfig(_))
        ));
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
