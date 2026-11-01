"""Behavior through the shipped hooks and MCP entrypoints; no real host state."""
import fcntl
import json
import os
from pathlib import Path
import shlex
import stat
import subprocess
import sys
import tempfile
import time
import unittest

PLUGIN = Path(__file__).resolve().parents[1]
SCRATCH = PLUGIN.parents[1] / ".orbit/tmp"

STUB = r'''#!/usr/bin/env python3
import hashlib, json, os, pathlib, sys, time
home = pathlib.Path.home()
args = sys.argv
payload = json.loads(args[args.index('--input') + 1])
tool = args[3]
mode = (home / 'stub-mode').read_text() if (home / 'stub-mode').exists() else ''
if mode == 'hang':
    time.sleep(10)
if mode == 'failure':
    print('Bearer secret-value-do-not-log', file=sys.stderr)
    sys.exit(1)
with (home / 'calls.jsonl').open('a') as log:
    log.write(json.dumps({'tool': tool, 'input': payload, 'workspace': args[args.index('--workspace')+1]})+'\n')
if tool == 'orbit.task.add':
    descriptor = json.loads(payload['description'].split('Initial descriptor: ')[1].split('\n')[0])
    path = pathlib.Path(descriptor)
    value = json.loads(path.read_text())
    data = pathlib.Path(value['transcript_path']).read_bytes()
    assert value['transcript_sha256'] == hashlib.sha256(data).hexdigest()
    assert value['byte_length'] == len(data)
    assert path.stat().st_mode & 0o777 == 0o600
    assert pathlib.Path(value['transcript_path']).stat().st_mode & 0o777 == 0o600
    count = len([r for r in (home / 'calls.jsonl').read_text().splitlines() if json.loads(r)['tool']=='orbit.task.add'])
    print(json.dumps({'id': 'TEST-' + str(count), 'status': 'proposed'}))
elif tool == 'orbit.task.show':
    status = (home / 'task-status').read_text() if (home / 'task-status').exists() else 'in-progress'
    print(json.dumps({'id': payload['id'], 'status': status}))
elif tool == 'office.recall':
    print((home / 'recall.json').read_text())
elif tool == 'office.capture':
    if 'transcript_sha256' in payload and 'selections' not in payload:
        print(json.dumps({'ok': False, 'error': {'code': 'invalid_argument'}}))
        sys.exit(0)
    print(json.dumps({'source_id': 'src-fixture', 'title': payload.get('title')}))
else:
    print(json.dumps({'sections': [], 'pending': []}))
'''


