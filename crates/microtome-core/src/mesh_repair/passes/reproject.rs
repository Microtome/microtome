//! Reprojection: snap repaired vertices back onto a reference surface.
//!
//! After smoothing or simplification, vertices drift off the true
//! isosurface. This pass projects each candidate vertex back via
//! `ctx.target`. Vertices classified `Fixed` are always skipped; class
//! `Feature` / `Boundary` are skipped by default (need tangent projection
//! to slide along the crease/boundary rather than falling off — v2.5
//! will add that). Set `project_features` / `project_boundary` to opt
//! them in with the simple unconstrained projection.

use super::super::error::PassError;
use super::super::half_edge::{HalfEdgeMesh, VertexId};
use super::super::pass::{MeshRepairPass, PassOutcome, PassWarningKind};
use super::super::vertex_class::VertexClass;
use crate::mesh_repair::RepairContext;

/// Snaps vertex positions back to the surface defined by `ctx.target`.
#[derive(Debug, Clone)]
pub struct ReprojectToSurface {
    /// Maximum allowed displacement, expressed as a multiple of the median
    /// edge length. Projections moving further than this are rejected and
    /// the vertex stays where it is (Clamped warning).
    pub max_distance: f32,
    /// Number of reprojection rounds. Each round walks every candidate
    /// vertex once.
    pub iterations: u32,
    /// If `true`, also reproject `Feature` vertices. v2 first-cut: simple
    /// unconstrained projection (no tangent slide); fine for nearly-flat
    /// crease neighbourhoods, less ideal for sharp corners.
    pub project_features: bool,
    /// If `true`, also reproject `Boundary` vertices.
    pub project_boundary: bool,
}

impl Default for ReprojectToSurface {
    fn default() -> Self {
        Self {
            max_distance: 1.5,
            iterations: 1,
            project_features: false,
            project_boundary: false,
        }
    }
}

impl MeshRepairPass for ReprojectToSurface {
    fn name(&self) -> &'static str {
        "reproject_to_surface"
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        let target = ctx.target.ok_or_else(|| {
            PassError::PreConstruction("reproject_to_surface requires a ReprojectionTarget".into())
        })?;

        let mut outcome = PassOutcome::noop(self.name());
        let median = median_edge_length(mesh);
        let max_disp = self.max_distance * median;

        for _ in 0..self.iterations {
            let count = mesh.vertices.len();
            for vi in 0..count {
                if mesh.vertices[vi].removed {
                    continue;
                }
                let vid = VertexId(vi as u32);
                let class = mesh.vertex_class(vid);
                let allowed = match class {
                    VertexClass::Fixed => false,
                    VertexClass::Boundary => self.project_boundary,
                    VertexClass::Feature => self.project_features,
                    VertexClass::Interior => true,
                };
                if !allowed {
                    continue;
                }
                let pos = mesh.vertices[vi].pos;
                let projection = target.project(pos, None);
                let Some(result) = projection else {
                    continue;
                };
                if max_disp > 0.0 && result.distance > max_disp {
                    outcome.warn(
                        PassWarningKind::Clamped,
                        format!(
                            "vertex {vi} reprojection distance {} exceeds max {}",
                            result.distance, max_disp
                        ),
                    );
                    continue;
                }
                mesh.vertices[vi].pos = result.position;
                outcome.stats.vertices_smoothed += 1;
            }
        }

        Ok(outcome)
    }
}

