"""Scribe v2 through the shipped MCP entrypoint and hooks: opt-in on/off,
Stop-hook cadence, marker routing and the orchestrator's turn-on.

A recording `orbit` stub plays the v2 Office tools over files in the test
HOME (markers are detected in the stub with the same line rules); a
recording pass stub stands in for the headless scribe run. No real host
state.
"""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time
import unittest

PLUGIN = Path(__file__).resolve().parents[1]
SCRATCH = PLUGIN.parents[1] / ".orbit/tmp"

STUB = r'''#!/usr/bin/env python3
import json, pathlib, sys
home = pathlib.Path.home()
args = sys.argv
tool = args[3]
payload = json.loads(args[args.index('--input') + 1])
with (home / 'calls.jsonl').open('a') as log:
    log.write(json.dumps({'tool': tool, 'input': payload}) + '\n')

TAGS = [
    {'name': 'preferences', 'scope': 'global', 'sink': 'preference_page', 'mode': 'propose',
     'capture': 'markers', 'instruction': 'A standing answer.'},
    {'name': 'decisions', 'scope': 'session', 'sink': 'decisions_ledger', 'mode': 'append',
     'capture': 'markers', 'instruction': 'A choice that was made.'},
    {'name': 'lesson', 'scope': 'global', 'sink': 'orbit_task', 'mode': 'propose',
     'capture': 'markers', 'instruction': 'A lesson for other sessions.', 'workspace': 'ws-polaris'},
]

def status_block():
    path = home / 'status.json'
    if path.exists():
        return json.loads(path.read_text())
    return {'cadence': {'minutes': 20, 'turns': 10}, 'tags': TAGS, 'tags_customized': False}

def messages(transcript):
    out = []
    for raw in pathlib.Path(transcript).read_text().splitlines():
        record = json.loads(raw)
        if record.get('type') not in ('user', 'assistant') or record.get('isSidechain'):
            continue
        content = record['message']['content']
        text = content if isinstance(content, str) else ' '.join(
            b['text'] for b in content if b.get('type') == 'text')
        if text.strip():
            out.append({'id': 'm-%04d' % (len(out) + 1), 'role': record['message']['role'],
                        'at': record['timestamp'], 'text': text})
    return out

def markers(all_messages, start, session_scope):
    user, agent = [], []
    names = {t['name'] for t in TAGS} | {'preferences', 'preference', 'decisions', 'decision'}
    for message in all_messages[start:]:
        for number, raw in enumerate(message['text'].splitlines(), start=1):
            line = raw.strip()
            if not line.startswith('#'):
                continue
            words = line[1:].split(None, 1)
            word = words[0].lower()
            rest = words[1] if len(words) > 1 else ''
            tag, text = None, rest
            if word in ('preferences', 'preference'):
                tag = 'preferences'
            elif word in ('decisions', 'decision'):
                tag = 'decisions'
            elif word == 'scribe':
                head, _, tail = rest.partition(' ')
                if head in names:
                    tag = {'preference': 'preferences', 'decision': 'decisions'}.get(head, head)
                    text = tail
            else:
                continue
            scope, marked = session_scope, False
            head, _, tail = text.partition(' ')
            if head.startswith('@'):
                scope = 'global' if head == '@global' else 'project:' + head[1:]
                text, marked = tail, True
            elif tag == 'preferences':
                scope = 'global'
            marker = {'msg': message['id'], 'line': number, 'marker': '#' + word, 'tag': tag,
                      'scope': scope, 'scope_marked': marked, 'text': text, 'because': None,
                      'quote': line, 'by': 'user' if message['role'] == 'user' else 'agent',
                      'at': message['at'], 'in_force': True, 'large_turn': False}
            (user if marker['by'] == 'user' else agent).append(marker)
    return user, agent

def scr(sid):
    path = home / ('scr2-' + sid + '.json')
    return json.loads(path.read_text()) if path.exists() else None

def save_scr(sid, record):
    (home / ('scr2-' + sid + '.json')).write_text(json.dumps(record))

if tool == 'office.session-brief':
    print(json.dumps({'sections': [], 'pending': []}))
elif tool == 'office.status':
    print(json.dumps({'version': 'test', 'scribe': status_block()}))
elif tool == 'office.scribe-control':
    path = home / ('control-' + payload['session_id'] + '.json')
    record = json.loads(path.read_text()) if path.exists() else {
        'on': None, 'on_by': None, 'scope': None, 'now_requested_at': None,
        'title': None, 'heartbeat_at': None}
    op, consumed = payload['op'], None
    if op == 'set_switch':
        record['on'] = payload['on']
        if payload['on']:
            if payload.get('on_by') or record['on_by'] is None:
                record['on_by'] = payload.get('on_by') or 'user'
            if payload.get('scope'):
                record['scope'] = payload['scope']
        else:
            record['on_by'] = None
    elif op == 'request_now':
        import time as _t
        record['now_requested_at'] = record['now_requested_at'] or str(_t.time())
    elif op == 'consume_now':
        expected = payload.get('requested_at')
        consumed = record['now_requested_at'] is not None and expected in (None, record['now_requested_at'])
        if consumed:
            record['now_requested_at'] = None
    elif op == 'set_title':
        record['title'] = payload['title'].strip()
    if op != 'get':
        path.write_text(json.dumps(record))
    out = {'op': op, 'session_id': payload['session_id'], 'on': record['on'],
           'on_by': record['on_by'], 'scope': record['scope'],
           'now_requested': record['now_requested_at'] is not None,
           'now_requested_at': record['now_requested_at'], 'title': record['title'],
           'heartbeat_at': record['heartbeat_at'], 'window_open': False, 'updated_at': None}
    if consumed is not None:
        out['consumed'] = consumed
    print(json.dumps(out))
elif tool == 'office.scribe-pending':
    sid = payload['session_id']
    all_messages = messages(pathlib.Path(payload['transcript']))
    (home / ('parsed-' + sid + '.json')).write_text(json.dumps(all_messages))
    record = scr(sid)
    through = record['scribed_through'] if record else None
    start = int(through.split('-')[1]) if through else 0
    scope = record['scope'] if record else payload.get('scope') or 'project:constellation'
    user, agent = markers(all_messages, start, scope)
    returned = [dict(m, context=True) for m in all_messages[max(0, start - 10):start]] + all_messages[start:]
    scr_id = record['id'] if record else 'SCR-20261030-0000-' + sid[:6]
    print(json.dumps({
        'format': 'v2', 'session_id': sid, 'provider': payload['provider'],
        'parsed': 'library/.office/state/scribe/' + sid + '.jsonl', 'content_withheld': False,
        'message_count': len(all_messages), 'new_count': len(all_messages) - start,
        'scr': {'exists': record is not None, 'id': scr_id,
                'path': 'sources/conversation/2026/10/' + scr_id + '.md', 'title': None,
                'title_by': None, 'scope': scope, 'previous': None, 'scribed_through': through,
                'sections': None, 'tag_entries': len(record['entries']) if record else 0,
                'next_tag': 'T' + str((len(record['entries']) if record else 0) + 1)},
        'scope': scope, 'previous': None, 'scribed_through': through, 'messages': returned,
        'markers': user, 'agent_markers': agent, 'custom_title': None,
        'rules': {'source': 'defaults', 'tags': [], 'models': {}},
        'tags': status_block()['tags'], 'cadence': status_block()['cadence']}))
elif tool == 'office.scribe-append':
    sid = payload['session_id']
    fail = home / 'fail-append-once'
    if fail.exists():
        fail.unlink()
        print(json.dumps({'ok': False, 'error': {'code': 'scribe_changed'}}))
        sys.exit(0)
    record = scr(sid) or {'id': 'SCR-20261030-0000-' + sid[:6], 'scope': 'project:constellation',
                          'scribed_through': None, 'entries': []}
    added, needs, duplicates = [], [], []
    for finding in payload.get('findings') or []:
        if any(e['msg'] == finding['msg'] and e['quote'] == finding.get('quote')
               for e in record['entries']):
            duplicates.append({'msg': finding['msg'], 'line': finding['line'], 'entry': 'T1'})
            continue
        number = len(record['entries']) + 1
        record['entries'].append({'msg': finding['msg'], 'quote': finding.get('quote')})
        added.append({'id': 'T%d' % number, 'group': finding.get('tag') or 'Scribe',
                      'msg': finding['msg'], 'line': finding['line']})
        if finding.get('tag') == 'preferences' and not finding.get('proposal'):
            needs.append({'entry': 'T%d' % number, 'cite': record['id'] + '#T%d' % number,
                          'msg': finding['msg'], 'by': finding.get('by', 'user'), 'scope': 'global',
                          'page': 'dom-preferences-global', 'matter': finding.get('matter'),
                          'statement': finding.get('statement'),
                          'conflicts': finding.get('conflicts') or [],
                          'reason': 'propose_mode', 'proposal': None})
    record['scribed_through'] = payload.get('through')
    save_scr(sid, record)
    (home / ('append-' + sid + '.json')).write_text(json.dumps(payload))
    print(json.dumps({'format': 'v2', 'session_id': sid, 'id': record['id'],
                      'path': 'sources/conversation/2026/10/' + record['id'] + '.md',
                      'created': True, 'written': True, 'title': None, 'title_by': 'scribe',
                      'scope': record['scope'], 'previous': None,
                      'scribed_through': record['scribed_through'], 'added': added,
                      'ledger': [], 'applied': [], 'needs_proposal': needs, 'withheld': [],
                      'reported': [], 'duplicates': duplicates, 'refused': [], 'files': []}))
elif tool == 'office.knowledge-show':
    page = home / ('page-' + payload['id'] + '.json')
    if not page.exists():
        print(json.dumps({'ok': False, 'error': {'code': 'page_not_found'}}))
        sys.exit(0)
    print(json.dumps({'page': json.loads(page.read_text()), 'backlinks': []}))
elif tool == 'office.proposal-create':
    count = len([r for r in (home / 'calls.jsonl').read_text().splitlines()
                 if json.loads(r)['tool'] == 'office.proposal-create'])
    print(json.dumps({'created': True, 'id': 'PROP-' + str(count), 'suppressed': False,
                      'evidence_page': 'proposals/PROP-' + str(count) + '.md',
                      'payload_sha256': '0' * 64}))
elif tool == 'orbit.task.add':
    count = len([r for r in (home / 'calls.jsonl').read_text().splitlines()
                 if json.loads(r)['tool'] == 'orbit.task.add'])
    print(json.dumps({'id': 'ORB-900' + str(count), 'status': 'proposed',
                      'title': payload.get('title'), 'tags': payload.get('tags', [])}))
else:
    print(json.dumps({'ok': False, 'error': {'code': 'unexpected_tool'}}))
'''

