//! The office plugin's application layer.
//!
//! - `registry` lists every tool, built or planned, one row each;
//! - [`serve`] answers one parsed exec envelope: find the tool, refuse
//!   unknown input keys, run the handler, render the response;
//! - `tools/` holds one module per tool, grouped by spec area.
//!
//! Tools reach files, models and Orbit only through `office-core` ports, so
//! each one runs against the fakes in `office-testkit` (ARCHITECTURE.md).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod dispatch;
mod registry;
mod tools;

pub use dispatch::serve;
pub use registry::{Tool, tools};
pub use tools::scripts::run::prepare_script;

#[cfg(test)]
mod tests;
