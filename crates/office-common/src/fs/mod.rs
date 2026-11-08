//! Shared filesystem primitives for safe paths, atomic writes, and content hashes.
//! These mechanics stay independent of Library document semantics.

pub mod atomic;
pub mod hash;
pub mod path_safety;
pub mod permissions;
