"""Test support: an exported plugin, the real backend as a subprocess, a fake
`orbit` that records the backend's callbacks, and loopback HTTP servers that
record what they receive.

Tests never touch host state: each one gets a temp root for HOME and
`{{plugin_state}}`, set on the child's environment only.
"""

from __future__ import annotations

import json
import os
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from collections.abc import Callable
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any

GATEWAY = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(GATEWAY / "src"))
sys.path.insert(0, str(GATEWAY / "scripts"))

import export  # noqa: E402  (scripts/export.py)

TOKEN = "tok-9f8e7d6c5b4a39281706"
# Bounds every backend run (STD-03 §R22); a test that needs longer is wrong.
CALL_TIMEOUT = 60


class GatewayCase(unittest.TestCase):
    """A fresh export, state root and HOME per test."""

    def setUp(self) -> None:
        temp = tempfile.TemporaryDirectory(prefix="gateway-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        self.plugin = export.export("gateway", self.root / "plugin") / ".orbit-plugin"
        self.backend = self.plugin / "bin" / "backend.py"
        self.state = self.root / "state"
        self.home = self.root / "home"
        self.home.mkdir()
        self.orbit = FakeOrbit(self.root / "fake-orbit")

    def call(
        self,
        tool: str,
        payload: dict[str, Any] | None = None,
        secrets: dict[str, str] | None = None,
        context: dict[str, Any] | None = None,
        umask: int | None = None,
    ) -> dict[str, Any]:
        """Run one tool through the real backend, as Orbit does."""
        request = {
            "schema_version": 1,
            "tool": f"gateway.{tool}",
            "input": payload if payload is not None else {},
            "context": {
                "workspace_root": str(self.root),
                "agent": "claude",
                "model": "test-model",
                "task_id": "T-1",
                "job_run_id": "run-1",
                "secrets": {name: {"value": value, "version": "fixture"} for name, value in (secrets or {}).items()},
                **(context or {}),
            },
        }
        env = {
            "HOME": str(self.home),
            "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
            "ORBIT_PLUGIN_STATE": str(self.state),
            "ORBIT_PLUGIN_ROOT": str(self.plugin),
            "ORBIT_TOOL_NAME": f"gateway.{tool}",
            "ORBIT_BIN": str(self.orbit.program),
            "ORBIT_PLUGIN": "gateway",
            # Not forwarded to a callback: outside its environment allowlist.
            "GATEWAY_TEST_AMBIENT": "ambient",
        }
        argv = [sys.executable, str(self.backend)]
        if umask is not None:
            argv = ["/bin/sh", "-c", f'umask {umask:03o}; exec "$0" "$@"', *argv]
        done = subprocess.run(
            argv,
            input=json.dumps(request),
            capture_output=True,
            text=True,
            env=env,
            cwd=self.root,
            timeout=CALL_TIMEOUT,
            check=False,
        )
        self.assertEqual(done.returncode, 0, f"backend exited {done.returncode}: {done.stderr}")
        self.assertEqual(done.stderr, "", "the backend writes nothing to stderr")
        lines = done.stdout.splitlines()
        self.assertEqual(len(lines), 1, f"exactly one answer line, got {done.stdout!r}")
        self.last_stdout = done.stdout
        return json.loads(lines[0])

    def ok(self, tool: str, payload: dict[str, Any] | None = None, **kwargs: Any) -> dict[str, Any]:
        answer = self.call(tool, payload, **kwargs)
        self.assertTrue(answer.get("ok"), f"expected success, got {answer}")
        return answer["output"]

    def refused(self, code: str, tool: str, payload: dict[str, Any] | None = None, **kwargs: Any) -> dict[str, Any]:
        answer = self.call(tool, payload, **kwargs)
        self.assertFalse(answer.get("ok"), f"expected {code}, got {answer}")
        self.assertEqual(answer["error"]["code"], code, answer)
        return answer["error"]

    def put(
        self, name: str, base_url: str, scheme: str = "bearer", secrets: dict[str, str] | None = None, **fields: Any
    ) -> dict[str, Any]:
        auth = {"scheme": scheme}
        for key in ("header", "username"):
            if key in fields:
                auth[key] = fields.pop(key)
        payload = {"name": name, "base_url": base_url, "auth": auth, **fields}
        return self.ok("connection-put", payload, secrets=secrets)

    def audit_lines(self) -> list[dict[str, Any]]:
        lines: list[dict[str, Any]] = []
        for path in sorted((self.state / "audit").glob("*.jsonl")):
            lines.extend(json.loads(line) for line in path.read_text().splitlines() if line.strip())
        return lines

    def state_bytes(self) -> bytes:
        """Every byte the gateway persisted."""
        return b"".join(path.read_bytes() for path in sorted(self.state.rglob("*")) if path.is_file())


# ---------------------------------------------------------------------------
# Loopback servers
# ---------------------------------------------------------------------------


# ---------------------------------------------------------------------------
# A fake `orbit` for the backend's callbacks
# ---------------------------------------------------------------------------

FAKE_ORBIT = """#!{python} -I
import json, os, sys, time
LOG, REPLIES = {log!r}, {replies!r}
argv = sys.argv[1:]
tool = argv[2]
payload = json.loads(argv[argv.index("--input") + 1])
entry = {{"argv": argv[:6], "tool": tool, "input": payload, "cwd": os.getcwd(), "env": dict(os.environ)}}
if tool == "orbit.task.artifact.put":
    with open(payload["source_path"], encoding="utf-8") as handle:
        entry["artifact"] = handle.read()
    entry["artifact_mode"] = os.stat(payload["source_path"]).st_mode & 0o777
with open(LOG, "a", encoding="utf-8") as handle:
    handle.write(json.dumps(entry) + "\\n")
with open(REPLIES, encoding="utf-8") as handle:
    reply = json.load(handle).get(tool, {{}})
if reply.get("orphan"):
    import subprocess
    sleeper = subprocess.Popen(["sleep", "60"])  # keeps stdout open, left behind
    with open(reply["orphan"], "w", encoding="utf-8") as handle:
        handle.write(str(sleeper.pid))
if reply.get("mark"):
    # Another sender got there first: rewrite a draft file's state.
    with open(reply["mark"]["path"], encoding="utf-8") as handle:
        draft = json.load(handle)
    draft["state"] = reply["mark"]["state"]
    with open(reply["mark"]["path"], "w", encoding="utf-8") as handle:
        json.dump(draft, handle)
if reply.get("flood"):
    sys.stdout.write("x" * reply["flood"])
    sys.exit(0)
if reply.get("sleep"):
    time.sleep(reply["sleep"])
if "stderr" in reply:
    sys.stderr.write(json.dumps(reply["stderr"]))
    sys.exit(reply.get("exit", 1))
sys.stdout.write(json.dumps(reply.get("stdout", {{}})))
"""


class FakeOrbit:
    """An `orbit` executable (ORBIT_BIN) that logs each callback and answers
    from `replies`: per tool, `stdout` (a JSON result), or `stderr` and `exit`
    (a structured refusal), optionally after `sleep` seconds; `flood` writes
    that many bytes, `orphan` leaves a `sleep` behind, its pid in a file, and
    `mark` rewrites a draft file's state as a concurrent sender would."""

    def __init__(self, directory: Path):
        directory.mkdir()
        self.log = directory / "calls.jsonl"
        self.replies_file = directory / "replies.json"
        self.program = directory / "orbit"
        self.program.write_text(FAKE_ORBIT.format(python=sys.executable, log=str(self.log), replies=str(self.replies_file)))
        self.program.chmod(self.program.stat().st_mode | stat.S_IXUSR)
        self.replies: dict[str, dict[str, Any]] = {
            "orbit.task.add": {"stdout": {"id": "T-100", "status": "proposed"}},
            "orbit.task.artifact.put": {"stdout": {"id": "T-100"}},
            "orbit.task.show": {"stdout": {"id": "T-100", "status": "proposed", "terminal": False}},
            "orbit.task.update": {"stdout": {"id": "T-100", "status": "done"}},
        }
        self.save()

    def save(self) -> None:
        self.replies_file.write_text(json.dumps(self.replies))

    def reply(self, tool: str, **reply: Any) -> None:
        self.replies[tool] = reply
        self.save()

    def calls(self, tool: str | None = None) -> list[dict[str, Any]]:
        if not self.log.exists():
            return []
        entries = [json.loads(line) for line in self.log.read_text().splitlines() if line.strip()]
        return [entry for entry in entries if tool is None or entry["tool"] == tool]


@dataclass
class Seen:
    method: str
    path: str
    headers: dict[str, str]
    body: bytes = b""


@dataclass
class Reply:
    status: int = 200
    body: bytes = b""
    headers: dict[str, str] = field(default_factory=dict)


Route = Callable[[Seen], Reply]


class Server:
    """A loopback HTTP server: `routes` maps a path (without query) to a reply."""

    def __init__(self, case: unittest.TestCase, routes: dict[str, Route | Reply] | None = None):
        self.routes: dict[str, Route | Reply] = dict(routes or {})
        self.seen: list[Seen] = []
        owner = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def _serve(self) -> None:
                length = int(self.headers.get("Content-Length") or 0)
                body = self.rfile.read(length) if length else b""
                seen = Seen(self.command, self.path, {key.lower(): value for key, value in self.headers.items()}, body)
                owner.seen.append(seen)
                route = owner.routes.get(self.path.split("?", 1)[0], Reply(404, b"not found", {"Content-Type": "text/plain"}))
                reply = route(seen) if callable(route) else route
                self.send_response(reply.status)
                for key, value in reply.headers.items():
                    self.send_header(key, value)
                self.send_header("Content-Length", str(len(reply.body)))
                self.end_headers()
                if self.command != "HEAD":
                    self.wfile.write(reply.body)

            do_GET = do_HEAD = do_POST = do_PUT = do_PATCH = do_DELETE = _serve

            def log_message(self, *args: Any) -> None:
                pass

        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.httpd.daemon_threads = True
        self.thread = threading.Thread(target=self.httpd.serve_forever, kwargs={"poll_interval": 0.05}, daemon=True)
        self.thread.start()
        case.addCleanup(self.close)

    @property
    def origin(self) -> str:
        return f"http://127.0.0.1:{self.httpd.server_address[1]}"

    def close(self) -> None:
        self.httpd.shutdown()
        self.httpd.server_close()
        self.thread.join(timeout=5)


def json_reply(value: Any, status: int = 200, **headers: str) -> Reply:
    return Reply(status, json.dumps(value).encode(), {"Content-Type": "application/json", **headers})


class SilentServer:
    """A loopback server that accepts each request, reads it and never answers."""

    def __init__(self, case: unittest.TestCase):
        self.listener = socket.create_server(("127.0.0.1", 0))
        self.listener.settimeout(0.05)
        self.requests: list[bytes] = []
        self.held: list[socket.socket] = []
        self.stop = threading.Event()
        self.thread = threading.Thread(target=self._serve, daemon=True)
        self.thread.start()
        case.addCleanup(self.close)

    def _serve(self) -> None:
        while not self.stop.is_set():
            try:
                conn, _ = self.listener.accept()
            except (TimeoutError, OSError):
                continue
            conn.settimeout(2)
            data = b""
            try:
                while b"\r\n\r\n" not in data:
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
            except OSError:
                pass
            self.requests.append(data)
            self.held.append(conn)  # held open, never answered

    @property
    def origin(self) -> str:
        return f"http://127.0.0.1:{self.listener.getsockname()[1]}"

    def close(self) -> None:
        self.stop.set()
        self.thread.join(timeout=5)
        for conn in self.held:
            conn.close()
        self.listener.close()
