//! Spatial acceleration structures used by mesh-repair passes.
//!
//! [`TriangleBvh`] is a stand-alone, top-down median-split BVH over a slice
//! of triangles. Used by [`MeshTarget`](super::reprojection::MeshTarget) to
//! accelerate closest-point queries and by
//! [`DetectSelfIntersections`](super::passes::detect_self_intersect::DetectSelfIntersections)
//! to skip the O(n²) pairwise scan in favour of AABB-overlap queries.
//!
//! The BVH stores only the topology (node AABBs + child / leaf-tri-index
//! references); triangle vertices are owned by the caller and supplied via
//! callbacks at query time.

use glam::Vec3;

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    /// Componentwise minimum corner.
    pub min: Vec3,
    /// Componentwise maximum corner.
    pub max: Vec3,
}

impl Aabb {
    /// Builds an AABB tightly enclosing a triangle.
    pub fn from_triangle(t: &[Vec3; 3]) -> Self {
        let min = t[0].min(t[1]).min(t[2]);
        let max = t[0].max(t[1]).max(t[2]);
        Self { min, max }
    }

    /// Builds the smallest AABB enclosing both `self` and `other`.
    pub fn union(&self, other: &Self) -> Self {
        Self {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    /// Returns the squared Euclidean distance from `p` to this AABB.
    /// Zero when `p` is inside the box.
    pub fn distance_squared_to_point(&self, p: Vec3) -> f32 {
        let dx = (self.min.x - p.x).max(0.0).max(p.x - self.max.x);
        let dy = (self.min.y - p.y).max(0.0).max(p.y - self.max.y);
        let dz = (self.min.z - p.z).max(0.0).max(p.z - self.max.z);
        dx * dx + dy * dy + dz * dz
    }

    /// Returns `true` if the two AABBs share at least one point.
    pub fn intersects(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
            && self.min.z <= other.max.z
            && self.max.z >= other.min.z
    }
}

/// Top-down median-split BVH over a slice of triangles.
pub struct TriangleBvh {
    nodes: Vec<BvhNode>,
}

#[derive(Debug, Clone, Copy)]
struct BvhNode {
    aabb: Aabb,
    /// For inner nodes: left child index. For leaf nodes: triangle index.
    primary: u32,
    /// For inner nodes: right child index. For leaf nodes: unused (0).
    secondary: u32,
    is_leaf: bool,
}

impl TriangleBvh {
    /// Builds a BVH from the given triangles. Returns `None` if the input
    /// is empty.
    pub fn build(triangles: &[[Vec3; 3]]) -> Option<Self> {
        if triangles.is_empty() {
            return None;
        }
        let centroids: Vec<Vec3> = triangles
            .iter()
            .map(|t| (t[0] + t[1] + t[2]) / 3.0)
            .collect();
        let aabbs: Vec<Aabb> = triangles.iter().map(Aabb::from_triangle).collect();
        let mut indices: Vec<u32> = (0..triangles.len() as u32).collect();
        let mut nodes: Vec<BvhNode> = Vec::with_capacity(triangles.len() * 2);
        Self::build_recursive(&mut indices, &centroids, &aabbs, &mut nodes);
        Some(Self { nodes })
    }

    fn build_recursive(
        indices: &mut [u32],
        centroids: &[Vec3],
        aabbs: &[Aabb],
        nodes: &mut Vec<BvhNode>,
    ) -> u32 {
        let mut aabb = aabbs[indices[0] as usize];
        for &i in indices.iter().skip(1) {
            aabb = aabb.union(&aabbs[i as usize]);
        }

        if indices.len() == 1 {
            let id = nodes.len() as u32;
            nodes.push(BvhNode {
                aabb,
                primary: indices[0],
                secondary: 0,
                is_leaf: true,
            });
            return id;
        }

        let extent = aabb.max - aabb.min;
        let axis = if extent.x >= extent.y && extent.x >= extent.z {
            0
        } else if extent.y >= extent.z {
            1
        } else {
            2
        };
        indices.sort_by(|&a, &b| {
            let ca = centroids[a as usize][axis];
            let cb = centroids[b as usize][axis];
            ca.partial_cmp(&cb).unwrap_or(std::cmp::Ordering::Equal)
        });
        let mid = indices.len() / 2;

        let our_id = nodes.len() as u32;
        nodes.push(BvhNode {
            aabb,
            primary: 0,
            secondary: 0,
            is_leaf: false,
        });
        let (left_slice, right_slice) = indices.split_at_mut(mid);
        let left_id = Self::build_recursive(left_slice, centroids, aabbs, nodes);
        let right_id = Self::build_recursive(right_slice, centroids, aabbs, nodes);
        nodes[our_id as usize].primary = left_id;
        nodes[our_id as usize].secondary = right_id;
        our_id
    }

    /// Closest point on any triangle to `query`. `get_tri` resolves a stored
    /// triangle index back to its three vertices. Returns
    /// `(triangle_index, position, distance)` of the nearest contact.
    pub fn closest_point<F>(&self, query: Vec3, get_tri: F) -> Option<(usize, Vec3, f32)>
    where
        F: Fn(usize) -> [Vec3; 3],
    {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best: Option<(usize, Vec3, f32)> = None;
        self.closest_recurse(0, query, &get_tri, &mut best);
        best.map(|(i, p, d_sq)| (i, p, d_sq.sqrt()))
    }

    fn closest_recurse<F>(
        &self,
        node_id: u32,
        query: Vec3,
        get_tri: &F,
        best: &mut Option<(usize, Vec3, f32)>,
    ) where
        F: Fn(usize) -> [Vec3; 3],
    {
        let node = self.nodes[node_id as usize];
        let lower = node.aabb.distance_squared_to_point(query);
        if let Some((_, _, b)) = *best
            && lower > b
        {
            return;
        }
        if node.is_leaf {
            let idx = node.primary as usize;
            let tri = get_tri(idx);
            let q = closest_point_on_triangle(query, &tri);
            let d_sq = (q - query).length_squared();
            let better = match *best {
                Some((_, _, b)) => d_sq < b,
                None => true,
            };
            if better {
                *best = Some((idx, q, d_sq));
            }
            return;
        }
        let l = node.primary;
        let r = node.secondary;
        let l_lo = self.nodes[l as usize].aabb.distance_squared_to_point(query);
        let r_lo = self.nodes[r as usize].aabb.distance_squared_to_point(query);
        let (first, second) = if l_lo <= r_lo { (l, r) } else { (r, l) };
        self.closest_recurse(first, query, get_tri, best);
        self.closest_recurse(second, query, get_tri, best);
    }

    /// Invokes `visit` once for every triangle whose AABB overlaps `query`.
    pub fn visit_overlapping<F>(&self, query: Aabb, mut visit: F)
    where
        F: FnMut(usize),
    {
        if self.nodes.is_empty() {
            return;
        }
        self.visit_overlapping_recurse(0, &query, &mut visit);
    }

    fn visit_overlapping_recurse<F>(&self, node_id: u32, query: &Aabb, visit: &mut F)
    where
        F: FnMut(usize),
    {
        let node = self.nodes[node_id as usize];
        if !node.aabb.intersects(query) {
            return;
        }
        if node.is_leaf {
            visit(node.primary as usize);
            return;
        }
        self.visit_overlapping_recurse(node.primary, query, visit);
        self.visit_overlapping_recurse(node.secondary, query, visit);
    }
}

/// Closest point on a triangle to `p` (Ericson, *Real-Time Collision
/// Detection* §5.1.5).
pub fn closest_point_on_triangle(p: Vec3, tri: &[Vec3; 3]) -> Vec3 {
    let a = tri[0];
    let b = tri[1];
    let c = tri[2];
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + v * ab;
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + w * ac;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + w * (c - b);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aabb_distance_squared_zero_when_inside() {
        let aabb = Aabb {
            min: Vec3::new(-1.0, -1.0, -1.0),
            max: Vec3::new(1.0, 1.0, 1.0),
        };
        assert_eq!(aabb.distance_squared_to_point(Vec3::ZERO), 0.0);
    }

    #[test]
    fn aabb_distance_squared_outside_is_positive() {
        let aabb = Aabb {
            min: Vec3::new(0.0, 0.0, 0.0),
            max: Vec3::new(1.0, 1.0, 1.0),
        };
        // (3,0,0) is dx=2 beyond +x face. d² = 4.
        assert!((aabb.distance_squared_to_point(Vec3::new(3.0, 0.0, 0.0)) - 4.0).abs() < 1e-6);
    }

    #[test]
    fn aabb_intersects_overlapping_pair() {
        let a = Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        let b = Aabb {
            min: Vec3::new(0.5, 0.5, 0.5),
            max: Vec3::new(2.0, 2.0, 2.0),
        };
        assert!(a.intersects(&b));
        let c = Aabb {
            min: Vec3::new(2.0, 2.0, 2.0),
            max: Vec3::new(3.0, 3.0, 3.0),
        };
        assert!(!a.intersects(&c));
    }

    #[test]
    fn bvh_build_returns_none_for_empty() {
        assert!(TriangleBvh::build(&[]).is_none());
    }

    #[test]
    fn bvh_closest_point_finds_nearest_triangle() {
        let triangles = vec![
            [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            [
                Vec3::new(10.0, 10.0, 0.0),
                Vec3::new(11.0, 10.0, 0.0),
                Vec3::new(10.0, 11.0, 0.0),
            ],
        ];
        let bvh = TriangleBvh::build(&triangles).expect("non-empty");
        let (idx, pos, _d) = bvh
            .closest_point(Vec3::new(0.25, 0.25, 5.0), |i| triangles[i])
            .expect("any triangle");
        assert_eq!(idx, 0);
        assert!((pos.z).abs() < 1e-4);
    }

    #[test]
    fn bvh_closest_point_matches_brute_force_on_random_query() {
        // Build a small grid of triangles and verify the BVH agrees with
        // an O(n²) brute-force search.
        let mut triangles = Vec::new();
        for x in 0..5 {
            for y in 0..5 {
                let fx = x as f32;
                let fy = y as f32;
                triangles.push([
                    Vec3::new(fx, fy, 0.0),
                    Vec3::new(fx + 1.0, fy, 0.0),
                    Vec3::new(fx, fy + 1.0, 0.0),
                ]);
            }
        }
        let bvh = TriangleBvh::build(&triangles).expect("non-empty");
        for query in [
            Vec3::new(2.5, 2.5, 1.0),
            Vec3::new(-3.0, 0.5, 2.0),
            Vec3::new(7.0, 7.0, -3.0),
        ] {
            let (_bvh_idx, _bvh_pos, bvh_d) = bvh
                .closest_point(query, |i| triangles[i])
                .expect("non-empty");
            let mut best_d = f32::INFINITY;
            for tri in &triangles {
                let q = closest_point_on_triangle(query, tri);
                let d = (q - query).length();
                if d < best_d {
                    best_d = d;
                }
            }
            assert!((bvh_d - best_d).abs() < 1e-3, "bvh={bvh_d} brute={best_d}");
        }
    }

    #[test]
    fn bvh_visit_overlapping_returns_aabb_overlap_set() {
        let triangles = vec![
            [
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            [
                Vec3::new(0.5, 0.5, 0.5),
                Vec3::new(1.5, 0.5, 0.5),
                Vec3::new(0.5, 1.5, 0.5),
            ],
            [
                Vec3::new(10.0, 10.0, 10.0),
                Vec3::new(11.0, 10.0, 10.0),
                Vec3::new(10.0, 11.0, 10.0),
            ],
        ];
        let bvh = TriangleBvh::build(&triangles).expect("non-empty");
        let query = Aabb {
            min: Vec3::new(0.4, 0.4, -1.0),
            max: Vec3::new(0.6, 0.6, 1.0),
        };
        let mut hits: Vec<usize> = Vec::new();
        bvh.visit_overlapping(query, |i| hits.push(i));
        hits.sort();
        // Triangles 0 and 1 overlap the query box; triangle 2 is far away.
        assert_eq!(hits, vec![0, 1]);
    }
}
