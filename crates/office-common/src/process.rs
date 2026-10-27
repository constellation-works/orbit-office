//! Bounded synchronous subprocess supervision, shared by adapters and fixtures.
//! Stdout and stderr are drained concurrently into bounded in-memory captures.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::{Error, Result};

const OUTPUT_LIMIT: u64 = 512 * 1024; // two streams, 1 MiB total
const TIMEOUT: Duration = Duration::from_secs(30);

/// Bounded process captures. Do not embed raw child output in diagnostics.
pub struct CapturedOutput {
    /// Actual exit status, after the process group has been swept and reaped.
    pub status: ExitStatus,
    /// Captured standard output, within the selected stdout budget.
    pub stdout: Vec<u8>,
    /// Captured standard error, at most 512 KiB.
    pub stderr: Vec<u8>,
}

/// Run with the same bounds and lifecycle as [`run`], retaining both streams
/// for protocols whose machine-readable errors arrive on stderr.
#[cfg(unix)]
pub fn run_captured(command: &mut Command, root: &Path) -> Result<CapturedOutput> {
    run_captured_bounded(command, root, TIMEOUT, OUTPUT_LIMIT, OUTPUT_LIMIT)
}

#[cfg(not(unix))]
pub fn run_captured(_command: &mut Command, _root: &Path) -> Result<CapturedOutput> {
    Err(Error::ProcessUnsupported)
}

/// Capture a protocol whose encoded success reply needs a larger stdout
/// budget. Stderr retains the default 512 KiB bound, and the same 30-second
/// deadline, process-group cleanup and bounded drains apply. The adapter
/// chooses the budget from its protocol contract, never from caller input.
#[cfg(unix)]
pub fn run_captured_with_stdout_limit(
    command: &mut Command,
    root: &Path,
    stdout_limit: u64,
) -> Result<CapturedOutput> {
    run_captured_bounded(command, root, TIMEOUT, stdout_limit, OUTPUT_LIMIT)
}

/// Unsupported-platform counterpart of the bounded protocol capture.
#[cfg(not(unix))]
pub fn run_captured_with_stdout_limit(
    _command: &mut Command,
    _root: &Path,
    _stdout_limit: u64,
) -> Result<CapturedOutput> {
    Err(Error::ProcessUnsupported)
}

/// Run a command with a 30-second deadline, empty stdin, a fresh process group
/// and concurrent drains limited to 512 KiB per stream (1 MiB total).
///
/// The caller supplies a clean environment and a resolved root for diagnostics.
/// Returns the actual child exit status and UTF-8 stdout; stderr is drained and
/// discarded because embedding arbitrary child output in errors can leak secrets.
/// Error paths terminate/reap the process group. Linux and macOS are supported.
#[cfg(unix)]
pub fn run(command: &mut Command, root: &Path) -> Result<(ExitStatus, String)> {
    run_bounded(command, root, TIMEOUT, OUTPUT_LIMIT)
}

#[cfg(unix)]
pub(crate) fn run_bounded(
    command: &mut Command,
    root: &Path,
    timeout: Duration,
    output_limit: u64,
) -> Result<(ExitStatus, String)> {
    // Preserve run's established I/O error shape for existing adapters. The
    // captured protocol API exposes ProcessSpawn so callers can distinguish a
    // request never sent from one whose reply was lost.
    let output = run_captured_bounded(command, root, timeout, output_limit, output_limit).map_err(
        |error| match error {
            Error::ProcessSpawn { source, .. } => Error::io("start subprocess", root, source),
            other @ (Error::ProcessTimeout
            | Error::ProcessOutputLimit
            | Error::ProcessUnsupported
            | Error::UnsafePath { .. }
            | Error::Io { .. }) => other,
        },
    )?;
    let text = String::from_utf8(output.stdout)
        .map_err(|e| Error::io("decode subprocess output", root, std::io::Error::other(e)))?;
    Ok((output.status, text))
}

