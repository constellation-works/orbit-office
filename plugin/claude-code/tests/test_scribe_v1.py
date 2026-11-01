"""Scribe v1 through the shipped MCP entrypoint and hooks, with fixture Office responses.

A recording `orbit` stub plays office.scribe-pending, office.scribe-append and
office.scribe-query over files in the test HOME; a recording `claude` stub
proves the plugin never starts a model process. The scribe agent's steps are
driven as the agent would call them. No real host state.
"""
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import unittest

PLUGIN = Path(__file__).resolve().parents[1]
SCRATCH = PLUGIN.parents[1] / ".orbit/tmp"

STUB = r'''#!/usr/bin/env python3
import json, pathlib, sys
home = pathlib.Path.home()
args = sys.argv
tool = args[3]
workspace = args[args.index('--workspace') + 1]
payload = json.loads(args[args.index('--input') + 1])
with (home / 'calls.jsonl').open('a') as log:
    log.write(json.dumps({'tool': tool, 'input': payload}) + '\n')
TAGS = ['decision', 'direction', 'question', 'todo', 'fact', 'landed', 'preference', 'why',
        'agent-decision', 'pending', 'resolved', 'incident']

def scr(sid):
    path = home / ('scr-' + sid + '.json')
    return json.loads(path.read_text()) if path.exists() else None

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

def lines():
    found = []
    for path in sorted(home.glob('scr-*.json')):
        record = json.loads(path.read_text())
        for number, text in enumerate(record['lines'], start=21):
            found.append({'scr_id': record['id'], 'path': record['path'], 'line': number, 'text': text,
                          'msg': text.split(' ')[1], 'by': 'user', 'counterpart': 'orchestrator',
                          'resume': 'claude --resume ' + record['session_id']})
    return found

if tool == 'office.session-brief':
    print(json.dumps({'sections': [], 'pending': []}))
elif tool == 'office.scribe-pending':
    sid = payload['session_id']
    transcript = pathlib.Path(payload['transcript']).resolve()
    assert str(transcript).startswith(str(pathlib.Path(workspace).resolve()) + '/'), 'stage inside the workspace'
    all_messages = messages(transcript)
    (home / ('parsed-' + sid + '.json')).write_text(json.dumps(all_messages))
    record = scr(sid)
    through = record['scribed_through'] if record else None
    start = int(through.split('-')[1]) if through else 0
    returned = [dict(m, context=True) for m in all_messages[max(0, start - 10):start]] + all_messages[start:]
    project = payload['cwd'].rstrip('/').rsplit('/', 1)[-1]
    view = None
    if record:
        view = {'exists': True, 'id': record['id'], 'path': record['path'], 'title': record.get('title'),
                'scribed_through': through, 'last_scribed_at': None, 'topics': [], 'aliases': [],
                'counterpart': None, 'counterpart_name': None}
    print(json.dumps({'session_id': sid, 'provider': payload['provider'], 'parsed': 'library/.office/state/scribe/' + sid + '.jsonl',
                      'content_withheld': False, 'message_count': len(all_messages),
                      'new_count': len(all_messages) - start, 'scr': view, 'scribed_through': through,
                      'messages': returned,
                      'rules': {'source': 'defaults', 'tags': [{'name': t, 'meaning': t, 'examples': [], 'authors': 'any'} for t in TAGS],
                                'topics': [project], 'capture': [], 'matters': ['release-with-open-defects']}}))
elif tool == 'office.scribe-append':
    sid = payload['session_id']
    parsed = home / ('parsed-' + sid + '.json')
    if not parsed.exists():
        print(json.dumps({'ok': False, 'error': {'code': 'scribe_not_parsed'}}))
        sys.exit(0)
    by_id = {m['id']: m for m in json.loads(parsed.read_text())}
    assert payload['through'] in by_id, 'through names a parsed message'
    record = scr(sid) or {'session_id': sid, 'id': 'SCR-30-0000-' + sid, 'title': None,
                          'path': 'sources/conversation/2026/10/SCR-30-0000-' + sid + '.md', 'lines': []}
    added, refused = [], []
    for index, entry in enumerate(payload.get('entries', [])):
        if entry['tag'] not in TAGS:
            refused.append({'index': index, 'msg': entry.get('msg', 'note'), 'reason': 'unknown_tag', 'message': 'x'})
            continue
        if 'msg' in entry and entry['quote'] not in by_id.get(entry['msg'], {}).get('text', ''):
            refused.append({'index': index, 'msg': entry['msg'], 'reason': 'quote_not_verbatim', 'message': 'x'})
            continue
        head = entry.get('msg', 'note')
        text = '- %s 2026-10-30T10:00-07:00 #%s "%s"' % (head, entry['tag'], entry.get('quote', entry.get('text')))
        if entry.get('refs'):
            text += ' refs: ' + ', '.join(entry['refs'])
        if entry.get('by'):
            text += ' by: ' + entry['by']
        record['lines'].append(text)
        added.append({'line': 20 + len(record['lines']), 'text': text})
    previous = record.get('scribed_through')
    record['scribed_through'] = payload['through']
    if payload.get('title'):
        record['title'] = payload['title']
    (home / ('scr-' + sid + '.json')).write_text(json.dumps(record))
    print(json.dumps({'session_id': sid, 'id': record['id'], 'path': record['path'], 'created': previous is None,
                      'written': True, 'renamed_from': None, 'previous_through': previous,
                      'scribed_through': payload['through'], 'topics': [], 'added': added,
                      'duplicates': 0, 'refused': refused}))
elif tool == 'office.scribe-query':
    found = lines()
    mode = payload.get('mode')
    want = {'pending': '#pending', 'pref': '#preference', 'landed': '#landed', 'why': '#decision'}.get(mode)
    if want:
        found = [r for r in found if want in r['text']]
    if mode in ('pref', 'why') and payload.get('matter'):
        found = [r for r in found if 'matter: ' + payload['matter'] in r['text']]
    if mode == 'why' and payload.get('topic'):
        found = [r for r in found if payload['topic'] in r['text']]
    print(json.dumps({'results': found, 'zone': 'PDT', 'files_searched': len(list(home.glob('scr-*.json')))}))
else:
    print(json.dumps({'ok': False, 'error': {'code': 'unexpected_tool'}}))
'''

