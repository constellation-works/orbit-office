//! The root of the command tree: the program and its one subcommand.
//!
//! The binary's surface is the Orbit exec protocol, not a human workflow:
//! Orbit invokes `office orbit-tool` with one JSON envelope on stdin and
//! reads one JSON envelope from stdout. `--help` is rendered from these doc
//! comments, so they are user-facing text (STD-01 R23, R25).

use clap::{Parser, Subcommand};
use std::ffi::OsString;

const ROOT_EXAMPLES: &str = "\
Examples:
  office orbit-tool < request.json
  office orbit-tool < request.json > response.json

Exit status: 0 on success, 1 when a command fails, 2 on a usage error.";

/// Orbit exec backend for the office plugin: serve one call per invocation.
#[derive(Debug, Parser)]
#[command(
    name = "office",
    version,
    arg_required_else_help = true,
    after_help = ROOT_EXAMPLES
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// The one command: serve a single Orbit exec call.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Serve one Orbit exec call (JSON envelope on stdin, one on stdout)
    OrbitTool,
}

impl Cli {
    /// Parse a command line. Errors are clap's, rendered by the caller as a
    /// usage error (exit 2, STD-01 R20).
    pub(crate) fn parse_args(args: &[OsString]) -> Result<Self, clap::Error> {
        Self::try_parse_from(args)
    }
}
