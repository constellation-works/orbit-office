//! Narrow filesystem mechanics shared by the workspace's crates:
//! path safety, atomic owner-only writes, and content hashing.
//! Mechanisms only — this module knows nothing about Library documents.

pub mod atomic;
pub mod hash;
pub mod path_safety;
