//! Caller-supplied feature edges and pinned vertices.
//!
//! [`VertexClassifier`](super::vertex_class::VertexClassifier) computes
//! features automatically from dihedral angles, but some inputs already
//! know their creases — e.g. a STL/OBJ import that carried smoothing
//! groups, or a CAD-derived mesh where exact-90° edges should be marked
//! features regardless of the dihedral threshold. `FeatureSet` lets the
//! caller declare those explicitly; the classifier consults it *before*
//! running the dihedral check, so explicit creases always survive.

use std::collections::HashSet;

use super::half_edge::VertexId;

/// Caller-supplied set of crease edges and pinned vertices.
///
/// Edge keys are stored canonically (lower-id first) so callers can use
/// either direction interchangeably.
#[derive(Debug, Clone, Default)]
pub struct FeatureSet {
    crease_edges: HashSet<(VertexId, VertexId)>,
    pinned_vertices: HashSet<VertexId>,
}

impl FeatureSet {
    /// Creates an empty feature set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a crease edge between `u` and `v`. Order doesn't matter — the
    /// edge is stored canonically.
    pub fn add_crease(&mut self, u: VertexId, v: VertexId) {
        self.crease_edges.insert(canonical(u, v));
    }

    /// Pins a vertex to [`VertexClass::Fixed`](super::vertex_class::VertexClass::Fixed).
    pub fn pin(&mut self, v: VertexId) {
        self.pinned_vertices.insert(v);
    }

    /// Returns whether the edge `(u, v)` is a registered crease.
    pub fn is_crease(&self, u: VertexId, v: VertexId) -> bool {
        self.crease_edges.contains(&canonical(u, v))
    }

    /// Returns whether the vertex `v` is pinned.
    pub fn is_pinned(&self, v: VertexId) -> bool {
        self.pinned_vertices.contains(&v)
    }

    /// Iterates all canonical crease edges.
    pub fn creases(&self) -> impl Iterator<Item = (VertexId, VertexId)> + '_ {
        self.crease_edges.iter().copied()
    }

    /// Iterates all pinned vertices.
    pub fn pinned(&self) -> impl Iterator<Item = VertexId> + '_ {
        self.pinned_vertices.iter().copied()
    }

    /// Number of declared creases.
    pub fn crease_count(&self) -> usize {
        self.crease_edges.len()
    }

    /// Number of pinned vertices.
    pub fn pinned_count(&self) -> usize {
        self.pinned_vertices.len()
    }
}

fn canonical(u: VertexId, v: VertexId) -> (VertexId, VertexId) {
    if u.0 <= v.0 { (u, v) } else { (v, u) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_crease_canonicalises_edge_order() {
        let mut fs = FeatureSet::new();
        fs.add_crease(VertexId(5), VertexId(2));
        assert!(fs.is_crease(VertexId(2), VertexId(5)));
        assert!(fs.is_crease(VertexId(5), VertexId(2)));
        assert_eq!(fs.crease_count(), 1);
    }

    #[test]
    fn add_crease_dedupes_reversed_duplicate() {
        let mut fs = FeatureSet::new();
        fs.add_crease(VertexId(1), VertexId(2));
        fs.add_crease(VertexId(2), VertexId(1));
        assert_eq!(fs.crease_count(), 1);
    }

    #[test]
    fn pin_round_trip() {
        let mut fs = FeatureSet::new();
        fs.pin(VertexId(7));
        assert!(fs.is_pinned(VertexId(7)));
        assert!(!fs.is_pinned(VertexId(8)));
        assert_eq!(fs.pinned_count(), 1);
    }

    #[test]
    fn creases_iterator_yields_canonical_pairs() {
        let mut fs = FeatureSet::new();
        fs.add_crease(VertexId(3), VertexId(1));
        let collected: Vec<_> = fs.creases().collect();
        assert_eq!(collected.len(), 1);
        let (u, v) = collected[0];
        assert!(u.0 <= v.0);
    }
}
