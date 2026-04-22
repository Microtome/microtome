//! Half-edge mesh data structure.
//!
//! v1 scope:
//! - ID newtypes with `INVALID = u32::MAX` sentinels.
//! - `HalfEdgeMesh` with `vertices`, `half_edges`, `faces` Vecs + freelists.
//! - `from_iso_mesh` / `to_iso_mesh` conversion.
//! - Basic validators and core queries (valence, one-ring, boundary loops).
//!
//! Subsequent tasks fill in construction, operations, and queries.

/// Opaque identifier for a vertex in a [`HalfEdgeMesh`].
///
/// The sentinel [`VertexId::INVALID`] is used for removed or placeholder slots.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct VertexId(pub u32);

/// Opaque identifier for a half-edge in a [`HalfEdgeMesh`].
///
/// The sentinel [`HalfEdgeId::INVALID`] is used for boundary twins
/// (half-edges with no opposing face) and for removed or placeholder slots.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct HalfEdgeId(pub u32);

/// Opaque identifier for a triangular face in a [`HalfEdgeMesh`].
///
/// The sentinel [`FaceId::INVALID`] is used for boundary half-edges
/// (half-edges with no face on their left) and for removed or placeholder slots.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct FaceId(pub u32);

impl VertexId {
    /// Sentinel value meaning "no vertex".
    pub const INVALID: Self = Self(u32::MAX);

    /// Returns this ID as a `usize` index into a backing `Vec`.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `true` if this ID is not the `INVALID` sentinel.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

impl HalfEdgeId {
    /// Sentinel value meaning "no half-edge".
    pub const INVALID: Self = Self(u32::MAX);

    /// Returns this ID as a `usize` index into a backing `Vec`.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `true` if this ID is not the `INVALID` sentinel.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

impl FaceId {
    /// Sentinel value meaning "no face".
    pub const INVALID: Self = Self(u32::MAX);

    /// Returns this ID as a `usize` index into a backing `Vec`.
    #[inline]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    /// Returns `true` if this ID is not the `INVALID` sentinel.
    #[inline]
    pub const fn is_valid(self) -> bool {
        self.0 != u32::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_sentinel_is_u32_max() {
        assert_eq!(VertexId::INVALID.0, u32::MAX);
        assert_eq!(HalfEdgeId::INVALID.0, u32::MAX);
        assert_eq!(FaceId::INVALID.0, u32::MAX);
    }

    #[test]
    fn invalid_ids_report_invalid() {
        assert!(!VertexId::INVALID.is_valid());
        assert!(!HalfEdgeId::INVALID.is_valid());
        assert!(!FaceId::INVALID.is_valid());
    }

    #[test]
    fn regular_ids_report_valid() {
        assert!(VertexId(0).is_valid());
        assert!(HalfEdgeId(42).is_valid());
        assert!(FaceId(1000).is_valid());
    }

    #[test]
    fn index_converts_to_usize() {
        assert_eq!(VertexId(7).index(), 7usize);
    }
}
