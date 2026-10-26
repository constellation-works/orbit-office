//! Shared fixture for the integration tests: run the built binary with a
//! cleared environment, feeding it one request envelope on stdin.
//!
//! Every child gets an empty environment with only `HOME` set to a
//! temporary directory, so no test can inherit the developer's variables
//! (STD-03 R20).
#![allow(dead_code)]

use std::io::Write;
use std::process::{Command, Output, Stdio};

/// The binary under test, exactly as cargo built it (STD-01 R24).
pub const BIN: &str = env!("CARGO_BIN_EXE_office");

/// A temporary home, deleted on drop.
pub struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::create_dir(dir.path().join("home")).expect("create fake home");
        Self { dir }
    }

    /// Run the binary with `args` and `stdin`, capturing everything.
    pub fn run(&self, args: &[&str], stdin: &str) -> Output {
        let mut child = Command::new(BIN)
            .args(args)
            .env_clear()
            .env("HOME", self.dir.path().join("home"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin is piped")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("wait for binary")
    }

    /// Run and require exit 0.
    pub fn ok(&self, args: &[&str], stdin: &str) -> Output {
        let out = self.run(args, stdin);
        assert_eq!(
            out.status.code(),
            Some(0),
            "{args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        out
    }
}

pub fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).expect("utf-8 output")
}
