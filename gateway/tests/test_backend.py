"""bin/backend.py: the version check and the envelope."""

from __future__ import annotations

import ast
import json
import subprocess
import sys

from tests.support import CALL_TIMEOUT, TOKEN, GatewayCase

# Run the real backend file as __main__ with sys.version_info replaced, the way
# an older interpreter would present itself to it.
OLD_PYTHON = """
import runpy, sys
sys.version_info = (3, 11, 9, "final", 0)
sys.argv = [sys.argv[1]]
runpy.run_path(sys.argv[0], run_name="__main__")
"""


class VersionCheck(GatewayCase):
    def run_backend(self, prelude: str, request: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, "-I", "-c", prelude, str(self.backend)],
            input=request,
            capture_output=True,
            text=True,
            env={"HOME": str(self.home), "PATH": "/usr/bin:/bin", "ORBIT_PLUGIN_STATE": str(self.state)},
            cwd=self.root,
            timeout=CALL_TIMEOUT,
            check=False,
        )

    def test_an_older_interpreter_answers_python_too_old(self) -> None:
        request = json.dumps(
            {
                "schema_version": 1,
                "tool": "gateway.connection-list",
                "input": {},
                "context": {"secrets": {"conn_01": {"value": TOKEN, "version": "v"}}},
            }
        )
        done = self.run_backend(OLD_PYTHON, request)
        self.assertEqual(done.returncode, 0, done.stderr)
        answer = json.loads(done.stdout)
        self.assertEqual(answer["ok"], False)
        self.assertEqual(answer["error"]["code"], "python_too_old")
        self.assertIn("3.11.9", answer["error"]["message"])
        self.assertIn("orbit-office install", answer["error"]["message"])
        self.assertNotIn(TOKEN, done.stdout)
        self.assertFalse(self.state.exists(), "an old interpreter touches no state")

    def test_a_current_interpreter_serves(self) -> None:
        self.assertEqual(self.ok("connection-list", {}), {"connections": [], "free_slots": 16})

    def test_backend_parses_with_old_grammar(self) -> None:
        # Structural guard for the version check above: if backend.py used
        # newer syntax, an old interpreter would fail to parse it and never
        # reach the `python_too_old` answer.
        source = self.backend.read_text()
        ast.parse(source, feature_version=(3, 7))


class Envelope(GatewayCase):
    def raw(self, text: str) -> dict:
        done = subprocess.run(
            [sys.executable, str(self.backend)],
            input=text,
            capture_output=True,
            text=True,
            env={"HOME": str(self.home), "PATH": "/usr/bin:/bin", "ORBIT_PLUGIN_STATE": str(self.state)},
            timeout=CALL_TIMEOUT,
            check=False,
        )
        self.assertEqual((done.returncode, done.stderr), (0, ""))
        return json.loads(done.stdout)

    def test_bad_requests_are_error_envelopes(self) -> None:
        self.assertEqual(self.raw("not json")["error"]["code"], "invalid_request")
        self.assertEqual(self.raw("[]")["error"]["code"], "invalid_request")
        self.assertEqual(self.raw('{"schema_version": 2, "tool": "gateway.connection-list"}')["error"]["code"], "invalid_request")

    def test_an_unknown_tool_is_refused(self) -> None:
        answer = self.raw('{"schema_version": 1, "tool": "gateway.http-send", "input": {}, "context": {}}')
        self.assertEqual(answer["error"]["code"], "unknown_tool")

    def test_missing_plugin_state_is_refused(self) -> None:
        done = subprocess.run(
            [sys.executable, str(self.backend)],
            input='{"schema_version": 1, "tool": "gateway.connection-list", "input": {}, "context": {}}',
            capture_output=True,
            text=True,
            env={"HOME": str(self.home), "PATH": "/usr/bin:/bin"},
            timeout=CALL_TIMEOUT,
            check=False,
        )
        self.assertEqual(json.loads(done.stdout)["error"]["code"], "state_unavailable")
