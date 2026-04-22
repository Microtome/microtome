//! Per-vertex classification used by feature-aware passes.
//!
//! Each vertex of a [`HalfEdgeMesh`](super::half_edge::HalfEdgeMesh) carries
//! a [`VertexClass`] that controls how passes treat it. Smoothing skips
//! `Fixed` vertices outright, slides `Boundary` and `Feature` vertices along
//! tangents only, and lets `Interior` vertices move freely. Simplification
//! refuses collapses that would merge vertices of incompatible class
//! (e.g. across two distinct boundary loops).
//!
//! v1 of the half-edge mesh stores all vertices as `Interior`. The
//! [`VertexClassifier`] populates the actual classes from mesh topology
//! plus an optional [`FeatureSet`](super::features::FeatureSet) — typically
//! invoked once after construction and again after any pass that mutates
//! connectivity (signalled via [`MeshRepairPass::reclassifies`](super::pass::MeshRepairPass::reclassifies)).

/// How a vertex should be treated by repair passes.
///
/// Higher discriminants take priority on conflict (see
/// [`combine`](Self::combine) and [`stronger`](Self::stronger)).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum VertexClass {
    /// Free interior vertex; smoothers and collapses move it without
    /// constraint.
    #[default]
    Interior,
    /// On a feature crease (dihedral angle exceeds threshold). Passes may
    /// move it tangentially along the crease but must not cross it.
    Feature,
    /// On a boundary loop. Passes preserve boundary topology — tangential
    /// motion only, collapses across boundary loops blocked.
    Boundary,
    /// User-pinned or artificial vertex (e.g. centroid added by hole fill).
    /// Passes treat as Dirichlet-fixed.
    Fixed,
}

impl VertexClass {
    /// Returns the stronger of two classes (Fixed > Boundary > Feature > Interior).
    pub fn stronger(a: Self, b: Self) -> Self {
        if a >= b { a } else { b }
    }

    /// Resolves the class of a vertex that's just been merged from `a` and `b`.
    /// Equivalent to [`stronger`](Self::stronger); kept as a named alias because
    /// callers typically reach for "what class does the merged vertex have?".
    pub fn combine(a: Self, b: Self) -> Self {
        Self::stronger(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_interior() {
        assert_eq!(VertexClass::default(), VertexClass::Interior);
    }

    #[test]
    fn stronger_picks_higher_priority() {
        assert_eq!(
            VertexClass::stronger(VertexClass::Interior, VertexClass::Boundary),
            VertexClass::Boundary
        );
        assert_eq!(
            VertexClass::stronger(VertexClass::Boundary, VertexClass::Fixed),
            VertexClass::Fixed
        );
        assert_eq!(
            VertexClass::stronger(VertexClass::Feature, VertexClass::Boundary),
            VertexClass::Boundary
        );
        assert_eq!(
            VertexClass::stronger(VertexClass::Feature, VertexClass::Interior),
            VertexClass::Feature
        );
    }

    #[test]
    fn combine_is_alias_for_stronger() {
        for a in [
            VertexClass::Interior,
            VertexClass::Feature,
            VertexClass::Boundary,
            VertexClass::Fixed,
        ] {
            for b in [
                VertexClass::Interior,
                VertexClass::Feature,
                VertexClass::Boundary,
                VertexClass::Fixed,
            ] {
                assert_eq!(VertexClass::combine(a, b), VertexClass::stronger(a, b));
            }
        }
    }
}
