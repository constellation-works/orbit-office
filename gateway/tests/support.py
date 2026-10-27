"""Test support: an exported plugin, the real backend as a subprocess, and
loopback HTTP servers that record what they receive.

Tests never touch host state: each one gets a temp root for HOME and
`{{plugin_state}}`, set on the child's environment only.
"""

from __future__ import annotations

import json
import os
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


@dataclass
class Seen:
    method: str
    path: str
    headers: dict[str, str]


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
                seen = Seen(self.command, self.path, {key.lower(): value for key, value in self.headers.items()})
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
