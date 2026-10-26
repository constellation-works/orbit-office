//! Model adapters for the office plugin, behind `office-core` ports.
//!
//! The only crate allowed an HTTP client (ARCHITECTURE.md). Calls are
//! blocking: the plugin backend is one process per call. Secrets arrive
//! from the call context and never appear in errors or logs (STD-05).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]

mod baseten;
mod meta;
mod routing;
