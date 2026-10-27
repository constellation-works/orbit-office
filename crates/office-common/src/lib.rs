//! Shared foundation crate for the office workspace.
//!
//! Structured after `orbit-common` (Orbit's shared mechanism crate — a
//! labelled dependency, none of its code vendored here): domain contracts
//! stay in the owning crates (`office-core`, `office-store`), while this
//! crate owns the one common [`Error`], responsibility-based filesystem
//! mechanisms, SQLite connection defaults, bounded subprocess supervision, and small shared types.
//!
//! Layers above may depend on this crate; it depends on nothing internal,
//! and its contract types perform no I/O (STD-02 R5).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod error;
pub mod fs;
pub mod process;
mod sensitivity;
pub mod storage;

pub use error::{Error, Result};
pub use sensitivity::Sensitivity;

#[cfg(test)]
mod tests;
