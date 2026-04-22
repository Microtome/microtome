//! Vertex welding: merge spatially-coincident input vertices.
//!
//! DC often emits duplicate positions where adjacent cells solve their QEF
//! to the same world-space point (most commonly the mass-point fallback).
//! These coincidences break half-edge construction: the twin-pairing map
//! sees the "same" edge multiple times and rejects the input as
//! non-manifold. `WeldVertices` fixes this before half-edge construction.

use std::collections::HashMap;

use glam::{IVec3, Vec3};

use super::super::error::PassError;
use super::super::pass::{MeshRepairPass, PassOutcome, PassStage};
use crate::isosurface::IsoMesh;

/// Merges spatially-coincident vertices within `epsilon`.
#[derive(Debug, Clone)]
pub struct WeldVertices {
    /// Merge tolerance. When `bbox_relative` is `true`, this is a fraction
    /// of the bbox diagonal; otherwise an absolute world-space distance.
    pub epsilon: f32,
    /// Treat `epsilon` as a fraction of the input mesh's bbox diagonal.
    pub bbox_relative: bool,
}

impl Default for WeldVertices {
    fn default() -> Self {
        // 1e-6 × bbox_diag is tighter than DC ever produces unintentionally
        // and looser than the float precision of any reasonable mesh.
        Self {
            epsilon: 1e-6,
            bbox_relative: true,
        }
    }
}

impl WeldVertices {
    /// Constructs a pass with absolute `epsilon` (no bbox scaling).
    pub fn absolute(epsilon: f32) -> Self {
        Self {
            epsilon,
            bbox_relative: false,
        }
    }

    /// Constructs a pass with bbox-relative `epsilon`.
    pub fn relative(epsilon: f32) -> Self {
        Self {
            epsilon,
            bbox_relative: true,
        }
    }
}

impl MeshRepairPass for WeldVertices {
    fn name(&self) -> &'static str {
        "weld_vertices"
    }

    fn stage(&self) -> PassStage {
        PassStage::PreConstruction
    }

    fn pre_construction(&self, iso: IsoMesh) -> Result<(IsoMesh, PassOutcome), PassError> {
        let mut outcome = PassOutcome::noop(self.name());

        if iso.positions.is_empty() {
            return Ok((iso, outcome));
        }

        // Compute effective epsilon.
        let eps = if self.bbox_relative {
            let (min, max) = bbox(&iso.positions);
            let diag = (max - min).length().max(f32::MIN_POSITIVE);
            self.epsilon * diag
        } else {
            self.epsilon
        };
        if eps <= 0.0 {
            return Err(PassError::InvalidConfig(format!(
                "weld_vertices epsilon must be positive; got {eps}"
            )));
        }

        // Spatial hash: bin vertices by floor(p / eps). Each bin can hold
        // multiple original vertices.
        let mut bins: HashMap<IVec3, Vec<u32>> = HashMap::new();
        // Each original vertex maps to a cluster index (initially: its own).
        let mut cluster_of: Vec<u32> = (0..iso.positions.len() as u32).collect();
        // Cluster representatives: running sums for centroid / normal.
        let mut cluster_pos_sum: Vec<Vec3> = iso.positions.clone();
        let mut cluster_normal_sum: Vec<Vec3> = iso.normals.clone();
        let mut cluster_count: Vec<u32> = vec![1; iso.positions.len()];

        let inv_eps = 1.0 / eps;
        for (i, &p) in iso.positions.iter().enumerate() {
            let key = IVec3::new(
                (p.x * inv_eps).floor() as i32,
                (p.y * inv_eps).floor() as i32,
                (p.z * inv_eps).floor() as i32,
            );

            // Find any existing cluster within eps in the 27 surrounding bins.
            let mut target: Option<u32> = None;
            'outer: for dz in -1..=1 {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let neighbour = IVec3::new(key.x + dx, key.y + dy, key.z + dz);
                        if let Some(candidates) = bins.get(&neighbour) {
                            for &cand in candidates {
                                let cluster = cluster_of[cand as usize];
                                let rep = cluster_pos_sum[cluster as usize]
                                    / (cluster_count[cluster as usize] as f32);
                                if (rep - p).length() <= eps {
                                    target = Some(cluster);
                                    break 'outer;
                                }
                            }
                        }
                    }
                }
            }

            match target {
                Some(cluster) => {
                    // Merge into existing cluster; don't start a new one.
                    cluster_of[i] = cluster;
                    cluster_pos_sum[cluster as usize] += p;
                    cluster_normal_sum[cluster as usize] +=
                        iso.normals.get(i).copied().unwrap_or(Vec3::ZERO);
                    cluster_count[cluster as usize] += 1;
                    outcome.stats.vertices_merged += 1;
                }
                None => {
                    // New cluster; bins reference this vertex's cluster index.
                    bins.entry(key).or_default().push(i as u32);
                }
            }
        }

        // Compact clusters: assign dense new indices.
        let mut cluster_to_new: HashMap<u32, u32> = HashMap::new();
        let mut new_positions: Vec<Vec3> = Vec::new();
        let mut new_normals: Vec<Vec3> = Vec::new();
        for (i, &cluster) in cluster_of.iter().enumerate() {
            cluster_to_new.entry(cluster).or_insert_with(|| {
                let new_idx = new_positions.len() as u32;
                let centroid =
                    cluster_pos_sum[cluster as usize] / (cluster_count[cluster as usize] as f32);
                let normal = cluster_normal_sum[cluster as usize].normalize_or_zero();
                new_positions.push(centroid);
                new_normals.push(normal);
                new_idx
            });
            let _ = i;
        }
        let new_of: Vec<u32> = cluster_of.iter().map(|c| cluster_to_new[c]).collect();

        // Rewrite indices; drop degenerate triangles where two indices became equal.
        let mut new_indices: Vec<u32> = Vec::with_capacity(iso.indices.len());
        let mut dropped_tris: u32 = 0;
        for tri in iso.indices.chunks_exact(3) {
            let (a, b, c) = (
                new_of[tri[0] as usize],
                new_of[tri[1] as usize],
                new_of[tri[2] as usize],
            );
            if a == b || b == c || a == c {
                dropped_tris += 1;
                continue;
            }
            new_indices.extend_from_slice(&[a, b, c]);
        }
        outcome.stats.faces_removed = dropped_tris;

        let welded = IsoMesh {
            positions: new_positions,
            normals: new_normals,
            indices: new_indices,
        };
        Ok((welded, outcome))
    }
}