PASS_STUB = r'''#!/usr/bin/env python3
import os, pathlib, sys, time
home = pathlib.Path.home()
with (home / 'pass-starts').open('a') as log:
    log.write(sys.argv[1] + '\n')
time.sleep(float(os.environ.get('PASS_STUB_SLEEP', '0.2')))
'''


class ScribeV2(unittest.TestCase):
    def setUp(self):
        SCRATCH.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=SCRATCH, prefix="scribe-v2-tests-")
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.home = self.root / "home"
        self.home.mkdir(mode=0o700)
        self.workspace = self.root / "office"
        self.workspace.mkdir(mode=0o700)
        self.project = self.root / "constellation"
        self.project.mkdir()
        self.providers = self.home / ".claude/projects/-constellation"
        self.providers.mkdir(parents=True)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.bin / "orbit").write_text(STUB)
        (self.bin / "orbit").chmod(0o700)
        self.pass_stub = self.bin / "pass_stub.py"
        self.pass_stub.write_text(PASS_STUB)
        self.pass_stub.chmod(0o700)
        self.env = {"HOME": str(self.home),
                    "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                    "CLAUDE_PLUGIN_ROOT": str(PLUGIN),
                    "CLAUDE_PLUGIN_OPTION_WORKSPACE": str(self.workspace),
                    "CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS": "[]",
                    "SCRIBE_PASS_COMMAND": json.dumps(
                        [sys.executable, str(self.pass_stub), "{session_id}"])}
        self.sid = "session-one"
        self.path = self.providers / (self.sid + ".jsonl")
        self.context = {"session_id": self.sid, "cwd": str(self.project),
                        "transcript_path": str(self.path)}

    # ---- fixtures -------------------------------------------------------

    def transcript(self, turns, entrypoint=None, path=None, sid=None):
        path = path or self.path
        sid = sid or self.sid
        with path.open("w") as stream:
            for index, (role, text) in enumerate(turns):
                record = {"type": role, "sessionId": sid, "cwd": str(self.project),
                          "timestamp": "2026-10-30T0{}:00:00Z".format(index % 10),
                          "message": {"role": role, "content": text}}
                if entrypoint is not None:
                    record["entrypoint"] = entrypoint
                stream.write(json.dumps(record) + "\n")
        return path

    def hook(self, event, context=None):
        hooks = json.loads((PLUGIN / "hooks/hooks.json").read_text())["hooks"]
        command = hooks[event][0]["hooks"][0]["command"].replace("${CLAUDE_PLUGIN_ROOT}", str(PLUGIN))
        started = time.monotonic()
        result = subprocess.run(shlex.split(command), input=json.dumps(context or self.context),
                                text=True, env=self.env, cwd=self.root,
                                capture_output=True, timeout=10)
        result.elapsed = time.monotonic() - started
        return result

    def tool(self, arguments, error=False):
        data = json.dumps({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "scribe", "arguments": arguments}}) + "\n"
        process = subprocess.run([sys.executable, "-I", str(PLUGIN / "scripts/bridge.py")],
                                 input=data, text=True, env=self.env, cwd=self.root,
                                 capture_output=True, timeout=60)
        self.assertEqual(process.returncode, 0, process.stderr)
        result = json.loads(process.stdout.splitlines()[0])["result"]
        self.assertEqual(result["isError"], error, result)
        text = result["content"][0]["text"]
        return text if error else json.loads(text)

    def scribe(self, args, sid=None):
        return self.tool({"session_id": sid or self.sid, "args": args})

    def calls(self, tool=None):
        path = self.home / "calls.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return [r for r in records if not tool or r["tool"] == tool]

    def inputs(self, tool):
        return [c["input"] for c in self.calls(tool)]

    def pass_starts(self):
        path = self.home / "pass-starts"
        deadline = time.monotonic() + 2.0
        while time.monotonic() < deadline:
            if path.exists():
                return path.read_text().splitlines()
            time.sleep(0.05)
        return []

    def state_file(self, suffix=""):
        return self.workspace / ".orbit/tmp/scribe" / (self.sid + suffix)

    def seed_pass_state(self, **changes):
        path = self.state_file(".json")
        state = json.loads(path.read_text()) if path.exists() else {}
        state.update(changes)
        path.write_text(json.dumps(state))

    def turn_on(self, turns=None):
        """A session the user turned on; the first-pass reply comes back."""
        if turns is not None:
            self.transcript(turns)
        self.hook("SessionStart")
        return self.scribe("on")

    # ---- opt-in ---------------------------------------------------------

    def test_a_new_session_is_not_scribed_until_on_and_off_stops_passes(self):
        self.transcript([("user", "We decided to keep the scribe local")])
        update = self.scribe("update")
        self.assertFalse(update["on"])
        self.assertIn("off", update["display"])
        self.assertEqual(self.calls("office.scribe-pending"), [])
        self.hook("SessionStart")
        stop = self.hook("Stop")
        self.assertEqual(stop.returncode, 0, stop.stderr)
        self.assertEqual(self.pass_starts(), [], "no pass before /scribe on")
        self.assertEqual(self.calls("office.scribe-append"), [])

        reply = self.scribe("on @global")
        switches = [c for c in self.inputs("office.scribe-control")
                    if c["op"] == "set_switch" and c.get("on")]
        self.assertEqual(switches[-1]["on_by"], "user")
        self.assertEqual(switches[-1]["scope"], "global")
        self.assertTrue(reply["on"])
        self.assertIn("agent", reply, "the first pass runs at once")
        pending = self.tool({"session_id": self.sid, "action": "pending2"})
        self.assertEqual(pending["scope"], "global")
        done = self.tool({"session_id": self.sid, "action": "append2",
                          "sections": {"summary": "Decided the scribe stays local",
                                       "goals": [], "outcome": [], "next": []},
                          "findings": []})
        self.assertIn("Scribed through", done["display"])

        off = self.scribe("off")
        self.assertFalse(off["on"])
        self.assertIn("Nothing already written is removed", off["display"])
        written = (self.home / ("scr2-" + self.sid + ".json")).read_text()
        update = self.scribe("update")
        self.assertFalse(update["on"])
        self.hook("Stop")
        self.assertEqual(self.pass_starts(), [], "off stops Stop passes too")
        self.assertEqual((self.home / ("scr2-" + self.sid + ".json")).read_text(), written)

    def test_status_and_tags_are_model_free(self):
        self.turn_on([("user", "hello")])
        status = self.scribe("status")
        self.assertTrue(status["on"])
        self.assertEqual(status["on_by"], "user")
        self.assertIn("SCR:", status["display"])
        self.assertFalse(status["model_call"])
        tags = self.scribe("tags")
        self.assertIn("preferences", tags["display"])
        self.assertIn("orbit_task", tags["display"])
        self.assertFalse(tags["model_call"])

    def test_orchestrator_on_records_agent_and_announces_but_headless_is_refused(self):
        self.transcript([("user", "hello")])
        self.hook("SessionStart")
        reply = self.tool({"session_id": self.sid, "action": "on"})
        self.assertEqual(reply["on_by"], "agent")
        self.assertIn("Tell the user in one line", reply["display"])
        switches = [c for c in self.inputs("office.scribe-control")
                    if c["op"] == "set_switch" and c.get("on")]
        self.assertEqual(switches[-1]["on_by"], "agent")

        sid = "session-headless"
        path = self.providers / (sid + ".jsonl")
        self.transcript([("user", "hello")], entrypoint="sdk-cli", path=path, sid=sid)
        before = len([c for c in self.inputs("office.scribe-control")
                      if c["op"] == "set_switch"])
        error = self.tool({"session_id": sid, "action": "on"}, error=True)
        self.assertEqual(error, "scribe_headless_session")
        after = len([c for c in self.inputs("office.scribe-control")
                     if c["op"] == "set_switch"])
        self.assertEqual(after, before, "nothing was turned on")

    # ---- cadence --------------------------------------------------------

    def set_cadence(self, minutes, turns):
        block = {"cadence": {"minutes": minutes, "turns": turns},
                 "tags": [], "tags_customized": True}
        (self.home / "status.json").write_text(json.dumps(block))

    def test_stop_hook_starts_one_pass_at_the_turns_threshold_and_none_before(self):
        self.set_cadence(minutes=240, turns=3)
        self.turn_on([("user", "one"), ("assistant", "ok")])
        self.seed_pass_state(v2_last_pass_at=time.time(), v2_last_pass_turns=1)
        self.hook("Stop")
        self.assertEqual(self.pass_starts(), [], "one new turn is under the threshold")
        self.transcript([("user", "one"), ("assistant", "ok"),
                         ("user", "two"), ("user", "three"), ("user", "four")])
        self.env["PASS_STUB_SLEEP"] = "5"
        stop = self.hook("Stop")
        self.assertLess(stop.elapsed, 4.0, "the hook returns without waiting for the pass")
        self.assertEqual(self.pass_starts(), [self.sid])
        self.hook("Stop")
        time.sleep(0.3)
        self.assertEqual(self.pass_starts(), [self.sid], "the cadence holds the second Stop")

    def test_stop_hook_minutes_threshold_and_session_end_final_pass(self):
        self.set_cadence(minutes=20, turns=100)
        self.turn_on([("user", "one")])
        self.seed_pass_state(v2_last_pass_at=time.time() - 21 * 60, v2_last_pass_turns=1)
        self.hook("Stop")
        self.assertEqual(self.pass_starts(), [self.sid], "21 minutes reaches the threshold")
        time.sleep(1.0)  # let the pass stub finish and release the lock
        self.hook("SessionEnd")
        self.assertEqual(self.pass_starts(), [self.sid, self.sid],
                         "SessionEnd runs a final pass inside the cadence")

    def test_a_running_pass_blocks_a_second_start(self):
        self.set_cadence(minutes=5, turns=2)
        self.turn_on([("user", "one"), ("user", "two")])
        self.env["PASS_STUB_SLEEP"] = "3"
        self.seed_pass_state(v2_last_pass_at=time.time() - 600, v2_last_pass_turns=0)
        self.hook("Stop")
        self.assertEqual(self.pass_starts(), [self.sid])
        # Due again on paper, but the first pass still holds the lock.
        self.seed_pass_state(v2_last_pass_at=time.time() - 600, v2_last_pass_turns=0)
        self.hook("Stop")
        time.sleep(0.3)
        self.assertEqual(self.pass_starts(), [self.sid], "at most one pass runs")
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            lock = self.state_file(".pass.lock")
            try:
                fd = os.open(lock, os.O_RDWR)
                import fcntl
                try:
                    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                    break
                except BlockingIOError:
                    time.sleep(0.1)
                finally:
                    os.close(fd)
            except FileNotFoundError:
                break

    # ---- routing --------------------------------------------------------

    SECTIONS = {"summary": "Planning the release", "goals": [], "outcome": [], "next": []}

    def append2(self, findings, todos=None, error=False):
        arguments = {"session_id": self.sid, "action": "append2",
                     "sections": dict(self.SECTIONS), "findings": findings}
        if todos is not None:
            arguments["todos"] = todos
        return self.tool(arguments, error=error)

    def test_a_user_scheduling_marker_files_one_proposed_task_and_a_retry_none(self):
        import datetime as dt
        self.turn_on([
            ("user", "#scribe add this as a scheduled task at 11PM tonight"),
            ("assistant", "#scribe schedule my own thing tomorrow"),
        ])
        pending = self.tool({"session_id": self.sid, "action": "pending2"})
        self.assertEqual([m["by"] for m in pending["markers"]], ["user"])
        self.assertEqual([m["by"] for m in pending["agent_markers"]], ["agent"])
        when = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(hours=5)).isoformat()
        findings = [
            {"msg": "m-0001", "line": 1, "tag": None, "by": "user",
             "quote": "#scribe add this as a scheduled task at 11PM tonight",
             "schedule": {"not_before": when, "title": "Do the scheduled thing",
                          "zone": "UTC"}},
            {"msg": "m-0002", "line": 1, "tag": None, "by": "agent",
             "quote": "#scribe schedule my own thing tomorrow",
             "schedule": {"not_before": when, "title": "Agent must not schedule"}},
        ]
        (self.home / "fail-append-once").write_text("x")
        self.append2(findings, error=True)
        adds = self.inputs("orbit.task.add")
        self.assertEqual(len(adds), 1, "the failed append still filed exactly one task")
        done = self.append2(findings)
        adds = self.inputs("orbit.task.add")
        self.assertEqual(len(adds), 1, "a retry files none")
        task = adds[0]
        self.assertEqual(task["not_before"], when)
        self.assertIn("#scribe add this as a scheduled task at 11PM tonight",
                      task["description"])
        self.assertIn(when, task["description"])
        self.assertIn("UTC", task["description"])
        self.assertNotIn("status", task, "filed proposed, never dispatched")
        self.assertNotIn("owner:agent", task.get("tags", []))
        appended = json.loads((self.home / ("append-" + self.sid + ".json")).read_text())
        by_msg = {f["msg"]: f for f in appended["findings"]}
        self.assertEqual(by_msg["m-0001"]["task"], "ORB-9001")
        self.assertNotIn("task", by_msg["m-0002"], "an agent marker never schedules")
        self.assertNotIn("schedule", by_msg["m-0001"], "routing fields never reach Office")
        self.assertIn("ORB-9001", done["display"])

    def test_an_unresolvable_schedule_time_files_nothing(self):
        import datetime as dt
        self.turn_on([("user", "#scribe do it at some point")])
        self.tool({"session_id": self.sid, "action": "pending2"})
        past = (dt.datetime.now(dt.timezone.utc) - dt.timedelta(hours=1)).isoformat()
        done = self.append2([
            {"msg": "m-0001", "line": 1, "tag": None, "by": "user",
             "quote": "#scribe do it at some point",
             "schedule": {"not_before": past, "title": "Too late"}}])
        self.assertEqual(self.inputs("orbit.task.add"), [])
        self.assertIn("schedule_time_unresolved", done["display"])

    def test_every_filed_task_is_proposed_or_an_owner_you_todo(self):
        self.turn_on([
            ("user", "I'll call Jane tomorrow\n#scribe lesson check the lock before retrying"),
        ])
        pending = self.tool({"session_id": self.sid, "action": "pending2"})
        marker = pending["markers"][0]
        self.assertEqual((marker["tag"], marker["line"]), ("lesson", 2))
        done = self.append2(
            [{"msg": "m-0001", "line": 2, "tag": "lesson", "by": "user",
              "quote": "#scribe lesson check the lock before retrying",
              "task_title": "Lesson: check the lock before retrying"}],
            todos=[{"msg": "m-0001", "line": 1, "title": "Call Jane"}])
        adds = self.inputs("orbit.task.add")
        self.assertEqual(len(adds), 2)
        for payload in adds:
            self.assertNotIn("owner:agent", payload.get("tags", []))
            self.assertNotEqual(payload.get("status"), "backlog")
        todo = next(p for p in adds if "owner:you" in p.get("tags", []))
        self.assertEqual(todo["title"], "Call Jane")
        lesson = next(p for p in adds if "owner:you" not in p.get("tags", []))
        self.assertEqual(lesson["workspace"], "ws-polaris",
                         "an orbit_task tag files in its tag's workspace")
        self.assertIn("#scribe lesson check the lock before retrying",
                      lesson["description"])
        appended = json.loads((self.home / ("append-" + self.sid + ".json")).read_text())
        self.assertEqual(appended["findings"][0]["task"], "ORB-9001")
        self.assertTrue(any("ORB-9002" in item and "owner: you" in item
                            for item in appended["sections"]["next"]),
                        "Next cites the to-do")
        self.assertIn("ORB-9002", done["display"])

    def test_a_preference_finding_files_one_proposal_after_the_append(self):
        self.turn_on([("user", "#preferences always draft emails, never send")])
        self.tool({"session_id": self.sid, "action": "pending2"})
        done = self.append2([
            {"msg": "m-0001", "line": 1, "tag": "preferences", "by": "user",
             "quote": "#preferences always draft emails, never send",
             "matter": "email", "statement": "Always draft emails, never send"}])
        creates = self.inputs("office.proposal-create")
        self.assertEqual(len(creates), 1)
        payload = creates[0]["payload"]
        self.assertEqual(payload["kind"], "knowledge_add")
        self.assertIn("dom-preferences-global", payload["markdown"])
        self.assertIn("## email", payload["markdown"])
        self.assertIn("SCR-20261030-0000-sessio#T1", payload["markdown"])
        self.assertEqual(creates[0]["evidence"], ["SCR-20261030-0000-sessio#T1"])
        self.assertIn("PROP-1", done["display"])
