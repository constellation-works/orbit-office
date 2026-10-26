//! Narrow filesystem mechanics shared by the drivers: path safety,
//! atomic owner-only writes, and content hashing. Mechanisms only — this
//! module knows nothing about Library documents.

pub(crate) mod atomic;
pub(crate) mod hash;
pub(crate) mod path_safety;
