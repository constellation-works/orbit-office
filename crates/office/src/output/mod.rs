//! The output layer: the only code that writes to stdout or stderr.
//!
//! stdout carries the one response envelope and nothing else (STD-01 R12).
//! Writes go through `write!` on a locked handle rather than `println!`, so
//! a closed pipe comes back as an `io::Error` that [`finish`] turns into a
//! silent exit 0 (STD-01 R13) instead of a panic.

use crate::error::{Error, Result};
use std::io::{self, Write};
use std::process::ExitCode;

/// Render a command's outcome and return the process exit code.
pub(crate) fn finish(outcome: Result<String>) -> ExitCode {
    match outcome.and_then(|payload| {
        let mut out = io::stdout().lock();
        writeln!(out, "{payload}").map_err(Error::Output)?;
        out.flush().map_err(Error::Output)
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Output(error)) if error.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "error: {error} [{}]", error.code());
            ExitCode::from(1)
        }
    }
}

/// Handle an argument-parsing outcome that is not a parsed command line:
/// `--help` and `--version` (stdout, exit 0) or a usage error (stderr,
/// exit 2; STD-01 R20).
pub(crate) fn finish_parse_error(error: &clap::Error) -> ExitCode {
    let code = u8::try_from(error.exit_code()).unwrap_or(2);
    let rendered = error.render().to_string();
    if error.use_stderr() {
        let _ = io::stderr().lock().write_all(rendered.as_bytes());
    } else {
        let written = io::stdout().lock().write_all(rendered.as_bytes());
        if let Err(io_error) = written
            && io_error.kind() != io::ErrorKind::BrokenPipe
        {
            return ExitCode::from(1);
        }
    }
    ExitCode::from(code)
}
