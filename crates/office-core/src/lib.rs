//! The office plugin's pure domain, as a library.
//!
//! - [`protocol`] parses one Orbit exec envelope (`{"schema_version": 1,
//!   "tool": "office.<verb>", "input": {...}, "context": {...}}`) and
//!   renders the response envelope;
//! - [`tool`] holds the tool contract types every tool module declares;
//! - [`error`] is the crate's typed [`Error`];
//! - `domain/` will hold the domain model and its rules, and `ports/` the
//!   traits the application calls to reach files, models and Orbit.
//!
//! It does no I/O, never parses command-line arguments, never touches the
//! terminal and never decides where data lives: identity and secrets arrive
//! in the call context, never in tool input (STD-02 R3, R5). Dispatch and
//! the tool handlers live in `office-tools`.

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod domain;
mod error;
pub mod ports;
pub mod protocol;
mod secrets;
pub mod tool;

pub use error::{Error, Result};
pub use protocol::SCHEMA_VERSION;
pub use secrets::SECRET_NAMES;

/// The plugin namespace: tools are `office.<verb>`.
pub const NAMESPACE: &str = "office";

/// The plugin version (the workspace package version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests;
