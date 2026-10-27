//! Test support for the office workspace. A dev-dependency only: no crate
//! lists it under `[dependencies]` (scripts/check-dependency-direction.sh).
//!
//! The fakes for the four dispatch ports are established together, so tool
//! tests never touch real host state (STD-04). Each adapter task adds the
//! operations its port needs (ARCHITECTURE.md).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]

pub mod envelope;
pub mod fakes;
mod library_fixture;
