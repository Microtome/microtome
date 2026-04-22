//! Pre-construction mesh cleanup: duplicate-face removal, orphan-vertex
//! removal, and (with a [`ReprojectionTarget`]) winding correction.
//!
//! T-junction resolution is intentionally deferred — it needs a spatial
//! index plus careful edge splitting that's easier post-construction;
//! v3 will tackle it.

use std::collections::HashSet;

use glam::Vec3;

use super::super::error::PassError;
use super::super::pass::{MeshRepairPass, PassOutcome, PassStage, PassWarningKind};
use crate::isosurface::IsoMesh;
use crate::mesh_repair::RepairContext;

/// Cheap mesh cleanup operations that don't need half-edge topology.
#[derive(Debug, Clone)]
pub struct CleanMesh {
    /// Drop duplicate triangles (same triangle as a sorted index tuple).
    pub remove_duplicate_faces: bool,
    /// Drop vertices not referenced by any surviving triangle, remap indices.
    pub remove_orphan_vertices: bool,
    /// Flip face winding when `face_normal · target.normal(centroid) < 0`.
    /// Requires `ctx.target = Some(...)`; emits a Skipped warning otherwise.
    pub fix_winding: bool,
}

impl Default for CleanMesh {
    fn default() -> Self {
        Self {
            remove_duplicate_faces: true,
            remove_orphan_vertices: true,
            fix_winding: true,
        }
    }
}

impl MeshRepairPass for CleanMesh {
    fn name(&self) -> &'static str {
        "clean_mesh"
    }

    fn stage(&self) -> PassStage {
        PassStage::PreConstruction
    }

    fn pre_construction(
        &self,
        mut iso: IsoMesh,
        ctx: &RepairContext<'_>,
    ) -> Result<(IsoMesh, PassOutcome), PassError> {
        let mut outcome = PassOutcome::noop(self.name());

        if self.remove_duplicate_faces {
            let pre = iso.indices.len() / 3;
            iso = drop_duplicate_faces(iso);
            let post = iso.indices.len() / 3;
            outcome.stats.faces_removed += (pre - post) as u32;
        }

        if self.fix_winding {
            if let Some(target) = ctx.target {
                let flips = fix_winding(&mut iso, target);
                if flips > 0 {
                    outcome.warn(
                        PassWarningKind::Clamped,
                        format!("flipped winding on {flips} triangles"),
                    );
                }
            } else {
                outcome.warn(
                    PassWarningKind::Skipped,
                    "fix_winding requires a ReprojectionTarget; skipped",
                );
            }
        }

        if self.remove_orphan_vertices {
            let pre = iso.positions.len();
            iso = drop_orphan_vertices(iso);
            let post = iso.positions.len();
            // Reuse vertices_merged as the orphan-removal counter; semantically
            // close enough ("vertices that disappeared from the output").
            outcome.stats.vertices_merged += (pre - post) as u32;
        }

        Ok((iso, outcome))
    }
}

fn drop_duplicate_faces(mut iso: IsoMesh) -> IsoMesh {
    let mut seen: HashSet<(u32, u32, u32)> = HashSet::with_capacity(iso.indices.len() / 3);
    let mut kept: Vec<u32> = Vec::with_capacity(iso.indices.len());
    for tri in iso.indices.chunks_exact(3) {
        let mut sorted = [tri[0], tri[1], tri[2]];
        sorted.sort_unstable();
        let key = (sorted[0], sorted[1], sorted[2]);
        if seen.insert(key) {
            kept.extend_from_slice(tri);
        }
    }
    iso.indices = kept;
    iso
}

fn drop_orphan_vertices(iso: IsoMesh) -> IsoMesh {
    let mut referenced: Vec<bool> = vec![false; iso.positions.len()];
    for &i in &iso.indices {
        referenced[i as usize] = true;
    }
    if referenced.iter().all(|&r| r) {
        return iso;
    }
    let mut remap: Vec<u32> = vec![u32::MAX; iso.positions.len()];
    let mut new_positions: Vec<Vec3> = Vec::new();
    let mut new_normals: Vec<Vec3> = Vec::new();
    for (old, &keep) in referenced.iter().enumerate() {
        if keep {
            remap[old] = new_positions.len() as u32;
            new_positions.push(iso.positions[old]);
            if let Some(&n) = iso.normals.get(old) {
                new_normals.push(n);
            }
        }
    }
    let new_indices: Vec<u32> = iso.indices.iter().map(|&i| remap[i as usize]).collect();
    IsoMesh {
        positions: new_positions,
        normals: new_normals,
        indices: new_indices,
    }
}

