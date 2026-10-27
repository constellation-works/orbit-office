//! Deterministic child deadlines and finite output, without synthetic load.
use crate::{Error, process::run_bounded};
use std::{
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn command(program: &str, home: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin");
    command
}

#[test]
fn child_timeout_terminates_the_group_before_returning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let start = Instant::now();
    let mut child = command("/bin/sh", dir.path());
    // The shell waits for a sleeping child in its own supervised process group.
    child.args(["-c", "sleep 30; exit 0"]);
    let result = run_bounded(&mut child, dir.path(), Duration::from_millis(50), 1024);
    assert!(matches!(result, Err(Error::ProcessTimeout)));
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn finite_output_limit_is_reported_without_unbounded_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = command("/usr/bin/printf", dir.path());
    child.arg("a finite string longer than eight bytes");
    let result = run_bounded(&mut child, dir.path(), Duration::from_secs(1), 8);
    assert!(matches!(result, Err(Error::ProcessOutputLimit)));
}

#[test]
fn clean_and_failed_child_exits_keep_their_actual_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = command("/usr/bin/printf", dir.path());
    child.arg("bounded output");
    let (status, output) =
        run_bounded(&mut child, dir.path(), Duration::from_secs(1), 1024).expect("run");
    assert!(status.success());
    assert_eq!(output, "bounded output");
    let mut child = command("/bin/sh", dir.path());
    child.args(["-c", "exit 7"]);
    assert_eq!(
        run_bounded(&mut child, dir.path(), Duration::from_secs(1), 1024)
            .expect("run")
            .0
            .code(),
        Some(7)
    );
}
