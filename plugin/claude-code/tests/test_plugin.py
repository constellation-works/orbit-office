"""Legacy automatic capture through the shipped hooks and the pane control; no real host state.

These passes are off by default in Scribe v1 (test_scribe_v1.py); this suite
turns them on with the plugin's `auto_capture` option.
"""
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
if mode == 'no-control' and tool == 'office.scribe-control':
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
elif tool == 'office.scribe-control':
    # Office's per-session record, as office.scribe-control keeps it.
    path = home / ('control-' + payload['session_id'] + '.json')
    record = json.loads(path.read_text()) if path.exists() else {
        'on': None, 'now_requested_at': None, 'title': None, 'heartbeat_at': None}
    op, now, consumed = payload['op'], time.time(), None
    if op == 'set_switch':
        record['on'] = payload['on']
    elif op == 'request_now':
        record['now_requested_at'] = record['now_requested_at'] or now
    elif op == 'consume_now':
        expected = payload.get('requested_at')
        consumed = record['now_requested_at'] is not None and expected in (None, record['now_requested_at'])
        if consumed:
            record['now_requested_at'] = None
    elif op == 'set_title':
        record['title'] = payload['title'].strip()
    elif op == 'heartbeat':
        record['heartbeat_at'] = now
    if op != 'get':
        path.write_text(json.dumps(record))
    out = {'op': op, 'session_id': payload['session_id'], 'on': record['on'],
           'now_requested': record['now_requested_at'] is not None, 'now_requested_at': record['now_requested_at'],
           'title': record['title'], 'heartbeat_at': record['heartbeat_at'],
           'window_open': record['heartbeat_at'] is not None and now - record['heartbeat_at'] < 120}
    if consumed is not None:
        out['consumed'] = consumed
    print(json.dumps(out))
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
                    "CLAUDE_PLUGIN_OPTION_TIMEOUT": "2", "CLAUDE_PLUGIN_OPTION_AUTO_CAPTURE": "true"}
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

    def pane(self, arguments):
        """The structured control the desktop pane runs (scripts/pane.py)."""
        return subprocess.run([sys.executable, "-I", str(PLUGIN / "scripts/pane.py"), json.dumps(arguments)],
                              text=True, env=self.env, cwd=self.root, capture_output=True, timeout=35)

    def command(self, action, **args):
        process = self.pane({"session_id": self.sid, "action": action, **args})
        self.assertEqual(process.returncode, 0, process.stderr)
        return json.loads(process.stdout)

    def calls(self, tool=None):
        path = self.home / "calls.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return [r for r in records if not tool or r["tool"] == tool]

    def finish_pass(self):
        (self.home / "task-status").write_text("done")

    def control_record(self, **changes):
        """Office's record for this session, as another host (the window) writes it."""
        path = self.home / ("control-" + self.sid + ".json")
        record = json.loads(path.read_text()) if path.exists() else {
            "on": None, "now_requested_at": None, "title": None, "heartbeat_at": None}
        if changes:
            record.update(changes)
            path.write_text(json.dumps(record))
        return record

    def control_calls(self, op=None):
        return [c["input"] for c in self.calls("office.scribe-control") if not op or c["input"]["op"] == op]

    def flag(self):
        return self.workspace / (".orbit/tmp/scribe/" + self.sid)

    def test_session_start_brief_forwards_project_and_orchestrator(self):
        self.hook("SessionStart")
        briefs = self.calls("office.session-brief")
        self.assertEqual(len(briefs), 1)
        self.assertEqual(briefs[0]["input"], {"project": "project", "orchestrator": "claude"})

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
        # Only the session's control record (its id, no transcript data) is read.
        self.assertEqual({c["tool"] for c in self.calls()}, {"office.scribe-control"})
        staging = self.workspace / ".orbit/tmp/session-transcripts"
        self.assertEqual(list(staging.iterdir()) if staging.exists() else [], [], "nothing staged")

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
        self.assertEqual(self.control_calls("heartbeat"), [{"op": "heartbeat", "session_id": self.sid}])
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 2)

    def test_shared_heartbeat_from_another_host_sets_max_turns_two_only_while_fresh(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.finish_pass()
        # The web window heartbeats through Office; no local file exists here.
        self.control_record(heartbeat_at=time.time() - 130)
        self.write_transcript(3)
        self.hook("Stop")
        self.assertEqual(len(self.calls("orbit.task.add")), 1, "a heartbeat over two minutes old is closed")
        self.control_record(heartbeat_at=time.time() - 30)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 2, "max_turns 2 while the heartbeat is fresh")
        self.assertFalse((self.workspace / (".orbit/tmp/scribe/" + self.sid + ".window-open")).exists())
        self.assertTrue(self.command("status")["window_open"])

    def test_local_heartbeat_cache_still_applies_when_office_cannot_answer(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.finish_pass()
        self.write_transcript(3)
        (self.home / "stub-mode").write_text("no-control")
        pane = subprocess.run(
            [sys.executable, "-I", str(PLUGIN / "scripts/pane.py"),
             json.dumps({"session_id": self.sid, "action": "heartbeat"})],
            text=True, env=self.env, cwd=self.root, capture_output=True, timeout=5)
        self.assertEqual(pane.returncode, 1, "the Office write failed and is reported")
        result = self.hook("Stop")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, "", "an unreadable control record fails open silently")
        self.assertEqual(len(self.calls("orbit.task.add")), 2)

    def test_shared_switch_is_mirrored_to_the_flag_file_and_commands_write_through(self):
        self.hook("SessionStart")
        self.hook("Stop")
        # The first turn publishes this host's default switch to Office.
        self.assertEqual(self.control_calls("set_switch"), [{"op": "set_switch", "session_id": self.sid, "on": True}])
        self.assertTrue(self.flag().exists())
        self.finish_pass()
        # The window turns the scribe off: the next turn mirrors it and captures nothing.
        self.control_record(on=False)
        self.write_transcript(20)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertFalse(self.flag().exists())
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        self.assertFalse(self.command("status")["on"])
        # The window turns it back on: mirrored and captured.
        self.control_record(on=True)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertTrue(self.flag().exists())
        self.assertEqual(len(self.calls("orbit.task.add")), 2)
        # /scribe off and on write Office first, then the flag file.
        self.assertEqual(self.command("off"), {"on": False})
        self.assertFalse(self.control_record()["on"])
        self.assertFalse(self.flag().exists())
        self.assertEqual(self.command("on"), {"on": True})
        self.assertTrue(self.control_record()["on"])
        self.assertTrue(self.flag().exists())

    def test_pending_capture_now_runs_at_the_next_turn_and_is_consumed_once(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.finish_pass()
        self.write_transcript(2)  # debounced on its own (2 < 8 turns, inside 5 minutes)
        self.control_record(now_requested_at=1234.5)  # the window pressed Capture now
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 2)
        self.assertEqual(self.control_calls("consume_now"),
                         [{"op": "consume_now", "session_id": self.sid, "requested_at": 1234.5}])
        self.assertIsNone(self.control_record()["now_requested_at"])
        self.finish_pass()
        self.write_transcript(3)
        self.assertEqual(self.hook("Stop").returncode, 0)
        self.assertEqual(len(self.calls("orbit.task.add")), 2, "consumed once: the next turn debounces again")
        self.assertEqual(len(self.control_calls("consume_now")), 1)

    def test_capture_now_waits_for_a_pending_pass_and_now_writes_through(self):
        self.hook("SessionStart")
        self.hook("Stop")  # TEST-1 still in progress
        self.write_transcript(2)
        self.control_record(now_requested_at=99.0)
        self.hook("Stop")
        self.assertEqual(len(self.calls("orbit.task.add")), 1)
        self.assertEqual(self.control_record()["now_requested_at"], 99.0, "kept while a pass is pending")
        self.finish_pass()
        self.hook("Stop")
        self.assertEqual(len(self.calls("orbit.task.add")), 2)
        self.assertIsNone(self.control_record()["now_requested_at"])
        # /scribe now records the request in Office, runs it and consumes it.
        self.finish_pass()
        self.write_transcript(3)
        result = self.command("now")
        self.assertTrue(result["queued"])
        self.assertEqual(result["capture_now"], "consumed")
        self.assertEqual(len(self.control_calls("request_now")), 1)
        self.assertIsNone(self.control_record()["now_requested_at"])

    def test_title_set_in_the_window_is_applied_once_as_a_user_title(self):
        self.hook("SessionStart")
        self.hook("Stop")
        self.control_record(title="Release planning")
        self.hook("Stop")
        self.hook("Stop")
        titled = [c["input"] for c in self.calls("office.capture") if c["input"].get("title")]
        self.assertEqual(len(titled), 1)
        self.assertEqual(titled[0]["title"], "Release planning")
        self.assertEqual(titled[0]["title_by"], "user")
        # /scribe title writes Office too, and is not re-applied by the hook.
        self.command("title", title="Final name")
        self.assertEqual(self.control_record()["title"], "Final name")
        self.hook("Stop")
        titled = [c["input"] for c in self.calls("office.capture") if c["input"].get("title")]
        self.assertEqual([t["title"] for t in titled], ["Release planning", "Final name"])

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
        response = self.pane({"session_id": self.sid, "action": "status", "title": "ignored?"})
        self.assertEqual(response.returncode, 1)
        self.assertIn("field_not_valid_for_action", response.stderr)
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
        process = self.pane({"session_id": self.sid, "action": "status"})
        return {"isError": process.returncode != 0,
                "content": [{"text": process.stderr.strip().removeprefix("office-scribe: ")}]}

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


