//! Tangential relaxation: move interior vertices toward the one-ring
//! centroid, projected onto the local tangent plane.
//!
//! Unlike Laplacian smoothing, this pass moves vertices *along* the
//! surface (in the tangent plane defined by the local normal), so it
//! preserves volume by construction. Used inside isotropic remeshing
//! and as a standalone pass to even out triangle quality.

use glam::Vec3;

use super::super::error::PassError;
use super::super::half_edge::{HalfEdgeMesh, VertexId};
use super::super::pass::{MeshRepairPass, PassOutcome};
use super::super::vertex_class::VertexClass;
use crate::mesh_repair::RepairContext;

/// Tangential relaxation: shift Interior vertices toward their one-ring
/// centroid, projected onto the local tangent plane.
#[derive(Debug, Clone)]
pub struct AngleRelax {
    /// Number of relaxation iterations.
    pub iterations: u32,
    /// Step factor in `(0, 1]`. `1.0` jumps the vertex straight to the
    /// projected centroid; `0.5` (default) takes a half-step for stability.
    pub step: f32,
}

impl Default for AngleRelax {
    fn default() -> Self {
        Self {
            iterations: 3,
            step: 0.5,
        }
    }
}

impl MeshRepairPass for AngleRelax {
    fn name(&self) -> &'static str {
        "angle_relax"
    }

    fn apply(
        &self,
        mesh: &mut HalfEdgeMesh,
        _ctx: &RepairContext<'_>,
    ) -> Result<PassOutcome, PassError> {
        if !(self.step > 0.0 && self.step <= 1.0) {
            return Err(PassError::InvalidConfig(format!(
                "angle_relax step must be in (0, 1]; got {}",
                self.step
            )));
        }

        let mut outcome = PassOutcome::noop(self.name());

        for _ in 0..self.iterations {
            let n_verts = mesh.vertices.len();
            let snapshot: Vec<Vec3> = mesh.vertices.iter().map(|v| v.pos).collect();
            let mut new_pos = snapshot.clone();

            for vi in 0..n_verts {
                let vid = VertexId(vi as u32);
                if !mesh.vertex_is_live(vid) {
                    continue;
                }
                if mesh.vertex_class(vid) != VertexClass::Interior {
                    continue;
                }

                // One-ring centroid (uniform weights — sufficient for v2
                // first cut; full Mean Value Coordinates land with v2.5).
                let mut sum = Vec3::ZERO;
                let mut count = 0u32;
                for n in mesh.vertex_one_ring(vid) {
                    sum += snapshot[n.index()];
                    count += 1;
                }
                if count == 0 {
                    continue;
                }
                let centroid = sum / count as f32;
                let displacement = centroid - snapshot[vi];

                // Local normal: area-weighted mean of incident face normals.
                let normal = vertex_normal(mesh, vid);
                if normal == Vec3::ZERO {
                    // Fall back to plain Laplacian shift.
                    new_pos[vi] = snapshot[vi] + self.step * displacement;
                    outcome.stats.vertices_smoothed =
                        outcome.stats.vertices_smoothed.saturating_add(1);
                    continue;
                }
                // Tangential component only.
                let tangent_disp = displacement - displacement.dot(normal) * normal;
                new_pos[vi] = snapshot[vi] + self.step * tangent_disp;
                outcome.stats.vertices_smoothed = outcome.stats.vertices_smoothed.saturating_add(1);
            }

            for (vi, &p) in new_pos.iter().enumerate() {
                if mesh.vertices[vi].removed {
                    continue;
                }
                mesh.vertices[vi].pos = p;
            }
        }

        Ok(outcome)
    }
}

fn vertex_normal(mesh: &HalfEdgeMesh, v: VertexId) -> Vec3 {
    // Iterate incident faces by walking the one-ring half-edges. Sum
    // area-weighted face normals. Returns Vec3::ZERO if degenerate.
    let he_out = mesh.vertex_he_out(v);
    if !he_out.is_valid() {
        return Vec3::ZERO;
    }
    let mut sum = Vec3::ZERO;
    let mut current = he_out;
    let start = current;
    loop {
        let face = mesh.he_face(current);
        if mesh.face_is_live(face) {
            let [p0, p1, p2] = mesh.face_positions(face);
            let n = (p1 - p0).cross(p2 - p0);
            // n is area-weighted (length = 2 × area).
            sum += n;
        }
        let prev = mesh.he_prev(current);
        let prev_twin = mesh.he_twin(prev);
        if !prev_twin.is_valid() {
            break;
        }
        if prev_twin == start {
            break;
        }
        current = prev_twin;
    }
    sum.normalize_or_zero()
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

    /// Hex fan: a central vertex surrounded by 6 boundary vertices in a
    /// regular hexagon. Central vertex placed off-centre.
    fn hex_fan_offcenter() -> HalfEdgeMesh {
        let mut positions = vec![Vec3::new(0.4, 0.2, 0.0)]; // off-centre central
        for i in 0..6u32 {
            let a = i as f32 * std::f32::consts::TAU / 6.0;
            positions.push(Vec3::new(a.cos(), a.sin(), 0.0));
        }
        let mut indices = Vec::new();
        for i in 0..6u32 {
            let a = i + 1;
            let b = (i + 1) % 6 + 1;
            indices.extend_from_slice(&[0, a, b]);
        }
        HalfEdgeMesh::from_iso_mesh(&iso(positions, indices)).expect("hex")
    }

    #[test]
    fn relax_shifts_offcenter_vertex_toward_centroid() {
        let mut mesh = hex_fan_offcenter();
        let pre = mesh.vertex_position(VertexId(0));
        let pass = AngleRelax::default();
        let ctx = RepairContext::noop();
        pass.apply(&mut mesh, &ctx).expect("relax");
        let post = mesh.vertex_position(VertexId(0));
        // Pre is at (0.4, 0.2, 0); centroid of the hexagon is (0, 0, 0).
        // Post should be closer to (0, 0, 0).
        assert!(
            post.length() < pre.length(),
            "vertex 0 didn't move toward centroid: pre={pre:?} post={post:?}"
        );
    }

    #[test]
    fn relax_pins_boundary_vertices() {
        let mut mesh = hex_fan_offcenter();
        // Boundary vertices are 1..7; classify them.
        super::super::super::vertex_class::VertexClassifier::default().classify(&mut mesh);
        // Boundary vertices got Boundary then Fixed (pin_boundary default).
        let pre = mesh.vertex_position(VertexId(1));
        let pass = AngleRelax::default();
        let ctx = RepairContext::noop();
        pass.apply(&mut mesh, &ctx).expect("relax");
        let post = mesh.vertex_position(VertexId(1));
        assert_eq!(pre, post, "boundary vertex must not move");
    }

    #[test]
    fn relax_rejects_invalid_step() {
        let mut mesh = hex_fan_offcenter();
        let pass = AngleRelax {
            step: -0.1,
            ..AngleRelax::default()
        };
        let ctx = RepairContext::noop();
        assert!(pass.apply(&mut mesh, &ctx).is_err());
    }
}
