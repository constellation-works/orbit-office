//! Orbit adapter for the office plugin, behind an `office-core` port.
//!
//! Implements the callbacks the manifest requests in `orbit_tools`
//! (`orbit.task.add`, `orbit.task.update`, `orbit.task.artifact.put`, …).

// Library code never prints; diagnostics go through `tracing` (STD-02 R15).
#![deny(clippy::print_stderr, clippy::print_stdout)]

mod callbacks;
mod error;

pub use callbacks::{CALLBACK_ENV_NAMES, ORBIT_TOOLS, OrbitCallbacks};
pub use error::{Error, Result};
