"""Orbit callbacks (orbit_gateway.callbacks): the allowlist, the supervisor's
deadline, group sweep and output cap, and the outcome of a lost reply."""

from __future__ import annotations

import os
import time
from pathlib import Path

from tests.support import TOKEN, GatewayCase

from orbit_gateway import callbacks
from orbit_gateway.envelope import GatewayError
from orbit_gateway.redact import Redactor


class Callbacks(GatewayCase):
    def client(self, deadline: float = 5.0, **environ: str) -> callbacks.Orbit:
        env = {"ORBIT_BIN": str(self.orbit.program), "PATH": os.environ.get("PATH", "/usr/bin:/bin"), **environ}
        return callbacks.Orbit(env, self.root, Redactor([TOKEN]), deadline=deadline)

    def code(self, tool: str, payload: dict, **kwargs) -> str:
        with self.assertRaises(GatewayError) as caught:
            self.client(**kwargs).call(tool, payload)
        return caught.exception.code

    def test_only_allowlisted_tools_are_called(self) -> None:
        self.assertEqual(self.code("orbit.task.delete", {"id": "T-1"}), "orbit_tool_not_allowed")
        self.assertEqual(self.code("orbit.friction.add", {}), "orbit_tool_not_allowed")
        self.assertEqual(self.orbit.calls(), [])

    def test_the_input_is_redacted_and_the_result_returned(self) -> None:
        result = self.client().call("orbit.task.add", {"title": f"about {TOKEN}"})
        self.assertEqual(result, {"id": "T-100", "status": "proposed"})
        [call] = self.orbit.calls()
        self.assertEqual(call["input"], {"title": "about [secret]"})

    def test_a_structured_refusal_keeps_its_code(self) -> None:
        self.orbit.reply("orbit.task.update", stderr={"error": "no", "code": "invalid_input"}, exit=1)
        self.assertEqual(self.code("orbit.task.update", {"id": "T-1"}), "orbit_invalid_input")
        self.orbit.reply("orbit.task.update", stderr={"error": "lost", "code": "outcome_unknown"}, exit=1)
        self.assertEqual(self.code("orbit.task.update", {"id": "T-1"}), "orbit_outcome_unknown")

    def test_a_silent_callback_is_ended_at_the_deadline(self) -> None:
        self.orbit.reply("orbit.task.add", sleep=30)
        self.orbit.reply("orbit.task.show", sleep=30)
        started = time.monotonic()
        # A write that may have committed is unknown; a read is unavailable.
        self.assertEqual(self.code("orbit.task.add", {}, deadline=0.5), "orbit_outcome_unknown")
        self.assertEqual(self.code("orbit.task.show", {"id": "T-1"}, deadline=0.5), "orbit_unavailable")
        self.assertLess(time.monotonic() - started, 15)
        self.assertEqual(len(self.orbit.calls("orbit.task.add")), 1, "never retried")

    def test_output_past_the_limit_is_refused(self) -> None:
        self.orbit.reply("orbit.task.show", flood=callbacks.STREAM_LIMIT + 1)
        self.assertEqual(self.code("orbit.task.show", {"id": "T-1"}), "orbit_unavailable")

    def test_a_missing_orbit_is_unavailable(self) -> None:
        self.assertEqual(self.code("orbit.task.add", {}, ORBIT_BIN=str(self.root / "missing")), "orbit_unavailable")

    def test_whatever_the_callback_leaves_behind_is_swept(self) -> None:
        if not hasattr(os, "waitid"):
            self.skipTest("the group is swept after a clean exit only where os.waitid exists")
        marker = self.root / "orphan.pid"
        self.orbit.reply("orbit.task.show", stdout={"id": "T-1"}, orphan=str(marker))
        started = time.monotonic()
        self.assertEqual(self.client().call("orbit.task.show", {"id": "T-1"}), {"id": "T-1"})
        self.assertLess(time.monotonic() - started, 10)
        pid = int(marker.read_text())
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if not alive(pid):
                return
            time.sleep(0.05)
        self.fail(f"the callback's leftover process {pid} is still running")


def alive(pid: int) -> bool:
    """Running, not merely a zombie waiting for its new parent to reap it."""
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    try:
        stat = Path(f"/proc/{pid}/stat").read_text()
    except OSError:
        return True
    return stat.rsplit(")", 1)[1].split()[0] not in ("Z", "X")
