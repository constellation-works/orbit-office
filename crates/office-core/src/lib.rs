//! The office plugin's domain, as a library.
//!
//! This crate is the Orbit exec backend's brain, split by responsibility:
//!
//! - [`protocol`] parses and dispatches one Orbit exec envelope
//!   (`{"schema_version": 1, "tool": "office.<verb>", "input": {...},
//!   "context": {...}}`) and renders the response envelope;
//! - [`tools`] holds the tool handlers (today only `status`);
//! - [`error`] is the crate's typed [`Error`].
//!
//! It never parses command-line arguments, never touches the terminal and
//! never decides where data lives: identity and secrets arrive in the call
//! context, never in tool input (STD-02 R3). Anything another surface would
//! need belongs here, not in the CLI crate.

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod error;
mod protocol;
mod tools;

pub use error::{Error, Result};
pub use protocol::{SCHEMA_VERSION, serve, tool_table};
pub use tools::SECRET_NAMES;

/// The plugin namespace: tools are `office.<verb>`.
pub const NAMESPACE: &str = "office";

/// The plugin version (the workspace package version).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests;
