"""Bounded local Orbit children; retain the leader until its group is swept."""
from __future__ import annotations

import json
import os
import re
import select
import selectors
import signal
import subprocess
import time

LIMIT = 512 * 1024


def signal_group(pid: int, sig: int) -> None:
    """Signal a child's process group, ignoring a group that is already gone.

    macOS answers EPERM, not ESRCH, when the only member left is the unreaped
    leader, so both mean nothing is left to signal.
    """
    try:
        os.killpg(pid, sig)
    except (ProcessLookupError, PermissionError):
        pass


class Failure(Exception):
    """A fixed diagnostic safe to log (never upstream text or input)."""


class NotSent(Failure):
    """Startup refused before the child could send a mutating request."""


_CODE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}\Z")


def _error_code(value) -> str | None:
    """The structured `error.code` of an error envelope, and nothing else.

    The envelope may be the tool reply itself or the `result` of the exec
    response envelope. Only a token-shaped code is surfaced: upstream
    message text, detail and the echoed input never reach a Failure.
    """
    if not isinstance(value, dict):
        return None
    error = value.get("error")
    if isinstance(error, dict):
        code = error.get("code")
        if isinstance(code, str) and _CODE.fullmatch(code):
            return code
    result = value.get("result")
    if isinstance(result, dict):
        return _error_code(result)
    return None


def run(argv: list[str], cwd: str, timeout: float) -> dict:
    # Orbit is the trusted installed client, not a shell or inference process.
    names = ("HOME", "PATH", "LANG", "LC_ALL", "TMPDIR", "TZ", "ORBIT_ROOT",
             "ORBIT_REGISTRY_ROOT", "ORBIT_WORKSPACE", "ORBIT_WORKTREE_ROOT", "ORBIT_RUN_ID",
             "ORBIT_MANAGED_RUN_CONTEXT", "ORBIT_AGENT_NAME", "ORBIT_AGENT_MODEL", "ORBIT_SESSION_ID",
             "ORBIT_TASK_ID", "ORBIT_ACTIVE_TASK_ID", "ORBIT_SCRATCH_DIR", "ORBIT_STEP_INDEX",
             "ORBIT_TASK_ACTOR_KIND", "ORBIT_PLUGIN", "ORBIT_PLUGIN_CALLBACK_FD", "ORBIT_PLUGIN_BROKER",
             "ORBIT_PLUGIN_CALLBACK")
    env = {key: os.environ[key] for key in names if key in os.environ}
    descriptors = ()
    if env.get("ORBIT_PLUGIN_CALLBACK") == "descriptor-required" and "ORBIT_PLUGIN_CALLBACK_FD" not in env:
        raise NotSent("callback_descriptor_unavailable")
    if "ORBIT_PLUGIN_CALLBACK_FD" in env:
        try:
            descriptor = int(env["ORBIT_PLUGIN_CALLBACK_FD"])
            if descriptor < 3:
                raise ValueError()
            os.fstat(descriptor)
        except (OSError, ValueError):
            raise NotSent("callback_descriptor_unavailable") from None
        descriptors = (descriptor,)
        env["ORBIT_PLUGIN_CALLBACK"] = "descriptor-required"
    if not hasattr(os, "waitid") and not hasattr(select, "kqueue"):
        raise NotSent("process_exit_observer_unavailable")
    observer = None
    try:
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True, pass_fds=descriptors)
    except OSError:
        raise NotSent("orbit_unavailable; request_not_sent") from None
    streams = selectors.DefaultSelector()
    output = bytearray()
    size = {"stdout": 0, "stderr": 0}
    deadline = time.monotonic() + timeout
    failed = False
    try:
        if not hasattr(os, "waitid"):
            observer = select.kqueue()
            observer.control([select.kevent(child.pid, filter=select.KQ_FILTER_PROC,
                flags=select.KQ_EV_ADD | select.KQ_EV_ONESHOT, fflags=select.KQ_NOTE_EXIT)], 0, 0)
        exited = False
        for pipe, name in ((child.stdout, "stdout"), (child.stderr, "stderr")):
            os.set_blocking(pipe.fileno(), False)
            streams.register(pipe, selectors.EVENT_READ, name)
        # Linux uses waitid(WNOWAIT); macOS Python builds without it use kqueue.
        # No poll()/wait() before the final group sweep: no recycled PID signal.
        while True:
            for key, _ in streams.select(timeout=0.01):
                chunk = os.read(key.fd, 65536)
                if not chunk:
                    streams.unregister(key.fileobj)
                    continue
                size[key.data] += len(chunk)
                if key.data == "stdout":
                    output.extend(chunk[:max(0, LIMIT - len(output))])
            if hasattr(os, "waitid"):
                exited = os.waitid(os.P_PID, child.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None
            elif not exited:
                exited = bool(observer.control(None, 1, 0))
            if exited and not streams.get_map():
                break
            if time.monotonic() >= deadline or max(size.values()) > LIMIT:
                failed = True
                signal_group(child.pid, signal.SIGTERM)
                grace = time.monotonic() + 0.1
                while time.monotonic() < grace:
                    time.sleep(0.01)
                break
    except BaseException:
        # Cancellation and unexpected faults get the same polite termination
        # before the final sweep; the leader remains unreaped throughout.
        signal_group(child.pid, signal.SIGTERM)
        time.sleep(0.1)
        raise
    finally:
        signal_group(child.pid, signal.SIGKILL)
        child.wait(timeout=1)
        streams.close()
        if observer is not None:
            observer.close()
        child.stdout.close()
        child.stderr.close()
    if failed:
        raise Failure("orbit_timeout_or_output_limit; mutation outcome unknown")
    # A non-zero exit still carries the tool's error envelope on stdout:
    # decode it before reducing the call to a generic failure, so the
    # structured code survives whichever exit the child chose.
    try:
        value = json.loads(output)
    except (ValueError, UnicodeDecodeError):
        if child.returncode != 0:
            raise Failure("orbit_call_failed; mutation outcome may be unknown") from None
        raise Failure("orbit_invalid_reply; mutation outcome unknown") from None
    if not isinstance(value, dict):
        if child.returncode != 0:
            raise Failure("orbit_call_failed; mutation outcome may be unknown")
        raise Failure("orbit_invalid_reply; mutation outcome unknown")
    code = _error_code(value)
    if code is not None:
        raise Failure(code)
    if child.returncode != 0:
        raise Failure("orbit_call_failed; mutation outcome may be unknown")
    if value.get("ok") is False or value.get("status") in ("error", "failed") or value.get("error"):
        raise Failure("orbit_tool_refused")
    # `--json --full` returns the tool object; tolerate the exec response envelope.
    if "schema_version" in value and "result" in value:
        value = value["result"]
    if not isinstance(value, dict):
        raise Failure("orbit_invalid_reply")
    return value
