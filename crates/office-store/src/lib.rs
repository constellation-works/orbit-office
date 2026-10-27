//! Persistence contracts and storage composition for the office Library.
//!
//! Structured after `orbit-store` (the persistence crate of
//! [Orbit](https://github.com/constellation-works/orbit), a labelled
//! dependency — none of its code is vendored here): file and SQLite
//! drivers stay behind a repository facade, narrow filesystem mechanics
//! live in `office_common::fs`, and the SQLite schema grows through an ordered
//! migration registry. The shape is reused; the code is this project's.
//!
//! Shared mechanisms (fs, SQLite defaults, common types) live in
//! `office-common`, this workspace's `orbit-common` counterpart.
//!
//! - [`contracts`] re-exports the pure core Library record shapes.
//! - [`repository::LibraryStore`] is the one surface callers use: it owns
//!   a Library root (plain Markdown files, the source of truth) and an
//!   index database under plugin state (rebuildable at any time).
//! - The file driver reads and writes the Library layout; the SQLite
//!   driver maintains the FTS5 index, link and citation tables.

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

pub mod contracts;
mod driver;
mod error;
mod repository;

pub use contracts::{DocumentKind, DocumentRecord, Revision, SearchFilter, SearchHit};
pub use driver::script::ScriptStore;
pub use error::{Error, Result};
pub use repository::LibraryStore;

#[cfg(test)]
mod tests;