#[cfg(unix)]
fn run_captured_bounded(
    command: &mut Command,
    root: &Path,
    timeout: Duration,
    output_limit: u64,
    stderr_limit: u64,
) -> Result<CapturedOutput> {
    use std::os::unix::process::CommandExt;
    command
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().map_err(|source| Error::ProcessSpawn {
        program: command.get_program().into(),
        source,
    })?;
    let mut guard = ChildGuard {
        child,
        reaped: false,
    };
    let limited = Arc::new(AtomicBool::new(false));
    let stdout = guard.child.stdout.take().ok_or_else(|| {
        Error::io(
            "capture subprocess stdout",
            root,
            std::io::Error::other("pipe unavailable"),
        )
    })?;
    let stderr = guard.child.stderr.take().ok_or_else(|| {
        Error::io(
            "capture subprocess stderr",
            root,
            std::io::Error::other("pipe unavailable"),
        )
    })?;
    let mut out = Reader::start(stdout, Arc::clone(&limited), output_limit);
    let mut err = Reader::start(stderr, Arc::clone(&limited), stderr_limit);
    let deadline = Instant::now() + timeout;
    let outcome = (|| {
        loop {
            if limited.load(Ordering::Relaxed) {
                return Err(Error::ProcessOutputLimit);
            }
            if guard.exited()? {
                break;
            }
            if Instant::now() >= deadline {
                return Err(Error::ProcessTimeout);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Sweep while the leader is still waitable, before wait releases its PID.
        guard.signal_group(9);
        let status = guard
            .child
            .wait()
            .map_err(|e| Error::io("reap subprocess", root, e))?;
        guard.reaped = true;
        Ok(status)
    })();
    // Error cleanup terminates/reaps the whole group before joining pipe readers.
    drop(guard);
    let stdout = out.finish(root);
    let stderr = err.finish(root);
    let status = outcome?;
    let stdout = stdout?;
    let stderr = stderr?;
    if limited.load(Ordering::Relaxed) {
        return Err(Error::ProcessOutputLimit);
    }
    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(not(unix))]
pub fn run(_command: &mut Command, _root: &Path) -> Result<(ExitStatus, String)> {
    Err(Error::ProcessUnsupported)
}

/// Each stream reader owns at most its selected limit plus one byte and one result slot.
/// The producer waits if the bounded result slot is full (one send per reader).
struct Reader {
    receiver: mpsc::Receiver<std::io::Result<Vec<u8>>>,
    thread: Option<JoinHandle<()>>,
}

impl Reader {
    fn start(stream: impl Read + Send + 'static, limited: Arc<AtomicBool>, limit: u64) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream.take(limit + 1).read_to_end(&mut bytes);
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
                limited.store(true, Ordering::Relaxed);
            }
            let _ = sender.send(result.map(|_| bytes));
        });
        Self {
            receiver,
            thread: Some(thread),
        }
    }

    fn finish(&mut self, root: &Path) -> Result<Vec<u8>> {
        let result = self
            .receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|e| Error::io("drain subprocess pipe", root, std::io::Error::other(e)))?;
        // Receipt means the reader has finished its only I/O; joining cannot wait
        // on a peer or stream. A panic is reported as an unavailable capture.
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| {
                Error::io(
                    "join subprocess pipe reader",
                    root,
                    std::io::Error::other("reader panicked"),
                )
            })?;
        }
        result.map_err(|e| Error::io("read subprocess pipe", root, e))
    }
}

#[cfg(unix)]
struct ChildGuard {
    child: Child,
    reaped: bool,
}

#[cfg(unix)]
impl ChildGuard {
    fn signal_group(&self, signal: i32) {
        if let Ok(pid) = i32::try_from(self.child.id()) {
            // SAFETY: this guard owns an unreaped child, started as group leader.
            // Its PID cannot be reused until wait; a negative PID targets its group.
            unsafe {
                unix::kill(-pid, signal);
            }
        }
    }

    fn exited(&self) -> Result<bool> {
        let pid = self.child.id();
        // siginfo_t is opaque here: only si_signo (the first c_int on Linux/macOS)
        // is read. 1024 aligned bytes exceed both platforms' ABI size/alignment.
        let mut info = [0_u64; 128];
        // SAFETY: P_PID selects our owned, unreaped child. The buffer is writable,
        // zeroed and adequately sized/aligned. WNOWAIT leaves the child waitable,
        // avoiding PID reuse between the group sweep and Child::wait.
        let rc =
            unsafe { unix::waitid(1, pid, info.as_mut_ptr().cast(), unix::EXITED_NOHANG_NOWAIT) };
        if rc != 0 {
            return Err(Error::io(
                "observe subprocess child",
                Path::new("subprocess"),
                std::io::Error::last_os_error(),
            ));
        }
        // SAFETY: si_signo is a c_int at offset zero on the supported ABIs.
        Ok(unsafe { *info.as_ptr().cast::<i32>() } != 0)
    }
}

#[cfg(unix)]
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        self.signal_group(15);
        let grace = Instant::now() + Duration::from_millis(200);
        // Probe the group, not merely the leader. A zombie leader can keep the
        // group observable through the grace period; that is safely conservative.
        while Instant::now() < grace {
            if let Ok(pid) = i32::try_from(self.child.id()) {
                // SAFETY: the guard still owns the unreaped process-group leader.
                if unsafe { unix::kill(-pid, 0) } != 0 {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.signal_group(9);
        let _ = self.child.wait();
        self.reaped = true;
    }
}

// These ABIs are the workspace's Linux/macOS CI targets. No external crate is
// needed for the two POSIX calls missing from std. Unsupported Unix targets
// deliberately fail compilation rather than using guessed wait flags.
#[cfg(unix)]
mod unix {
    #[cfg(target_os = "linux")]
    pub(super) const EXITED_NOHANG_NOWAIT: i32 = 4 | 1 | 0x0100_0000;
    #[cfg(target_os = "macos")]
    pub(super) const EXITED_NOHANG_NOWAIT: i32 = 4 | 1 | 0x20;
    unsafe extern "C" {
        pub(super) fn kill(pid: i32, signal: i32) -> i32;
        pub(super) fn waitid(kind: u32, pid: u32, info: *mut std::ffi::c_void, options: i32)
        -> i32;
    }
}
