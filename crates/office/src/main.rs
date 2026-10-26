//! `office`: the Orbit exec backend for the office plugin.
//!
//! `main` is wiring only: parse, run the one command, render through the
//! output module. See ARCHITECTURE.md for the layers.

// Nothing in this crate uses `print!`: the output module writes to locked
// handles so a closed pipe is an error value, not a panic (STD-01 R13).
#![deny(clippy::print_stderr, clippy::print_stdout)]
// Unit tests use unwrap/expect for fixture setup; production call sites remain linted.
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

mod app;
mod cli;
mod error;
mod output;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let cli = match cli::Cli::parse_args(&args) {
        Ok(cli) => cli,
        Err(err) => return output::finish_parse_error(&err),
    };
    match cli.command {
        cli::Command::OrbitTool => output::finish(app::serve_exec()),
    }
}

#[cfg(test)]
mod tests;
