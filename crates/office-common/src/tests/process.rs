//! Deterministic child deadlines, descendant sweeps, finite output and
//! environment allowlisting through `cw-supervise`, without synthetic load.
use crate::{
    Error,
    process::{Cap, env, run, run_captured, run_logged},
};
use std::{
    ffi::OsString,
    io::ErrorKind,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

fn command(program: &str, home: &Path) -> Command {
    let mut command = Command::new(program);
    env::allow(&mut command, &[]);
    command.env("HOME", home).env("PATH", "/usr/bin:/bin");
    command
}

#[cfg(target_os = "linux")]
fn process_is_gone_or_zombie(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Err(_) => true,
        Ok(stat) => stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .is_none_or(|state| state == "Z" || state == "X"),
    }
}

#[cfg(target_os = "macos")]
fn process_is_gone_or_zombie(pid: u32) -> bool {
    let mut ps = Command::new("/bin/ps");
    env::allow(&mut ps, &[]);
    ps.args(["-o", "state=", "-p", &pid.to_string()]);
    match run_captured(ps, Duration::from_secs(5), Cap::kill(1024), Cap::kill(1024)) {
        Ok(output) => {
            if !output.status.success() {
                return true;
            }
            let state = String::from_utf8_lossy(&output.stdout);
            let trimmed = state.trim();
            trimmed.is_empty() || trimmed.starts_with('Z')
        }
        Err(_) => false,
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn wait_until_gone(pid: u32, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if process_is_gone_or_zombie(pid) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn child_timeout_terminates_descendants_and_carries_redacted_capture() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pid_file = dir.path().join("grandchild.pid");
    let start = Instant::now();
    let mut child = command("/bin/sh", dir.path());
    // Ignore SIGTERM in both leader and background grandchild so only the
    // supervisor's post-grace SIGKILL sweep ends the group.
    child.args([
        "-c",
        "trap '' TERM; sleep 30 & printf '%s' \"$!\" > \"$HOME/grandchild.pid\"; printf 'partial-out-secret'; printf 'partial-err-secret' >&2; wait",
    ]);
    let error = run_captured(
        child,
        Duration::from_millis(200),
        Cap::kill(1024),
        Cap::kill(1024),
    )
    .err()
    .expect("timeout");
    assert_eq!(error.code(), "process_timeout");
    assert!(start.elapsed() < Duration::from_secs(4));

    let display = error.to_string();
    let debug = format!("{error:?}");
    for secret in ["partial-out-secret", "partial-err-secret"] {
        assert!(
            !display.contains(secret),
            "Display leaked output: {display}"
        );
        assert!(!debug.contains(secret), "Debug leaked output: {debug}");
    }

    let Error::ProcessTimeout { capture } = error else {
        panic!("expected ProcessTimeout");
    };
    assert_eq!(capture.stdout.bytes, b"partial-out-secret");
    assert_eq!(capture.stderr.bytes, b"partial-err-secret");
    assert!(capture.stdout.complete && capture.stderr.complete);
    #[cfg(target_os = "linux")]
    assert!(capture.swept);

    let grandchild_pid: u32 = std::fs::read_to_string(&pid_file)
        .expect("grandchild pid")
        .trim()
        .parse()
        .expect("numeric pid");
    assert!(
        wait_until_gone(grandchild_pid, Duration::from_secs(2)),
        "grandchild {grandchild_pid} survived timeout sweep"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn finite_output_limit_carries_prefix_without_leaking_in_display_or_debug() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = command("/usr/bin/printf", dir.path());
    child.arg("secret-prefix-beyond-eight-bytes");
    let error = run_captured(child, Duration::from_secs(2), Cap::kill(8), Cap::kill(1024))
        .err()
        .expect("output limit");
    assert_eq!(error.code(), "process_output_limit");
    assert!(!error.to_string().contains("secret-p"));
    assert!(!format!("{error:?}").contains("secret-p"));

    let capture = error.process_capture().expect("captured prefix");
    assert_eq!(capture.stdout.bytes, b"secret-p");
    assert!(capture.stdout.truncated);

    // Explicit truncate policy keeps the first 8 bytes and lets the child finish.
    let mut truncated = command("/usr/bin/printf", dir.path());
    truncated.arg("a finite string longer than eight bytes");
    let output = run_captured(
        truncated,
        Duration::from_secs(2),
        Cap::truncate(8),
        Cap::kill(1024),
    )
    .expect("truncated run completes");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a finite");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn environment_allowlist_blocks_unlisted_variables() {
    let dir = tempfile::tempdir().expect("tempdir");

    // Snapshot filtering through env::Allow.
    let mut child = Command::new("/bin/sh");
    let snapshot = vec![
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("ALLOWED_VAR"), OsString::from("allowed-ok")),
        (
            OsString::from("FORBIDDEN_SECRET"),
            OsString::from("must-not-reach-child"),
        ),
        (OsString::from("ORBIT_OPERATOR"), OsString::from("1")),
    ];
    env::Allow::names(["PATH", "ALLOWED_VAR"]).apply_from(&mut child, snapshot);
    child.args([
        "-c",
        "printf 'allowed=%s|forbidden=%s|operator=%s' \"$ALLOWED_VAR\" \"$FORBIDDEN_SECRET\" \"$ORBIT_OPERATOR\"",
    ]);
    let (status, output) = run(
        child,
        dir.path(),
        Duration::from_secs(2),
        Cap::kill(1024),
        Cap::kill(1024),
    )
    .expect("run");
    assert!(status.success());
    assert_eq!(output, "allowed=allowed-ok|forbidden=|operator=");

    // Ambient filtering through env::allow: Cargo always sets CARGO_PKG_NAME,
    // which is not in the allowlist and must not reach the child.
    assert!(std::env::var_os("CARGO_PKG_NAME").is_some());
    let mut ambient = Command::new("/bin/sh");
    env::allow(&mut ambient, &["PATH"]);
    ambient.args(["-c", "printf 'pkg=%s' \"$CARGO_PKG_NAME\""]);
    let (status, output) = run(
        ambient,
        dir.path(),
        Duration::from_secs(2),
        Cap::kill(1024),
        Cap::kill(1024),
    )
    .expect("run");
    assert!(status.success());
    assert_eq!(output, "pkg=");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn clean_and_failed_child_exits_keep_their_actual_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = command("/usr/bin/printf", dir.path());
    child.arg("bounded output");
    let (status, output) = run(
        child,
        dir.path(),
        Duration::from_secs(2),
        Cap::kill(1024),
        Cap::kill(1024),
    )
    .expect("run");
    assert!(status.success());
    assert_eq!(output, "bounded output");

    let mut child = command("/bin/sh", dir.path());
    child.args(["-c", "exit 7"]);
    assert_eq!(
        run(
            child,
            dir.path(),
            Duration::from_secs(2),
            Cap::kill(1024),
            Cap::kill(1024),
        )
        .expect("run")
        .0
        .code(),
        Some(7)
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn captured_protocol_errors_remain_separate_from_stdout() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = command("/bin/sh", dir.path());
    child.args([
        "-c",
        "printf 'result'; printf '{\"code\":\"policy_denied\"}' >&2; exit 1",
    ]);
    let output = run_captured(
        child,
        Duration::from_secs(5),
        Cap::kill(512 * 1024),
        Cap::kill(512 * 1024),
    )
    .expect("run");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"result");
    assert_eq!(output.stderr, br#"{"code":"policy_denied"}"#);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn missing_program_keeps_not_found_distinguishable() {
    let mut child = Command::new("office-common-no-such-program");
    env::allow(&mut child, &[]);
    let error = run_captured(
        child,
        Duration::from_secs(2),
        Cap::kill(1024),
        Cap::kill(1024),
    )
    .err()
    .expect("missing program fails");
    assert_eq!(error.code(), "process_spawn_failed");
    let Error::ProcessSpawn { program, source } = error else {
        panic!("expected ProcessSpawn");
    };
    assert_eq!(program, Path::new("office-common-no-such-program"));
    assert_eq!(source.kind(), ErrorKind::NotFound);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn run_logged_captures_combined_log_across_exit_timeout_and_overflow() {
    let dir = tempfile::tempdir().expect("tempdir");

    let mut ok = command("/bin/sh", dir.path());
    ok.args(["-c", "printf 'out\\n'; printf 'err\\n' >&2"]);
    let logged = run_logged(ok, Duration::from_secs(2), Cap::kill(1024)).expect("logged ok");
    assert_eq!(logged.status.and_then(|s| s.code()), Some(0));
    assert!(!logged.timed_out);
    assert!(!logged.truncated);
    let text = String::from_utf8_lossy(&logged.bytes);
    assert!(text.contains("out\n") && text.contains("err\n"), "{text}");

    let mut slow = command("/bin/sh", dir.path());
    slow.args(["-c", "printf 'before-timeout'; sleep 30"]);
    let timed = run_logged(slow, Duration::from_millis(150), Cap::kill(1024)).expect("logged");
    assert!(timed.status.is_none());
    assert!(timed.timed_out);
    assert!(!timed.truncated);
    assert_eq!(timed.bytes, b"before-timeout");

    let mut noisy = command("/usr/bin/printf", dir.path());
    noisy.arg("0123456789abcdef-extra");
    let capped = run_logged(noisy, Duration::from_secs(2), Cap::kill(16)).expect("logged");
    assert!(capped.status.is_none());
    assert!(!capped.timed_out);
    assert!(capped.truncated);
    assert_eq!(capped.bytes, b"0123456789abcdef");
}