MODEL_STUB = r'''#!/usr/bin/env python3
import pathlib
with (pathlib.Path.home() / 'model-calls').open('a') as log:
    log.write('claude\n')
'''


class ScribeV1(unittest.TestCase):
    def setUp(self):
        SCRATCH.mkdir(parents=True, exist_ok=True)
        self.tmp = tempfile.TemporaryDirectory(dir=SCRATCH, prefix="scribe-v1-tests-")
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
        for name, text in (("orbit", STUB), ("claude", MODEL_STUB)):
            (self.bin / name).write_text(text)
            (self.bin / name).chmod(0o700)
        # Default plugin options: auto_capture is unset, as on a fresh install.
        self.env = {"HOME": str(self.home), "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                    "OFFICE_WORKSPACE": str(self.workspace), "OFFICE_LIBRARY": str(self.root / "unused-library"),
                    "CLAUDE_PLUGIN_ROOT": str(PLUGIN), "CLAUDE_PLUGIN_OPTION_WORKSPACE": str(self.workspace),
                    "CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS": "[]"}
        self.sid = "session-one"
        self.path = self.providers / (self.sid + ".jsonl")
        self.context = {"session_id": self.sid, "cwd": str(self.project), "transcript_path": str(self.path)}

    # ---- fixtures -------------------------------------------------------

    def transcript(self, turns, path=None, sid=None, date="2026-10-30"):
        """`turns` as (role, text); returns the provider file."""
        path = path or self.path
        sid = sid or self.sid
        with path.open("w") as stream:
            for index, (role, text) in enumerate(turns):
                stream.write(json.dumps({"type": role, "sessionId": sid, "cwd": str(self.project),
                                         "timestamp": "{}T0{}:00:00Z".format(date, index % 10),
                                         "message": {"role": role, "content": text}}) + "\n")
        return path

    def hook(self, event):
        hooks = json.loads((PLUGIN / "hooks/hooks.json").read_text())["hooks"]
        command = hooks[event][0]["hooks"][0]["command"].replace("${CLAUDE_PLUGIN_ROOT}", str(PLUGIN))
        return subprocess.run(shlex.split(command), input=json.dumps(self.context), text=True,
                              env=self.env, cwd=self.root, capture_output=True, timeout=10)

    def mcp(self, *messages):
        data = "".join(json.dumps(m) + "\n" for m in messages)
        process = subprocess.run([sys.executable, "-I", str(PLUGIN / "scripts/bridge.py")], input=data,
                                 text=True, env=self.env, cwd=self.root, capture_output=True, timeout=60)
        self.assertEqual(process.returncode, 0, process.stderr)
        return [json.loads(line) for line in process.stdout.splitlines()]

    def tool(self, arguments, error=False):
        result = self.mcp({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
                           "params": {"name": "scribe", "arguments": arguments}})[0]["result"]
        self.assertEqual(result["isError"], error, result)
        text = result["content"][0]["text"]
        return text if error else json.loads(text)

    def scribe(self, args, sid=None):
        """`/scribe <args>` as the command calls the tool."""
        return self.tool({"session_id": sid or self.sid, "args": args})

    def agent_catchup(self, reply, entries):
        """The scribe agent's writing level for the session its request names."""
        sid = reply["session_id"]
        pending = self.tool({"session_id": sid, "action": "pending"})
        appended = self.tool({"session_id": sid, "action": "append", "entries": entries})
        return pending, appended

    def calls(self, tool=None):
        path = self.home / "calls.jsonl"
        records = [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []
        return [r for r in records if not tool or r["tool"] == tool]

    def office_calls(self):
        return [c["tool"] for c in self.calls()]

    def assert_no_model_process(self):
        self.assertFalse((self.home / "model-calls").exists(), "the plugin never starts a model")

    def seed_scribed(self):
        """A scribed session: one catch-up with a decision, a landed line and a preference."""
        self.transcript([("user", "We decided to keep the scribe local because it is cheap"),
                         ("assistant", "Merged as PR#268"),
                         ("user", "Always release early and exclude pending defects")])
        reply = self.scribe("")
        self.agent_catchup(reply, [
            {"msg": "m-0001", "quote": "keep the scribe local", "tag": "decision"},
            {"msg": "m-0002", "quote": "Merged as PR#268", "tag": "landed", "refs": ["PR#268"]},
            {"msg": "m-0003", "quote": "Always release early", "tag": "preference"}])

    # ---- criterion 1: catch-up ------------------------------------------

    def test_catchup_calls_pending_then_append_with_agent_entries_and_a_rerun_adds_nothing(self):
        self.transcript([("user", "We decided to keep the scribe local because it is cheap"),
                         ("assistant", "Merged as PR#268")])
        start = self.hook("SessionStart")
        self.assertEqual(start.returncode, 0, start.stderr)
        self.assertIn("hookSpecificOutput", json.loads(start.stdout), "the SessionStart brief stays")

        reply = self.scribe("")
        self.assertTrue(reply["model_call"])
        self.assertEqual(reply["agent"], {**reply["agent"], "subagent_type": "office-scribe:scribe",
                                          "level": "scribe", "model": "haiku", "effort": "medium"})
        self.assertEqual(reply["new_count"], 2)
        pending, appended = self.agent_catchup(reply, entries := [
            {"msg": "m-0001", "quote": "keep the scribe local", "tag": "decision", "topics": ["constellation"]},
            {"msg": "m-0002", "quote": "Merged as PR#268", "tag": "landed", "refs": ["PR#268"]}])
        self.assertEqual([m["id"] for m in pending["messages"]], ["m-0001", "m-0002"])
        self.assertEqual(self.office_calls(), ["office.session-brief", "office.scribe-pending", "office.scribe-append"])
        append = self.calls("office.scribe-append")[0]["input"]
        self.assertEqual(append, {"session_id": self.sid, "through": "m-0002", "entries": entries})
        self.assertEqual(appended["added"], 2)
        self.assertEqual(appended["counts"], {"decision": 1, "landed": 1})
        self.assertIn('L21 - m-0001 2026-10-30T10:00-07:00 #decision "keep the scribe local"', appended["display"])
        self.assertIn("Scribed through m-0002.", appended["display"])

        again = self.scribe("")
        self.assertFalse(again["model_call"])
        self.assertNotIn("agent", again)
        self.assertEqual(again["added"], 0)
        self.assertIn("Nothing new since m-0002", again["display"])
        self.assertEqual(len(self.calls("office.scribe-append")), 1, "a rerun with nothing new appends nothing")
        # An append without a fresh catch-up is refused by the plugin, not sent.
        self.assertIn("no_catchup_started", self.tool(
            {"session_id": self.sid, "action": "append", "entries": []}, error=True))
        self.assertEqual(len(self.calls("office.scribe-append")), 1)
        self.assert_no_model_process()

    def test_catchup_reads_only_messages_after_the_cursor_and_batches_long_backlogs(self):
        self.transcript([("user", "first decision here")])
        self.agent_catchup(self.scribe(""), [])
        long = "x" * 25_000
        self.transcript([("user", "first decision here"), ("user", long + " one"), ("user", long + " two")])
        reply = self.scribe("")
        self.assertEqual((reply["new_count"], reply["batch"]), (2, 1))
        pending, appended = self.agent_catchup(reply, [])
        self.assertEqual([(m["id"], m.get("context", False)) for m in pending["messages"]],
                         [("m-0001", True), ("m-0002", False)])
        self.assertTrue(pending["messages"][1]["truncated"])
        self.assertEqual(self.calls("office.scribe-append")[-1]["input"]["through"], "m-0002")
        self.assertIn("More messages remain", appended["display"])
        rest = self.scribe("")
        self.assertEqual(rest["new_count"], 1)

    # ---- criterion 2: model-free note and queries -----------------------

    def test_tagged_note_writes_through_append_without_a_model(self):
        self.transcript([("user", "hello scribe")])
        self.agent_catchup(self.scribe(""), [])
        before = len(self.calls("office.scribe-append"))
        reply = self.scribe("note #pending refs: ORB-1 Waiting on the gateway deploy")
        self.assertFalse(reply["model_call"])
        self.assertNotIn("agent", reply)
        self.assertEqual(self.office_calls()[-2:], ["office.scribe-pending", "office.scribe-append"])
        append = self.calls("office.scribe-append")[before]["input"]
        self.assertEqual(append, {"session_id": self.sid, "through": "m-0001", "entries": [
            {"tag": "pending", "by": "user", "refs": ["ORB-1"], "text": "Waiting on the gateway deploy"}]})
        self.assertIn("pending 1", reply["display"])
        # Run by an orchestrator, the note carries its crew.
        self.env["ORBIT_CREW"] = "opus"
        self.scribe('note #agent-decision "Kept the shift on main"')
        self.assertEqual(self.calls("office.scribe-append")[-1]["input"]["entries"],
                         [{"tag": "agent-decision", "by": "opus", "text": "Kept the shift on main"}])
        self.assert_no_model_process()

    def test_note_on_a_never_scribed_session_is_held_for_the_first_catchup(self):
        self.transcript([("user", "a first message that must stay unscribed until read")])
        held = self.scribe('note #todo "write the v1 spec" due: 2026-11-01')
        self.assertTrue(held["held"])
        self.assertFalse(held["model_call"])
        self.assertEqual(self.calls("office.scribe-append"), [], "no cursor is moved past unread messages")
        title = self.scribe("title Scribe v1 design")
        self.assertTrue(title["held"])
        self.agent_catchup(self.scribe(""), [{"msg": "m-0001", "quote": "a first message", "tag": "fact"}])
        append = self.calls("office.scribe-append")[0]["input"]
        self.assertEqual(append["entries"], [
            {"tag": "todo", "by": "user", "due": "2026-11-01", "text": "write the v1 spec"},
            {"msg": "m-0001", "quote": "a first message", "tag": "fact"}])
        self.assertEqual(append["title"], "Scribe v1 design")
        self.assertEqual(self.scribe("title Renamed")["model_call"], False)
        self.assertEqual(self.calls("office.scribe-append")[-1]["input"],
                         {"session_id": self.sid, "through": "m-0001", "entries": [], "title": "Renamed"})

    def test_untagged_note_asks_the_scribe_agent_for_a_tag(self):
        self.transcript([("user", "hello")])
        reply = self.scribe("note remember the dentist at 3pm")
        self.assertTrue(reply["model_call"])
        self.assertEqual((reply["agent"]["level"], reply["agent"]["model"]), ("scribe", "haiku"))
        self.assertIn(json.dumps("remember the dentist at 3pm"), reply["agent"]["prompt"])
        self.assertEqual(self.calls("office.scribe-append"), [])

    def test_canned_queries_call_scribe_query_without_a_model(self):
        self.seed_scribed()
        self.scribe("note #pending Waiting on the deploy")
        cases = [
            ("pending", [{"mode": "pending"}], "Waiting on the deploy"),
            ("pref release-with-open-defects", [{"mode": "pref", "matter": "release-with-open-defects"}], None),
            ("landed 2026-10-30", [{"mode": "landed", "since": "2026-10-30", "until": "2026-10-30"}], "PR#268"),
            ("why constellation", [{"mode": "why", "matter": "constellation"}, {"mode": "why", "topic": "constellation"}], None),
        ]
        for args, queries, shown in cases:
            with self.subTest(args=args):
                before = len(self.calls())
                reply = self.scribe(args)
                self.assertFalse(reply["model_call"])
                self.assertNotIn("agent", reply)
                made = self.calls()[before:]
                self.assertEqual([c["tool"] for c in made], ["office.scribe-query"] * len(queries))
                self.assertEqual([c["input"] for c in made], queries)
                if shown:
                    self.assertIn(shown, reply["display"])
                    self.assertRegex(reply["display"], r"SCR-30-0000-session-one:\d+")
        today = self.scribe("landed")
        self.assertEqual(self.calls()[-1]["input"]["mode"], "landed")
        self.assertEqual(self.calls()[-1]["input"]["since"], self.calls()[-1]["input"]["until"])
        self.assertFalse(today["model_call"])
        self.assert_no_model_process()

    # ---- criterion 3: ask -----------------------------------------------

    def test_ask_queries_first_and_a_fabricated_quote_is_dropped_before_display(self):
        self.seed_scribed()
        before = len(self.calls())
        reply = self.scribe("ask did we decide to keep the scribe local?")
        self.assertEqual([c["tool"] for c in self.calls()[before:]], ["office.scribe-query"])
        self.assertTrue(reply["model_call"])
        self.assertEqual((reply["agent"]["level"], reply["agent"]["model"], reply["agent"]["effort"]),
                         ("ask", "haiku", "low"))
        shortlist = self.tool({"session_id": self.sid, "action": "query"})["shortlist"]
        line = next(v for v in shortlist if "#decision" in v["text"])
        cited = "{}:{}".format(line["scr_id"], line["line"])
        answer = ('Yes. Daniel said "keep the scribe local" [{0} m-0001]. He also said '
                  '"move everything to the cloud" [{0} m-0001], and "cheap" without a citation.').format(cited)
        verified = self.tool({"session_id": self.sid, "action": "verify", "answer": answer})
        self.assertFalse(verified["model_call"])
        self.assertEqual((verified["kept"], verified["dropped"]), (1, 2))
        self.assertIn('"keep the scribe local" [{} m-0001]'.format(cited), verified["display"])
        self.assertNotIn("move everything to the cloud", verified["display"])
        self.assertNotIn('"cheap"', verified["display"])
        self.assertIn("2 quote(s) dropped", verified["display"])
        # The agent may narrow with filters; those lines count for the check too.
        self.tool({"session_id": self.sid, "action": "query", "query": {"mode": "landed", "since": "2026-10-30"}})
        landed = next(v for v in shortlist + self.tool({"session_id": self.sid, "action": "query"})["shortlist"]
                      if "#landed" in v["text"])
        again = self.tool({"session_id": self.sid, "action": "verify", "answer": '"Merged as PR#268" [{}:{}]'.format(
            landed["scr_id"], landed["line"])})
        self.assertEqual(again["dropped"], 0)
        anchored = self.tool({"session_id": self.sid, "action": "verify", "answer": '"Merged as PR#268" ({}#L{})'.format(
            landed["scr_id"], landed["line"])})
        self.assertEqual((anchored["kept"], anchored["dropped"]), (1, 0))
        self.assert_no_model_process()

    def test_ask_with_no_matching_line_says_not_captured_without_a_model(self):
        self.seed_scribed()
        reply = self.scribe("ask what about kubernetes?")
        self.assertFalse(reply["model_call"])
        self.assertIn("Not captured", reply["display"])
        self.assertIn("files_searched", reply["display"])

    # ---- criterion 4: backfill ------------------------------------------

    def test_backfill_since_processes_oldest_first_and_skips_a_session_scribed_to_its_end(self):
        def session(sid, date, text):
            return self.transcript([("user", text)], path=self.providers / (sid + ".jsonl"), sid=sid, date=date)
        session("ccc-new", "2026-10-20", "newest session decision")
        session("aaa-old", "2026-10-01", "oldest session decision")
        session("bbb-mid", "2026-10-10", "middle session already scribed")
        session("ddd-ancient", "2025-01-01", "before the since date")
        sub = self.providers / "aaa-old" / "subagents"
        sub.mkdir(parents=True)
        session("aaa-old", "2026-09-30", "a subagent sidechain file").rename(sub / "agent-1.jsonl")
        session("aaa-old", "2026-10-01", "oldest session decision")
        # bbb-mid was scribed to its end earlier.
        (self.home / "scr-bbb-mid.json").write_text(json.dumps({
            "session_id": "bbb-mid", "id": "SCR-10-0000-bbb-mid", "path": "sources/conversation/2026/10/SCR-10-0000-bbb-mid.md",
            "lines": [], "scribed_through": "m-0001"}))

        first = self.scribe("backfill --since 2026-09-15")
        self.assertTrue(first["continue"])
        self.assertEqual(first["session_id"], "aaa-old")
        self.agent_catchup(first, [{"msg": "m-0001", "quote": "oldest session decision", "tag": "decision"}])
        second = self.scribe("backfill --since 2026-09-15")
        self.assertEqual(second["session_id"], "ccc-new")
        self.assertEqual(second["skipped"], ["aaa-old", "bbb-mid"])
        self.agent_catchup(second, [{"msg": "m-0001", "quote": "newest session decision", "tag": "decision"}])
        third = self.scribe("backfill --since 2026-09-15")
        self.assertTrue(third["done"])
        self.assertFalse(third["model_call"])
        self.assertNotIn("agent", third)

        appended = [c["input"]["session_id"] for c in self.calls("office.scribe-append")]
        self.assertEqual(appended, ["aaa-old", "ccc-new"])
        pended = [c["input"]["session_id"] for c in self.calls("office.scribe-pending")]
        self.assertEqual(sorted(set(pended), key=pended.index), ["aaa-old", "bbb-mid", "ccc-new"])
        self.assertNotIn("ddd-ancient", pended)
        self.assertEqual(self.calls("office.scribe-pending")[0]["input"]["transcript"],
                         str(self.workspace / ".orbit/tmp/session-transcripts/scribe-aaa-old.jsonl"))
        # A finished backfill is a no-op: nothing is re-read or re-appended.
        before = len(self.calls())
        self.assertTrue(self.scribe("backfill --since 2026-09-15")["done"])
        self.assertEqual(self.calls()[before:], [])
        self.assert_no_model_process()

    def test_backfill_project_filter_and_exclusions(self):
        self.transcript([("user", "constellation work")])
        other = self.home / ".claude/projects/-elsewhere"
        other.mkdir()
        self.transcript([("user", "elsewhere work")], path=other / "zzz.jsonl", sid="zzz")
        reply = self.scribe("backfill --project constellation")
        self.assertEqual(reply["session_id"], self.sid)
        self.env["CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS"] = json.dumps([str(self.project)])
        excluded = self.scribe("backfill --project constellation")
        self.assertTrue(excluded["done"])
        self.assertEqual(self.scribe("")["excluded"], True)

    # ---- criterion 5: hooks ---------------------------------------------

    def test_automatic_stop_and_session_end_passes_are_off_by_default(self):
        self.transcript([("user", "Decision: use local evidence")] * 12)
        for event in ("Stop", "SessionEnd", "Stop"):
            result = self.hook(event)
            self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""), event)
        self.assertEqual(self.calls(), [], "no Office call, no extraction task")
        self.assertFalse((self.workspace / ".orbit").exists(), "nothing staged and no state written")
        start = self.hook("SessionStart")
        self.assertIn("hookSpecificOutput", json.loads(start.stdout))
        self.assertEqual(self.office_calls(), ["office.session-brief"])

    # ---- the tool surface -----------------------------------------------

    def test_one_structured_scribe_tool_refuses_mixed_or_unknown_fields(self):
        listed = self.mcp({"jsonrpc": "2.0", "id": 1, "method": "tools/list"})[0]["result"]["tools"]
        self.assertEqual([t["name"] for t in listed], ["scribe"])
        self.assertEqual(self.tool({"session_id": self.sid, "args": "", "action": "pending"}, error=True),
                         "unknown_command_field")
        self.assertEqual(self.tool({"session_id": self.sid, "action": "verify", "entries": []}, error=True),
                         "unknown_command_field")
        self.assertEqual(self.tool({"session_id": "../x", "args": "pending"}, error=True), "invalid_session_id")
        self.assertIn("usage: /scribe", self.scribe("frobnicate")["display"])
        self.assertEqual(self.calls(), [])


if __name__ == "__main__":
    unittest.main()