class PluginBehavior(unittest.TestCase):
    def setUp(self):
        SCRATCH.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=SCRATCH, prefix="scribe-tests-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.home = self.root / "home"
        self.home.mkdir(mode=0o700)
        self.workspace = self.root / "office"
        self.workspace.mkdir(mode=0o700)
        self.project = self.root / "project"
        self.project.mkdir()
        self.providers = self.home / ".claude/projects/project"
        self.providers.mkdir(parents=True)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.bin / "orbit").write_text(STUB)
        (self.bin / "orbit").chmod(0o700)
        self.env = {"HOME": str(self.home), "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                    "OFFICE_WORKSPACE": str(self.workspace), "OFFICE_LIBRARY": str(self.root / "unused-library"),
                    "CLAUDE_PLUGIN_ROOT": str(PLUGIN), "CLAUDE_PLUGIN_OPTION_WORKSPACE": str(self.workspace),
                    "CLAUDE_PLUGIN_OPTION_DEFAULT_ON": "true", "CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS": "[]",
                    "CLAUDE_PLUGIN_OPTION_TIMEOUT": "2"}
        self.sid = "session-one"
        self.path = self.providers / (self.sid + ".jsonl")
        self.write_transcript(1)
        self.context = {"session_id": self.sid, "cwd": str(self.project), "transcript_path": str(self.path)}

    def write_transcript(self, turns, path=None, sid=None, project=None, date="2026-10-30"):
        path = path or self.path
        sid = sid or self.sid
        project = project or self.project
        data = b""
        for n in range(turns):
            record = {"type": "user", "sessionId": sid, "cwd": str(project), "timestamp": date + "T00:00:00Z",
                      "message": {"role": "user", "content": "Note: Decision: use local evidence " + str(n)}}
            data += json.dumps(record).encode() + b"\n"
        path.write_bytes(data)
        return data

    def hook(self, event, context=None):
        hooks = json.loads((PLUGIN / "hooks/hooks.json").read_text())["hooks"]
        command = hooks[event][0]["hooks"][0]["command"]
        command = command.replace("${CLAUDE_PLUGIN_ROOT}", str(PLUGIN))
        return subprocess.run(shlex.split(command), input=json.dumps(context or self.context), text=True,
                              env=self.env, cwd=self.root, capture_output=True, timeout=5)

    def mcp(self, *messages):
        data = "".join(json.dumps(m) + "\n" for m in messages)
        process = subprocess.run([sys.executable, "-I", str(PLUGIN / "scripts/bridge.py")], input=data,
                                 text=True, env=self.env, cwd=self.root, capture_output=True, timeout=35)
        self.assertEqual(process.returncode, 0, process.stderr)
        return [json.loads(line) for line in process.stdout.splitlines()]

    def command(self, action, **args):
        result = self.mcp({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "scribe", "arguments": {"session_id": self.sid, "action": action, **args}}})[0]["result"]
        self.assertFalse(result["isError"], result)
        return json.loads(result["content"][0]["text"])

    def calls(self, tool=None):
        path = self.home / "calls.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return [r for r in records if not tool or r["tool"] == tool]

    def finish_pass(self):
        (self.home / "task-status").write_text("done")

    def test_start_brief_stop_debounce_switch_now_and_final(self):
        start = self.hook("SessionStart")
        self.assertEqual(start.returncode, 0, start.stderr)
        self.assertEqual(json.loads(start.stdout)["hookSpecificOutput"]["hookEventName"], "SessionStart")
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.finish_pass()
        self.write_transcript(2)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        self.assertTrue(self.command("now")["queued"])
        self.command("off")
        self.write_transcript(3)
        before = len(list((self.workspace / ".orbit/tmp/session-transcripts").glob("*.json")))
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(list((self.workspace / ".orbit/tmp/session-transcripts").glob("*.json"))), before)
        end = self.hook("SessionEnd")
        self.assertEqual(end.returncode, 0, end.stderr)
        self.assertEqual(len(self.calls("orbit.task.add")), 3)
        call = self.calls("orbit.task.add")[-1]
        self.assertEqual(call["input"]["complexity"], "low")
        self.assertNotIn("crew", call["input"])
        self.assertEqual(call["workspace"], str(self.workspace))
        self.assertEqual(len(call["input"]["requires_labels"]), 1)

    def test_pending_pass_blocks_now_and_lock_contention_fails_open(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.write_transcript(10)
        self.assertEqual(self.command("now")["reason"], "pass_pending")
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        lock = self.workspace / (".orbit/tmp/scribe/" + self.sid + ".lock")
        with lock.open("r+") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            result = self.hook("Stop")
            self.assertEqual(result.returncode, 0)
            self.assertIn("session_busy", result.stderr)

    def test_max_turns_and_interval_and_unchanged_dedupe(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.finish_pass()
        self.write_transcript(9)
        self.hook("Stop")
        self.assertEqual(len(self.calls("orbit.task.add")), 2)
        self.write_transcript(10)
        state_path = self.workspace / (".orbit/tmp/scribe/" + self.sid + ".json")
        state = json.loads(state_path.read_text())
        state["last_queued_at"] -= 301
        state_path.write_text(json.dumps(state))
        self.hook("Stop")
        self.hook("Stop")
        self.assertEqual(len(self.calls("orbit.task.add")), 3)

    def test_complete_record_snapshot_and_private_modes(self):
        original = self.path.read_bytes()
        self.path.write_bytes(original + b'{"type":"user"')
        result = self.hook("Stop")
        self.assertEqual(result.returncode, 0, result.stderr)
        descriptors = list((self.workspace / ".orbit/tmp/session-transcripts").glob("*.json"))
        self.assertEqual(len(descriptors), 1)
        descriptor = json.loads(descriptors[0].read_text())
        self.assertEqual(Path(descriptor["transcript_path"]).read_bytes(), original)
        self.assertEqual(descriptor["mode"], "live")
        for path in [descriptors[0], Path(descriptor["transcript_path"])]:
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(descriptors[0].parent.stat().st_mode), 0o700)

    def test_exclusions_cover_descendants_symlinks_transcript_cwd_and_backfill(self):
        self.env["CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS"] = json.dumps([str(self.project)])
        alias = self.root / "alias"
        alias.symlink_to(self.project, target_is_directory=True)
        self.context["cwd"] = str(alias)
        for event in ("SessionStart", "Stop", "SessionEnd"):
            self.assertEqual(self.hook(event).returncode, 0)
        self.assertFalse((self.workspace / ".orbit/tmp/session-transcripts").exists())
        self.assertEqual(self.command("backfill")["queued_count"], 0)
        self.assertEqual(self.calls(), [])
        # A nonexcluded hook cannot smuggle an excluded transcript cwd.
        allowed = self.root / "allowed"
        allowed.mkdir()
        self.context["cwd"] = str(allowed)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(self.calls(), [])

    def test_backfill_since_oldest_first_and_rerun_noops(self):
        old = self.providers / "old.jsonl"
        self.write_transcript(1, path=old, sid="old", date="2025-01-01")
        newer = self.providers / "newer.jsonl"
        self.write_transcript(1, path=newer, sid="newer", date="2026-10-31")
        self.assertEqual(self.command("backfill", since="2026-01-01")["queued_count"], 2)
        calls = self.calls("orbit.task.add")
        self.assertIn('"session-one"', calls[0]["input"]["description"])
        self.assertIn('"newer"', calls[1]["input"]["description"])
        self.assertEqual(self.command("backfill", since="2026-01-01")["queued_count"], 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 2)

    def test_title_is_structured_user_capture(self):
        self.hook("SessionStart")
        title = 'A user title with $(touch forbidden)'
        self.command("title", title=title)
        payload = self.calls("office.capture")[-1]["input"]
        self.assertEqual(payload["title"], title)
        self.assertEqual(payload["title_by"], "user")
        self.assertNotIn("selections", payload, "setting a title must not advance the cursor past unselected evidence")
        self.assertFalse((self.root / "forbidden").exists())

    def test_failed_orbit_hook_is_prompt_zero_and_no_secret(self):
        self.env["CLAUDE_PLUGIN_OPTION_TIMEOUT"] = "0.25"
        for mode in ("failure", "hang"):
            (self.home / "stub-mode").write_text(mode)
            began = time.monotonic()
            result = self.hook("SessionStart")
            self.assertEqual(result.returncode, 0)
            self.assertLess(time.monotonic() - began, 2)
            self.assertIn("office-scribe:", result.stderr)
            self.assertNotIn("secret-value", result.stderr + result.stdout)

    def test_unknown_queue_result_preserves_descriptor_without_retry(self):
        (self.home / "stub-mode").write_text("failure")
        result = self.hook("Stop")
        self.assertEqual(result.returncode, 0)
        self.assertIn("orbit_call_failed", result.stderr)
        (self.home / "stub-mode").unlink()
        again = self.hook("Stop")
        self.assertEqual(again.returncode, 0)
        self.assertIn("queue_outcome_unknown", again.stderr)
        self.assertEqual(self.calls("orbit.task.add"), [])
        self.assertEqual(len(list((self.workspace / ".orbit/tmp/session-transcripts").glob("*.json"))), 1)

    def test_final_complete_json_record_without_newline_is_captured(self):
        self.path.write_bytes(self.path.read_bytes().rstrip(b"\n"))
        result = self.hook("SessionEnd")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        self.assertEqual(self.command("backfill")["queued_count"], 0)

    def test_window_open_heartbeat_tightens_turn_debounce(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.finish_pass()
        # Two new turns inside the time window: debounced at the default
        # bound of eight, queued at the window-open bound of two.
        self.write_transcript(3)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        pane = subprocess.run(
            [sys.executable, "-I", str(PLUGIN / "scripts/pane.py"),
             json.dumps({"session_id": self.sid, "action": "heartbeat"})],
            text=True, env=self.env, cwd=self.root, capture_output=True, timeout=5)
        self.assertEqual(pane.returncode, 0, pane.stderr)
        self.assertEqual(json.loads(pane.stdout), {"window_open": True})
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 2)

    def test_tool_results_are_not_new_user_turns_for_debounce(self):
        self.hook("Stop")
        self.finish_pass()
        with self.path.open("ab") as stream:
            for index in range(8):
                record = {"type": "user", "sessionId": self.sid, "cwd": str(self.project),
                    "timestamp": "2026-10-30T00:00:00Z", "message": {"role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": str(index), "content": "tool output"}]}}
                stream.write(json.dumps(record).encode() + b"\n")
        result = self.hook("Stop")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls("orbit.task.add")), 1)

    def test_missing_scoped_callback_never_falls_back_to_user_authority(self):
        self.env["ORBIT_PLUGIN_CALLBACK"] = "descriptor-required"
        result = self.hook("SessionStart")
        self.assertEqual(result.returncode, 0)
        self.assertIn("callback_descriptor_unavailable", result.stderr)
        self.assertEqual(self.calls(), [])
        self.hook("Stop")
        self.env.pop("ORBIT_PLUGIN_CALLBACK")
        result = self.hook("Stop")
        self.assertEqual(result.stderr, "")
        self.assertEqual(len(self.calls("orbit.task.add")), 1, "an unsent request must remain retryable")

    def test_status_is_read_only_and_irrelevant_fields_are_refused(self):
        status = self.command("status")
        self.assertTrue(status["on"])
        self.assertFalse((self.workspace / ".orbit").exists())
        response = self.mcp({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "scribe", "arguments": {"session_id": self.sid, "action": "status", "title": "ignored?"}}})[0]
        self.assertTrue(response["result"]["isError"])
        self.assertFalse((self.workspace / ".orbit").exists())

    def test_final_pass_is_queued_behind_an_outstanding_live_pass(self):
        self.hook("Stop")
        result = self.hook("SessionEnd")
        self.assertEqual(result.returncode, 0, result.stderr)
        calls = self.calls("orbit.task.add")
        self.assertEqual(len(calls), 2)
        self.assertEqual(calls[-1]["input"]["relations"], [{"type": "blocked_by", "target": "TEST-1"}])
        self.hook("SessionEnd")
        self.assertEqual(len(self.calls("orbit.task.add")), 2)

    def test_backfill_incomplete_file_does_not_hide_other_sessions(self):
        self.path.write_bytes(b'{"type":"user"')
        good = self.providers / "good.jsonl"
        self.write_transcript(1, path=good, sid="good")
        result = self.command("backfill")
        self.assertEqual(result["queued_count"], 1)
        self.assertEqual(result["refused_count"], 1)

    def test_mcp_recall_answers_anchors_metadata_live_refs_and_empty_coverage(self):
        data = {"entries": [{"source_id": "src-fixture", "anchor": "#L1", "kind": "decision",
            "text": "Decision: stay local", "time": "2026-10-30T00:00:00Z", "refs": [{"kind": "task", "id": "TEST-1"}]}],
            "sessions": [{"source_id": "src-fixture", "title": "Local memory", "project": str(self.project),
                "started_at": "2026-10-30T00:00:00Z", "resume": {"provider": "claude", "command": "claude --resume session-one"}}],
            "refs": [{"kind": "task", "id": "TEST-1", "status": "done", "workspace": None}],
            "coverage": {"earliest_session_at": "2026-10-30T00:00:00Z", "private_sessions_skipped": 0, "providers_not_captured": ["codex"]}}
        (self.home / "recall.json").write_text(json.dumps(data))
        messages = [{"jsonrpc": "2.0", "id": 1, "method": "initialize"},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "office.recall", "arguments": {"question": "local"}}}]
        response = self.mcp(*messages)
        self.assertEqual(len(response), 3)
        self.assertEqual({t["name"] for t in response[1]["result"]["tools"]}, {"office.recall", "scribe"})
        answer = json.loads(response[2]["result"]["content"][0]["text"])["evidence_answer"]
        for text in ("src-fixture#L1", "Local memory", str(self.project), "2026-10-30", "claude --resume session-one", "done"):
            self.assertIn(text, answer)
        self.assertEqual([c["tool"] for c in self.calls()], ["office.recall"])
        data["entries"] = []
        data["sessions"] = []
        (self.home / "recall.json").write_text(json.dumps(data))
        response = self.mcp(messages[-1])[0]
        answer = json.loads(response["result"]["content"][0]["text"])["evidence_answer"]
        self.assertIn("not captured", answer)
        self.assertIn('"providers_not_captured": ["codex"]', answer)
        self.assertFalse((self.workspace / ".orbit").exists())

    def test_symlink_state_and_uncontained_transcripts_are_refused(self):
        self.hook("SessionStart")
        staging = self.workspace / ".orbit/tmp/session-transcripts"
        staging.rmdir()
        outside = self.root / "outside"
        outside.mkdir()
        staging.symlink_to(outside, target_is_directory=True)
        result = self.hook("Stop")
        self.assertEqual(result.returncode, 0)
        self.assertIn("unsafe_state_directory", result.stderr)
        self.assertEqual(list(outside.iterdir()), [])

    def status_result(self):
        return self.mcp({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {
            "name": "scribe", "arguments": {"session_id": self.sid, "action": "status"}}})[0]["result"]

    def test_orbit_created_tmp_at_0755_is_usable_and_scribe_state_stays_private(self):
        # Orbit creates .orbit/tmp at its own mode; scribe must work without it being changed.
        orbit_tmp = self.workspace / ".orbit/tmp"
        orbit_tmp.mkdir(parents=True)
        orbit_tmp.chmod(0o755)
        start = self.hook("SessionStart")
        self.assertEqual(start.stderr, "")
        self.assertIn("hookSpecificOutput", json.loads(start.stdout))
        self.assertTrue(self.command("status")["on"])
        self.assertEqual(self.command("off"), {"on": False})
        self.assertEqual(self.command("on"), {"on": True})
        self.assertTrue(self.command("now")["queued"])
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        self.assertEqual(stat.S_IMODE(orbit_tmp.stat().st_mode), 0o755)
        for name in ("scribe", "session-transcripts"):
            self.assertEqual(stat.S_IMODE((orbit_tmp / name).stat().st_mode), 0o700, name)

    def test_group_or_other_writable_tmp_is_refused(self):
        orbit_tmp = self.workspace / ".orbit/tmp"
        orbit_tmp.mkdir(parents=True)
        for mode in (0o775, 0o757):
            with self.subTest(mode=oct(mode)):
                orbit_tmp.chmod(mode)
                start = self.hook("SessionStart")
                self.assertEqual(start.returncode, 0)
                self.assertIn("unsafe_state_directory", start.stderr)
                result = self.status_result()
                self.assertTrue(result["isError"])
                self.assertEqual(result["content"][0]["text"], "unsafe_state_directory")
                self.assertFalse((orbit_tmp / "scribe").exists())
                self.assertEqual(self.calls(), [])

    def test_scribe_state_directories_stay_owner_only(self):
        self.hook("SessionStart")
        staging = self.workspace / ".orbit/tmp/session-transcripts"
        self.assertEqual(stat.S_IMODE(staging.stat().st_mode), 0o700)
        staging.chmod(0o755)
        result = self.hook("Stop")
        self.assertEqual(result.returncode, 0)
        self.assertIn("unsafe_state_directory", result.stderr)
        self.assertEqual(list(staging.iterdir()), [])

    def test_symlinked_tmp_is_refused_by_hooks_and_status(self):
        outside = self.root / "outside"
        outside.mkdir(mode=0o700)
        (self.workspace / ".orbit").mkdir(mode=0o700)
        (self.workspace / ".orbit/tmp").symlink_to(outside, target_is_directory=True)
        start = self.hook("SessionStart")
        self.assertEqual(start.returncode, 0)
        self.assertIn("unsafe_state_directory", start.stderr)
        result = self.status_result()
        self.assertTrue(result["isError"])
        self.assertEqual(result["content"][0]["text"], "unsafe_state_directory")
        self.assertEqual(list(outside.iterdir()), [])
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
