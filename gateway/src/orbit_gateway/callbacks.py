"""Orbit plugin callbacks: `orbit tool run <tool> --json --full --input <json>`.

Orbit's callback mechanism (its plugin scope §4.2, and the Office adapter in
docs/design/office/specs/plugin-surface.md): the host hands the backend an
open descriptor (`ORBIT_PLUGIN_CALLBACK_FD`, normally 3) and authenticates
it on every callback against the manifest's `orbit_tools`, the grants and the
caller's ceiling. This module keeps that descriptor open across the child and
never reads it.

- An exact-name allowlist (`TOOLS`), checked before a process starts.
- A cleared environment carrying only `ENV_NAMES`, plus the non-secret
  `ORBIT_PLUGIN_CALLBACK=descriptor-required` marker, so a missing descriptor
  is refused by Orbit rather than treated as a person's CLI call.
- The child leads its own process group; a 30-second deadline ends it with
  SIGTERM, a grace period and SIGKILL; stdout and stderr are drained while it
  runs and capped at 512 KiB each (STD-03@2 §R11-R15).
- Never retried. A mutating callback whose reply is missing or unreadable may
  have committed, so it is `orbit_outcome_unknown`, not a failure (§R31).
- The input is redacted before it leaves the process (STD-05@1 §R13).

With `cloud`, the only module that spawns a process, and it spawns only Orbit.
"""

from __future__ import annotations

import json
import os
import re
import signal
import subprocess
import threading
import time
from collections.abc import Mapping
from pathlib import Path
from typing import IO, Any

from .envelope import GatewayError
from .redact import Redactor

TOOLS = ("orbit.task.add", "orbit.task.show", "orbit.task.update", "orbit.task.artifact.put")
MUTATING = ("orbit.task.add", "orbit.task.update", "orbit.task.artifact.put")
# Non-secret names the callback CLI needs; nothing else of the backend's
# environment reaches the child.
ENV_NAMES = (
    "HOME",
    "PATH",
    "LANG",
    "LC_ALL",
    "TMPDIR",
    "TZ",
    "ORBIT_ROOT",
    "ORBIT_REGISTRY_ROOT",
    "ORBIT_WORKSPACE",
    "ORBIT_WORKTREE_ROOT",
    "ORBIT_RUN_ID",
    "ORBIT_MANAGED_RUN_CONTEXT",
    "ORBIT_AGENT_NAME",
    "ORBIT_AGENT_MODEL",
    "ORBIT_SESSION_ID",
    "ORBIT_TASK_ID",
    "ORBIT_ACTIVE_TASK_ID",
    "ORBIT_SCRATCH_DIR",
    "ORBIT_STEP_INDEX",
    "ORBIT_TASK_ACTOR_KIND",
    "ORBIT_PLUGIN",
    "ORBIT_PLUGIN_CALLBACK_FD",
    "ORBIT_PLUGIN_BROKER",
)
DEADLINE_SECONDS = 30.0
GRACE_SECONDS = 2.0
STREAM_LIMIT = 512 * 1024
CODE = re.compile(r"^[a-z0-9_.]{1,64}$")


class Orbit:
    """Callbacks for one call. Construction performs no I/O."""

    def __init__(self, environ: Mapping[str, str], cwd: Path, redactor: Redactor, deadline: float = DEADLINE_SECONDS):
        # ORBIT_BIN names another orbit executable (tests use a recording fake).
        self.program = environ.get("ORBIT_BIN") or "orbit"
        self.cwd = cwd
        self.redactor = redactor
        self.deadline = deadline
        self.environment = {name: environ[name] for name in ENV_NAMES if environ.get(name)}
        self.environment["ORBIT_PLUGIN_CALLBACK"] = "descriptor-required"
        self.fd = _descriptor(environ)

    def call(self, tool: str, payload: dict[str, Any]) -> dict[str, Any]:
        """Run one allowlisted callback and return its JSON result."""
        if tool not in TOOLS:
            raise GatewayError("orbit_tool_not_allowed", f"the gateway does not call {tool!r}")
        text = json.dumps(self.redactor.value(payload), ensure_ascii=False, sort_keys=True)
        argv = [self.program, "tool", "run", tool, "--json", "--full", "--input", text]
        try:
            child = subprocess.Popen(
                argv,
                cwd=self.cwd,
                env=self.environment,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
                pass_fds=(self.fd,) if self.fd is not None else (),
            )
        except OSError as error:
            # Nothing ran, so nothing was written.
            raise GatewayError(
                "orbit_unavailable",
                f"could not run {self.program!r} for {tool}: {error.strerror or error}",
                retryable=True,
            ) from None
        status, stdout, stderr, overflow = _supervise(child, self.deadline)
        if status is None:
            return self._indeterminate(tool, f"{tool} did not answer in {self.deadline:.0f}s")
        if overflow:
            return self._indeterminate(tool, f"{tool} wrote more than {STREAM_LIMIT} bytes")
        if status != 0:
            code = _refusal(stderr)
            if code == "outcome_unknown":
                return self._indeterminate(tool, f"Orbit reported the outcome of {tool} as unknown")
            if code is not None:
                raise GatewayError(f"orbit_{code}" if not code.startswith("orbit_") else code, f"Orbit refused {tool} ({code})")
            return self._indeterminate(tool, f"{tool} exited {status} without a structured refusal")
        try:
            result = json.loads(stdout.decode("utf-8"))
        except (UnicodeDecodeError, ValueError):
            return self._indeterminate(tool, f"{tool} answered with something other than JSON")
        if not isinstance(result, dict):
            return self._indeterminate(tool, f"{tool} answered with something other than a JSON object")
        return result

    def _indeterminate(self, tool: str, message: str) -> dict[str, Any]:
        if tool in MUTATING:
            raise GatewayError(
                "orbit_outcome_unknown",
                f"{message}; it may have been applied. Check Orbit before trying again.",
            )
        raise GatewayError("orbit_unavailable", message, retryable=True)