class ProcessGroupSweep(unittest.TestCase):
    def test_exited_group_eperm_keeps_the_result(self):
        # macOS answers EPERM from killpg when only the unreaped leader is left
        # (ORB-20814); the finished call's output must survive the sweep.
        sys.path.insert(0, str(PLUGIN / "scripts"))
        import process
        from unittest import mock

        def refuse(_pid, _sig):
            raise PermissionError(1, "Operation not permitted")

        with mock.patch.object(process.os, "killpg", refuse):
            result = process.run([sys.executable, "-c", "print('{\"ok\": true}')"], str(PLUGIN), 5)
        self.assertEqual(result, {"ok": True})


class ManifestCallability(unittest.TestCase):
    def test_mutating_tools_the_plugin_calls_are_agent_callable(self):
        # The plugin calls Orbit as `local-cli` holding [agent]; a mutating
        # Office tool without agent_callable is refused there (ORB-20856).
        sys.path.insert(0, str(PLUGIN / "scripts"))
        import scribe

        blocks: dict[str, dict[str, str]] = {}
        current = None
        manifest = PLUGIN.parent / ".orbit-plugin" / "plugin.yaml"
        for line in manifest.read_text().splitlines():
            if line.startswith("    - name: "):
                current = blocks.setdefault(line.split(": ", 1)[1].strip(), {})
            elif current is not None and line.startswith("      ") and ":" in line:
                key, _, value = line.strip().partition(":")
                current[key] = value.strip()
            elif not line.startswith("      "):
                current = None
        for tool in sorted(t for t in scribe.TOOLS if t.startswith("office.")):
            block = blocks.get(tool.split(".", 1)[1])
            self.assertIsNotNone(block, tool + " is declared in the manifest")
            if block.get("execution_kind") == "mutating":
                self.assertEqual(block.get("agent_callable"), "true", tool + " is agent_callable")