fn bbox(positions: &[Vec3]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    for p in positions {
        min = min.min(*p);
        max = max.max(*p);
    }
    (min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weld_merges_two_coincident_vertices() {
        // Two triangles sharing a vertex at (1,0,0) but with that vertex
        // duplicated (index 1 and index 3 both at the same point).
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0), // duplicate of vertex 1
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 6],
            indices: vec![0, 1, 2, 3, 4, 5],
        };
        let pass = WeldVertices::absolute(1e-4);
        let (out, outcome) = pass.pre_construction(iso).expect("weld");
        assert_eq!(out.positions.len(), 5);
        assert_eq!(outcome.stats.vertices_merged, 1);
        assert_eq!(out.indices.len(), 6);
    }

    #[test]
    fn weld_drops_degenerate_triangle_after_merge() {
        // Triangle whose two vertices are within epsilon → degenerate after weld.
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1e-6, 0.0, 0.0), // within epsilon of vertex 0
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        };
        let pass = WeldVertices::absolute(1e-4);
        let (out, outcome) = pass.pre_construction(iso).expect("weld");
        assert_eq!(out.positions.len(), 2);
        assert_eq!(outcome.stats.faces_removed, 1);
        assert!(out.indices.is_empty());
    }

    #[test]
    fn weld_preserves_winding_on_legitimate_mesh() {
        // A mesh with no coincident vertices: weld is a no-op on geometry.
        let iso = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        };
        let pass = WeldVertices::absolute(1e-4);
        let (out, outcome) = pass.pre_construction(iso.clone()).expect("weld");
        assert_eq!(out.positions.len(), 3);
        assert_eq!(outcome.stats.vertices_merged, 0);
        assert_eq!(out.indices, iso.indices);
    }

    #[test]
    fn weld_bbox_relative_scales_with_mesh() {
        // Same shape scaled up by 1000; with bbox_relative=true the epsilon
        // scales too, so the merge behaviour is identical.
        let scale = 1000.0;
        let iso_small = IsoMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0 + 1e-7, 0.0, 0.0), // within 1e-6×diag
            ],
            normals: vec![Vec3::Z; 4],
            indices: vec![0, 1, 2, 3, 1, 2],
        };
        let iso_big = IsoMesh {
            positions: iso_small.positions.iter().map(|p| *p * scale).collect(),
            normals: iso_small.normals.clone(),
            indices: iso_small.indices.clone(),
        };
        let pass = WeldVertices::relative(1e-6);
        let (out_small, _) = pass.pre_construction(iso_small).expect("weld");
        let (out_big, _) = pass.pre_construction(iso_big).expect("weld");
        assert_eq!(out_small.positions.len(), out_big.positions.len());
    }

    #[test]
    fn weld_stage_is_pre_construction() {
        let pass = WeldVertices::default();
        assert_eq!(pass.stage(), PassStage::PreConstruction);
    }

    #[test]
    fn weld_rejects_zero_epsilon() {
        let iso = IsoMesh {
            positions: vec![Vec3::ZERO; 3],
            normals: vec![Vec3::Z; 3],
            indices: vec![0, 1, 2],
        };
        let pass = WeldVertices::absolute(0.0);
        let err = pass.pre_construction(iso).unwrap_err();
        assert!(matches!(err, PassError::InvalidConfig(_)));
    }
}