def _descriptor(environ: Mapping[str, str]) -> int | None:
    """The inherited callback descriptor, when it is open; never read."""
    raw = environ.get("ORBIT_PLUGIN_CALLBACK_FD") or "3"
    try:
        fd = int(raw)
    except ValueError:
        return None
    if fd < 3:
        return None
    try:
        os.fstat(fd)
    except OSError:
        return None
    return fd


def _drain(stream: IO[bytes], sink: list[bytes], overflow: list[bool]) -> None:
    size = 0
    while True:
        chunk = stream.read(65536)
        if not chunk:
            return
        if size < STREAM_LIMIT:
            sink.append(chunk[: STREAM_LIMIT - size])
        size += len(chunk)
        if size > STREAM_LIMIT:
            overflow.append(True)


def _supervise(child: subprocess.Popen[bytes], deadline: float) -> tuple[int | None, bytes, bytes, bool]:
    """Wait for the child within `deadline`, draining both streams; sweep its group and reap it.

    The group is signalled only while its leader is unreaped, so its id cannot
    have been reused. Where `os.waitid` exists the exited leader is observed
    with WNOWAIT and the group swept before the reap; without it the group is
    signalled only on a timeout.
    Returns (exit status or None on timeout, stdout, stderr, overflowed).
    """
    out: list[bytes] = []
    err: list[bytes] = []
    overflow: list[bool] = []
    readers = [
        threading.Thread(target=_drain, args=(child.stdout, out, overflow), daemon=True),
        threading.Thread(target=_drain, args=(child.stderr, err, overflow), daemon=True),
    ]
    for reader in readers:
        reader.start()
    if hasattr(os, "waitid"):
        timed_out = not _exited(child.pid, time.monotonic() + deadline)
        if timed_out:
            _signal_group(child.pid, signal.SIGTERM)
            _exited(child.pid, time.monotonic() + GRACE_SECONDS)
        # Sweep: whatever is left in the group, the leader included, is
        # killed before the leader is reaped.
        _signal_group(child.pid, signal.SIGKILL)
        status = child.wait()
    else:
        # No waitid (macOS builds): the group is signalled only while a
        # timed-out leader is still unreaped, and never after.
        timed_out, status = _wait_reaping(child, deadline)
    for reader in readers:
        reader.join(timeout=GRACE_SECONDS)
    for stream in (child.stdout, child.stderr):
        if stream is not None:
            stream.close()
    return (None if timed_out else status), b"".join(out), b"".join(err), bool(overflow)


def _wait_reaping(child: subprocess.Popen[bytes], deadline: float) -> tuple[bool, int]:
    try:
        return False, child.wait(timeout=deadline)
    except subprocess.TimeoutExpired:
        pass
    _signal_group(child.pid, signal.SIGTERM)
    try:
        return True, child.wait(timeout=GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        _signal_group(child.pid, signal.SIGKILL)
        return True, child.wait()


def _exited(pid: int, until: float) -> bool:
    """True once `pid` has exited (left unreaped), False at `until`."""
    while True:
        try:
            if os.waitid(os.P_PID, pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None:
                return True
        except ChildProcessError:
            return True
        if time.monotonic() >= until:
            return False
        time.sleep(0.02)


def _signal_group(pgid: int, sig: signal.Signals) -> None:
    try:
        os.killpg(pgid, sig)
    except (ProcessLookupError, PermissionError):
        pass


def _refusal(stderr: bytes) -> str | None:
    """The machine code of a structured `{"error", "code"}` refusal, else None."""
    try:
        value = json.loads(stderr.decode("utf-8"))
    except (UnicodeDecodeError, ValueError):
        return None
    if not isinstance(value, dict) or not isinstance(value.get("error"), str):
        return None
    code = value.get("code")
    if isinstance(code, str) and CODE.match(code):
        return code
    return None