fn median_edge_length(mesh: &HalfEdgeMesh) -> f32 {
    let mut lengths: Vec<f32> = mesh.edge_iter().map(|h| mesh.edge_length(h)).collect();
    if lengths.is_empty() {
        return 0.0;
    }
    lengths.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    lengths[lengths.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::{IsoMesh, Sphere};
    use crate::mesh_repair::reprojection::ScalarFieldTarget;
    use glam::Vec3;

    fn perturbed_sphere() -> (HalfEdgeMesh, Sphere) {
        // Build a unit sphere as a tetrahedron-like shape with vertices on
        // the sphere, then perturb each vertex outward along its radial
        // direction by 0.5. ReprojectToSurface should pull them back.
        let positions = vec![
            Vec3::new(1.5, 0.0, 0.0),
            Vec3::new(-1.5, 0.0, 0.0),
            Vec3::new(0.0, 1.5, 0.0),
            Vec3::new(0.0, -1.5, 0.0),
            Vec3::new(0.0, 0.0, 1.5),
            Vec3::new(0.0, 0.0, -1.5),
        ];
        // Octahedron: 8 triangular faces, all CCW outward.
        #[rustfmt::skip]
        let indices = vec![
            0, 2, 4,  2, 1, 4,  1, 3, 4,  3, 0, 4,
            2, 0, 5,  1, 2, 5,  3, 1, 5,  0, 3, 5,
        ];
        let iso = IsoMesh {
            positions,
            normals: vec![Vec3::Z; 6],
            indices,
        };
        let mesh = HalfEdgeMesh::from_iso_mesh(&iso).expect("octahedron builds");
        let sphere = Sphere::with_center(1.0, Vec3::ZERO);
        (mesh, sphere)
    }

    #[test]
    fn reproject_pulls_perturbed_vertices_back_to_sphere() {
        let (mut mesh, sphere) = perturbed_sphere();
        let target = ScalarFieldTarget::new(&sphere);
        let nf = |_p: Vec3| Vec3::Z;
        let ctx = RepairContext::new(&nf).with_target(&target);
        let pass = ReprojectToSurface {
            max_distance: 100.0, // permissive for the test
            ..ReprojectToSurface::default()
        };

        // Pre: each vertex at radius 1.5.
        for vi in 0u32..6 {
            let r = mesh.vertex_position(VertexId(vi)).length();
            assert!((r - 1.5).abs() < 1e-3);
        }

        pass.apply(&mut mesh, &ctx).expect("reproject");

        // Post: each vertex pulled back to radius ≈ 1.0.
        for vi in 0u32..6 {
            let r = mesh.vertex_position(VertexId(vi)).length();
            assert!(
                (r - 1.0).abs() < 1e-2,
                "vertex {vi} radius {r} not at sphere"
            );
        }
    }

    #[test]
    fn reproject_respects_max_distance() {
        let (mut mesh, sphere) = perturbed_sphere();
        let target = ScalarFieldTarget::new(&sphere);
        let nf = |_p: Vec3| Vec3::Z;
        let ctx = RepairContext::new(&nf).with_target(&target);
        // max_distance multiplied by median edge length (~ sqrt(2) × 1.5);
        // 0.01 means projections moving more than 0.015 are rejected.
        let pass = ReprojectToSurface {
            max_distance: 0.01,
            ..ReprojectToSurface::default()
        };
        pass.apply(&mut mesh, &ctx).expect("reproject");

        // Vertices stay at 1.5 because the displacement (0.5) exceeds the
        // tiny max_distance.
        for vi in 0u32..6 {
            let r = mesh.vertex_position(VertexId(vi)).length();
            assert!(
                (r - 1.5).abs() < 1e-3,
                "vertex {vi} moved despite max_distance"
            );
        }
    }

    #[test]
    fn reproject_errors_without_target() {
        let (mut mesh, _sphere) = perturbed_sphere();
        let ctx = RepairContext::noop();
        let pass = ReprojectToSurface::default();
        let err = pass.apply(&mut mesh, &ctx).unwrap_err();
        assert!(matches!(err, PassError::PreConstruction(_)));
    }

    #[test]
    fn reproject_skips_fixed_vertices() {
        let (mut mesh, sphere) = perturbed_sphere();
        // Pin vertex 0 as Fixed.
        mesh.set_vertex_class(VertexId(0), VertexClass::Fixed);
        let target = ScalarFieldTarget::new(&sphere);
        let nf = |_p: Vec3| Vec3::Z;
        let ctx = RepairContext::new(&nf).with_target(&target);
        let pass = ReprojectToSurface {
            max_distance: 100.0,
            ..ReprojectToSurface::default()
        };
        let pre = mesh.vertex_position(VertexId(0));
        pass.apply(&mut mesh, &ctx).expect("reproject");
        let post = mesh.vertex_position(VertexId(0));
        assert_eq!(pre, post, "Fixed vertex should not move");
    }
}