fn fix_winding(
    iso: &mut IsoMesh,
    target: &dyn super::super::reprojection::ReprojectionTarget,
) -> u32 {
    let mut flips: u32 = 0;
    let positions = iso.positions.clone();
    for tri in iso.indices.chunks_exact_mut(3) {
        let p0 = positions[tri[0] as usize];
        let p1 = positions[tri[1] as usize];
        let p2 = positions[tri[2] as usize];
        let face_normal = (p1 - p0).cross(p2 - p0);
        if face_normal == Vec3::ZERO {
            continue;
        }
        let centroid = (p0 + p1 + p2) / 3.0;
        let target_normal = target.normal(centroid);
        if face_normal.dot(target_normal) < 0.0 {
            tri.swap(1, 2);
            flips += 1;
        }
    }
    flips
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isosurface::Sphere;
    use crate::mesh_repair::reprojection::ScalarFieldTarget;

    fn iso(positions: Vec<Vec3>, indices: Vec<u32>) -> IsoMesh {
        let n = positions.len();
        IsoMesh {
            positions,
            normals: vec![Vec3::Z; n],
            indices,
        }
    }

    #[test]
    fn clean_drops_duplicate_face() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2, 1, 2, 0],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert_eq!(out.indices.len(), 3);
        assert_eq!(outcome.stats.faces_removed, 1);
    }

    #[test]
    fn clean_removes_orphan_vertex() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(99.0, 99.0, 99.0), // orphan
            ],
            vec![0, 1, 2],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert_eq!(out.positions.len(), 3);
        assert_eq!(outcome.stats.vertices_merged, 1);
    }

    #[test]
    fn clean_fix_winding_flips_inverted_triangle() {
        // Sphere centered at origin; inward-facing triangle at +x outside.
        // Set up: triangle near (1,0,0) wound clockwise as viewed from +x
        // (i.e. its normal points -x). The target's outward normal is +x;
        // we expect a flip.
        let input = iso(
            vec![
                Vec3::new(2.0, -1.0, -1.0),
                Vec3::new(2.0, 1.0, -1.0),
                Vec3::new(2.0, 0.0, 1.0),
            ],
            // Order chosen so cross((p1-p0), (p2-p0)) points -x.
            vec![0, 2, 1],
        );
        let sphere = Sphere::with_center(1.0, Vec3::ZERO);
        let target = ScalarFieldTarget::new(&sphere);
        let pass = CleanMesh::default();
        let nf = |_p: Vec3| Vec3::ZERO;
        let ctx = RepairContext::new(&nf).with_target(&target);
        let (out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        // After flip, indices should be (0, 1, 2) — winding reversed.
        assert_eq!(&out.indices[..3], &[0, 1, 2]);
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| matches!(w.kind, PassWarningKind::Clamped))
        );
    }

    #[test]
    fn clean_fix_winding_skipped_without_target() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (_out, outcome) = pass.pre_construction(input, &ctx).expect("clean");
        assert!(
            outcome
                .warnings
                .iter()
                .any(|w| matches!(w.kind, PassWarningKind::Skipped))
        );
    }

    #[test]
    fn clean_no_op_on_already_clean_mesh() {
        let input = iso(
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            vec![0, 1, 2],
        );
        let pass = CleanMesh::default();
        let ctx = RepairContext::noop();
        let (out, outcome) = pass.pre_construction(input.clone(), &ctx).expect("clean");
        assert_eq!(out.positions.len(), input.positions.len());
        assert_eq!(out.indices.len(), input.indices.len());
        assert_eq!(outcome.stats.faces_removed, 0);
        assert_eq!(outcome.stats.vertices_merged, 0);
    }
}
