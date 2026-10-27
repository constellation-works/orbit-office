//! Test support for the office workspace. A dev-dependency only: no crate
//! lists it under `[dependencies]` (scripts/check-dependency-direction.sh).
//!
//! Each fake arrives with the port it fakes, so tool tests never touch real
//! host state (STD-04).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]

pub mod envelope;
pub mod fakes;
mod library_fixture;
