"""Scribe v2 (docs/design/office/specs/scribe.md): opt-in sessions, the
`/scribe on|off|update|status|tags` commands, marker routing and the v2
pass the scribe agent drives through the `pending2`/`append2` steps.

Office owns the parse and every Library write: `office.scribe-pending`
with `format: "v2"` returns the model-free markers, tags and cadence, and
`office.scribe-append` v2 checks each finding against them. This module
turns the session on and off through `office.scribe-control`, files what
needs an Orbit or proposal id through the user's own `orbit` CLI, and
never calls a model itself: a pass that needs one returns an `agent`
request, as in v1.

Routing order: tasks (scheduling markers, `orbit_task` tags, `owner:you`
to-dos) are filed before the append so their ids land on the SCR entry.
A preference proposal cites the SCR entry the append creates, and
`office.proposal-create` resolves that citation, so preference proposals
are filed from the append's `needs_proposal` right after it. Every filed
item is recorded under its idempotency key (SCR id + message id + line)
in the session's routing ledger, so a retried pass files nothing twice.
Nothing is dispatched: a filed task is proposed, or an `owner:you`
to-do, never backlog `owner:agent`.
"""
from __future__ import annotations

import datetime as dt
import json
import re

from process import Failure
from scribe import load, save
from scribe_v1 import (BATCH_CHARS, CONTEXT_CHARS, MAX_ENTRIES, MESSAGE_CHARS,
                       PROVIDER, V1, repo_project)

# The closed `office.scribe-append` v2 finding shape; routing keys ride
# along to `append2` and are stripped before the append.
FINDING_KEYS = {"msg", "line", "tag", "by", "quote", "decision", "matter",
                "statement", "conflicts", "proposal", "review_by", "task", "outcome"}
ROUTING_KEYS = {"schedule", "task_title", "task_description"}
TASK_ID = re.compile(r"[A-Za-z][A-Za-z0-9_-]*-\S+\Z")
SLUG = re.compile(r"[a-z0-9][a-z0-9-]*\Z")
ZONE = re.compile(r"[A-Za-z0-9][A-Za-z0-9/._+-]{0,63}\Z")

USAGE_V2 = ("usage: /scribe [on [@<project>|@global] | off | update | status | "
            "title <text> | ask <question> | tags]")


def scope_token(rest: str) -> str | None:
    """The scope a `/scribe on` argument names: `@global` or `@<project>`."""
    words = rest.split()
    if not words:
        return None
    if len(words) != 1 or not words[0].startswith("@"):
        raise Failure("usage: /scribe on [@<project>|@global]")
    name = words[0][1:].lower()
    if name == "global":
        return "global"
    if SLUG.fullmatch(name) and "--" not in name and not name.endswith("-"):
        return "project:" + name
    raise Failure("usage: /scribe on [@<project>|@global]")


def schedule_instant(value) -> dt.datetime:
    """The agent-resolved scheduling instant, checked without a model.

    It must be an RFC 3339 instant with an offset, in the future; anything
    else files nothing (specs/scribe.md, "Scheduling markers and to-dos").
    """
    if not isinstance(value, str):
        raise Failure("schedule_time_unresolved")
    try:
        instant = dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    except ValueError:
        raise Failure("schedule_time_unresolved") from None
    if instant.tzinfo is None or instant <= dt.datetime.now(dt.timezone.utc):
        raise Failure("schedule_time_unresolved")
    return instant


def message_index(msg: str) -> int | None:
    try:
        return int(msg.split("-", 1)[1])
    except (AttributeError, IndexError, ValueError):
        return None


class V2(V1):
    # ---- control --------------------------------------------------------

    def control(self, sid: str, op: str, **fields) -> dict:
        return self.client.control(sid, op, **fields)

    def is_on(self, sid: str) -> dict | None:
        """Office's record when the session is on, else None."""
        try:
            record = self.control(sid, "get")
        except Failure:
            return None
        return record if record.get("on") is True else None

    def refuse_headless(self, sid: str) -> None:
        """A headless (`sdk-cli`) session is never scribed (trust rule 5)."""
        try:
            ctx = self.context(sid)
            from pathlib import Path
            head = self.head(Path(ctx["transcript_path"]))
        except (Failure, OSError):
            return
        if head and head.get("entrypoint") == "sdk-cli":
            raise Failure("scribe_headless_session")

    def on(self, sid: str, rest: str, by: str) -> dict:
        """`/scribe on [@…]` (`by: user`) or the orchestrator's `on` (`by: agent`)."""
        scope = scope_token(rest)
        self.refuse_headless(sid)
        fields: dict = {"on": True, "on_by": by}
        if scope:
            fields["scope"] = scope
        record = self.control(sid, "set_switch", **fields)
        with self.client.locked(sid):
            self.client.mirror(sid, True)
        result = {"on": True, "on_by": record.get("on_by") or by,
                  "scope": record.get("scope") or scope, "model_call": False}
        if by == "agent":
            # The orchestrator turned it on: it says so, and the first
            # pass starts from the Stop hook (no pass has run yet).
            result["announce"] = ("Scribing is on for this session (turned on by the "
                                  "agent). /scribe off stops it.")
            result["display"] = ("Scribing is on (on_by: agent). Tell the user in one "
                                 "line: " + result["announce"])
            return result
        # The user's /scribe on runs the first pass at once.
        try:
            first = self.update(sid)
        except Failure as error:
            result["display"] = ("Scribing is on. The first pass could not start ({}); "
                                 "run /scribe update.".format(str(error).split(";", 1)[0]))
            return result
        result.update({k: v for k, v in first.items() if k not in result or k == "agent"})
        result["model_call"] = first.get("model_call", False)
        result["display"] = "Scribing is on. " + str(first.get("display", ""))
        return result

    def off(self, sid: str) -> dict:
        self.control(sid, "set_switch", on=False)
        with self.client.locked(sid):
            self.client.mirror(sid, False)
        return {"on": False, "model_call": False,
                "display": "Scribing is off. Nothing already written is removed."}

    def status(self, sid: str) -> dict:
        try:
            record = self.control(sid, "get")
        except Failure:
            record = None
        state = load(self.state / (sid + ".json")) if self.state.is_dir() else {}
        on = bool(record and record.get("on") is True)
        view: dict = {"on": on, "on_by": record.get("on_by") if record else None,
                      "scope": record.get("scope") if record else None,
                      "model_call": False}
        lines = ["Scribing is {}.".format("on" if on else "off")]
        if on and view["on_by"]:
            lines[0] += " (on_by: {})".format(view["on_by"])
        if view["scope"]:
            lines.append("Scope: {}.".format(view["scope"]))
        if on:
            try:
                _, reply = self.pending_v2(sid, record.get("scope") if record else None)
                scr = reply.get("scr") or {}
                view.update({"scr": scr.get("id"),
                             "scribed_through": reply.get("scribed_through"),
                             "scope": reply.get("scope") or view["scope"],
                             "cadence": reply.get("cadence")})
                lines.append("SCR: {}; scribed through {}.".format(
                    scr.get("id") or "none yet", reply.get("scribed_through") or "the start"))
                due = self.next_due(state, reply.get("cadence") or {})
                view["next_pass"] = due
                lines.append("Next pass: {}.".format(due))
            except (Failure, OSError):
                lines.append("SCR state could not be read.")
        view["display"] = "\n".join(lines)
        return view

    @staticmethod
    def next_due(state: dict, cadence: dict) -> str:
        last = state.get("v2_last_pass_at")
        if not isinstance(last, (int, float)):
            return "at the next end of turn (no pass yet)"
        minutes = cadence.get("minutes", 20)
        due = dt.datetime.fromtimestamp(last + minutes * 60, dt.timezone.utc)
        return "in up to {} minutes or {} user turns (by about {})".format(
            minutes, cadence.get("turns", 10), due.isoformat())

    def tags(self, sid: str) -> dict:
        block = (self.office("office.status", {}).get("scribe") or {})
        definitions = block.get("tags") or []
        lines = ["Tags in force (cadence: {} minutes or {} user turns):".format(
            (block.get("cadence") or {}).get("minutes", 20),
            (block.get("cadence") or {}).get("turns", 10))]
        for tag in definitions:
            line = "- {}: sink {} · mode {} · scope {} · capture {}".format(
                tag.get("name"), tag.get("sink"), tag.get("mode"),
                tag.get("scope"), tag.get("capture"))
            if tag.get("workspace"):
                line += " · workspace {}".format(tag["workspace"])
            lines.append(line)
            if tag.get("instruction"):
                lines.append("  {}".format(tag["instruction"]))
        if not definitions:
            lines.append("- none defined")
        return {"display": "\n".join(lines), "tags": definitions,
                "cadence": block.get("cadence"), "model_call": False}

    # ---- pass -----------------------------------------------------------

    def pending_v2(self, sid: str, scope: str | None = None):
        ctx = self.context(sid)
        staged = self.stage(sid, ctx["transcript_path"], ctx["cwd"])
        if staged is None:
            return None, {"excluded": True}
        payload = {"session_id": sid, "transcript": str(staged), "provider": PROVIDER,
                   "cwd": ctx["cwd"], "project": repo_project(ctx["cwd"]), "format": "v2"}
        if scope:
            payload["scope"] = scope
        custom_title = getattr(self, "_custom_title", None)
        if custom_title:
            payload["custom_title"] = custom_title
        return ctx, self.office("office.scribe-pending", payload)

    def update(self, sid: str) -> dict:
        record = self.is_on(sid)
        if record is None:
            return {"display": "Scribing is off for this session; /scribe on turns it on.",
                    "on": False, "model_call": False}
        with self.client.locked(sid):
            return self.prepare(sid, record.get("scope"))

    def prepare(self, sid: str, scope: str | None) -> dict:
        _, reply = self.pending_v2(sid, scope)
        if reply.get("excluded"):
            return {"display": "Excluded project: not scribed.", "excluded": True,
                    "model_call": False}
        if reply.get("content_withheld"):
            return {"display": "Private session: nothing is given to the scribe.",
                    "withheld": True, "model_call": False, "added": 0}
        messages = reply.get("messages") or []
        new = [m for m in messages if not m.get("context")]
        through = reply.get("scribed_through")
        if not new:
            return {"display": "Nothing new since {}; nothing added.".format(through or "the start"),
                    "added": 0, "new_count": 0, "model_call": False,
                    "scribed_through": through}
        batch, size = [], 0
        for message in new:
            if batch and size + len(message.get("text", "")) > BATCH_CHARS:
                break
            batch.append(message)
            size += len(message.get("text", ""))
        last = message_index(batch[-1]["id"])
        in_batch = lambda m: (idx := message_index(m.get("msg", ""))) is not None \
            and last is not None and idx <= last
        context = [m for m in messages if m.get("context")]
        ticket = {"through": batch[-1]["id"], "more": len(batch) < len(new),
                  "messages": [self.cut(m, CONTEXT_CHARS if m.get("context") else MESSAGE_CHARS)
                               for m in context + batch],
                  "markers": [m for m in reply.get("markers") or [] if in_batch(m)],
                  "agent_markers": [m for m in reply.get("agent_markers") or [] if in_batch(m)],
                  "tags": reply.get("tags") or [], "cadence": reply.get("cadence"),
                  "scr": reply.get("scr"), "scope": reply.get("scope"),
                  "previous": reply.get("previous"), "rules": reply.get("rules") or {}}
        save(self.file(sid, "v2catchup"), ticket)
        step = json.dumps({"action": "pending2", "session_id": sid})
        prompt = ("Writing level (scribe v2). Session " + json.dumps(sid) + ". Call the scribe "
                  "tool with " + step + ", write the pass under your v2 writing rules, then call "
                  'it with {"action": "append2", "session_id": ' + json.dumps(sid)
                  + ', "sections": {...}, "findings": [...]}. Return only the tool\'s display.')
        return {"display": "{} new message(s) after {}; the scribe agent reads {} now{}.".format(
                    len(new), through or "the start", len(batch),
                    " (run /scribe update again for the rest)" if ticket["more"] else ""),
                "agent": self.agent("scribe", "Scribe update", prompt, reply.get("rules")),
                "model_call": True, "new_count": len(new), "batch": len(batch),
                "session_id": sid}

    def step_pending_v2(self, sid: str) -> dict:
        ticket = load(self.file(sid, "v2catchup"))
        if not ticket:
            raise Failure("no_update_started; run /scribe update")
        return {"session_id": sid, "through": ticket["through"], "scr": ticket.get("scr"),
                "scope": ticket.get("scope"), "previous": ticket.get("previous"),
                "tags": ticket.get("tags"), "cadence": ticket.get("cadence"),
                "markers": ticket.get("markers", []),
                "agent_markers": ticket.get("agent_markers", []),
                "messages": ticket.get("messages", []),
                "note": "Messages marked context are read-only. Findings are one per marker: "
                        "{msg, line, tag, by, quote, decision|matter+statement+conflicts, "
                        "schedule:{not_before,title}, task_title}. Sections are "
                        "{summary, goals[], outcome[], next[]}."}

    # ---- routing --------------------------------------------------------

    def routed(self, sid: str) -> dict:
        value = load(self.file(sid, "routed"))
        entries = value.get("entries")
        return entries if isinstance(entries, dict) else {}

    def remember(self, sid: str, entries: dict) -> None:
        save(self.file(sid, "routed"), {"entries": entries})

    def file_task(self, entries: dict, key: str, kind: str, payload: dict,
                  report: dict) -> str | None:
        """One idempotent `orbit.task.add`; proposed or `owner:you` only."""
        known = entries.get(key)
        if isinstance(known, dict) and isinstance(known.get("id"), str):
            report["tasks"].append({"key": key, "kind": kind, "task": known["id"],
                                    "duplicate": True})
            return known["id"]
        if "owner:agent" in payload.get("tags", []):
            raise Failure("scribe_never_files_owner_agent")
        payload.setdefault("workspace", str(self.client.workspace))
        payload["model"] = "claude"
        payload.setdefault("complexity", "low")
        payload.setdefault("type", "chore")
        payload.setdefault("origin", "automation")
        reply = self.office("orbit.task.add", payload)
        node = reply.get("task") if isinstance(reply.get("task"), dict) else reply
        task_id, status = node.get("id"), node.get("status")
        if not isinstance(task_id, str) or not TASK_ID.fullmatch(task_id):
            report["failures"].append({"key": key, "reason": "task_reply_missing_id"})
            return None
        entries[key] = {"kind": kind, "id": task_id, "status": status}
        if kind != "todo" and status not in (None, "proposed"):
            report["failures"].append({"key": key, "reason": "task_not_proposed",
                                        "task": task_id, "status": status})
            return None
        report["tasks"].append({"key": key, "kind": kind, "task": task_id})
        return task_id

    def route_tasks(self, sid: str, ticket: dict, findings: list[dict],
                    sides: list[dict], report: dict) -> dict:
        """File the tasks that need an id before the append. Returns the ledger."""
        entries = self.routed(sid)
        scr_id = (ticket.get("scr") or {}).get("id") or "SCR-unknown"
        markers = {(m["msg"], m["line"]): m
                   for m in (ticket.get("markers") or []) + (ticket.get("agent_markers") or [])}
        tags = {t.get("name"): t for t in ticket.get("tags") or []}
        for finding, side in zip(findings, sides):
            marker = markers.get((finding.get("msg"), finding.get("line")), {})
            by = marker.get("by") or finding.get("by") or "user"
            key = "{}:{}:{}".format(scr_id, finding.get("msg"), finding.get("line"))
            schedule, definition = side.get("schedule"), tags.get(finding.get("tag") or "")
            if by == "agent":
                # An agent marker is recorded, never filed: no schedule,
                # no task id reaches the append (trust rule 4).
                finding.pop("task", None)
                continue
            if isinstance(schedule, dict):
                quote = marker.get("quote") or finding.get("quote") or ""
                try:
                    instant = schedule_instant(schedule.get("not_before"))
                except Failure:
                    report["failures"].append({"key": key, "reason": "schedule_time_unresolved"})
                    finding.setdefault("outcome", "reported")
                    continue
                zone = schedule.get("zone") if isinstance(schedule.get("zone"), str) \
                    and ZONE.fullmatch(schedule["zone"]) else (instant.tzname() or "local")
                title = schedule.get("title") if isinstance(schedule.get("title"), str) \
                    and schedule["title"].strip() else "Scheduled from a scribe marker"
                task_id = self.file_task(entries, key, "schedule", {
                    "title": title.strip()[:200],
                    "description": "Filed by the scribe from a scheduling marker. It stays "
                                   "proposed until approved; nothing is dispatched.\n\n"
                                   "Marker (verbatim): {}\n\nResolved time: {} ({})\n"
                                   "SCR: {} message {} line {}\nScribe-Key: {}".format(
                                       quote, instant.isoformat(), zone, scr_id,
                                       finding.get("msg"), finding.get("line"), key),
                    "not_before": instant.isoformat(),
                    "tags": ["office", "scribe"]}, report)
                if task_id:
                    finding["task"] = task_id
                continue
            if definition and definition.get("sink") == "orbit_task" and "task" not in finding:
                quote = marker.get("quote") or finding.get("quote") or marker.get("text") or ""
                title = side.get("task_title") if isinstance(side.get("task_title"), str) \
                    and side["task_title"].strip() else (marker.get("text") or quote)
                payload = {"title": str(title).strip()[:200] or "Scribe tag finding",
                           "description": "Filed by the scribe for the #{} tag. It stays proposed "
                                          "until approved.\n\nMarker (verbatim): {}\n\n"
                                          "SCR: {} message {} line {}\nScribe-Key: {}".format(
                                              finding.get("tag"), quote, scr_id,
                                              finding.get("msg"), finding.get("line"), key),
                           "tags": ["office", "scribe"]}
                if definition.get("workspace"):
                    payload["workspace"] = definition["workspace"]
                if isinstance(side.get("task_description"), str) and side["task_description"].strip():
                    payload["description"] += "\n\n" + side["task_description"]
                task_id = self.file_task(entries, key, "orbit_task", payload, report)
                if task_id:
                    finding["task"] = task_id
        return entries

    def route_todos(self, sid: str, ticket: dict, todos, entries: dict, report: dict) -> list[str]:
        """`owner:you` to-dos for user-owned action items; Next lines for new ones."""
        if todos is None:
            return []
        if not isinstance(todos, list) or len(todos) > MAX_ENTRIES \
                or any(not isinstance(t, dict) for t in todos):
            raise Failure("todos_must_be_a_list_of_objects")
        scr_id = (ticket.get("scr") or {}).get("id") or "SCR-unknown"
        added = []
        for todo in todos:
            if set(todo) - {"msg", "line", "title"} or not isinstance(todo.get("title"), str) \
                    or not todo["title"].strip() or not isinstance(todo.get("msg"), str):
                raise Failure("todo_needs_msg_and_title")
            key = "{}:{}:{}".format(scr_id, todo["msg"], todo.get("line", 0))
            known = entries.get(key)
            task_id = self.file_task(entries, key, "todo", {
                "title": todo["title"].strip()[:200],
                "description": "Filed by the scribe as your to-do (owner:you); no dispatcher "
                               "runs it.\n\nSCR: {} message {}\nScribe-Key: {}".format(
                                   scr_id, todo["msg"], key),
                "tags": ["office", "scribe", "owner:you"]}, report)
            if task_id and not (isinstance(known, dict) and known.get("id") == task_id):
                added.append("{} — {}. owner: you.".format(task_id, todo["title"].strip()))
        return added

    def file_proposals(self, sid: str, ticket: dict, reply: dict, findings: list[dict],
                       entries: dict, report: dict) -> None:
        """One `office.proposal-create` per `needs_proposal` preference finding."""
        by_msg: dict[str, dict] = {}
        for message in ticket.get("messages") or []:
            by_msg[message.get("id")] = message
        wanted = {(f.get("msg"), f.get("matter")): f for f in findings}
        for item in reply.get("needs_proposal") or []:
            if item.get("proposal"):
                continue
            cite = item.get("cite") or ""
            key = cite or "{}:{}".format(item.get("msg"), item.get("matter"))
            known = entries.get(key)
            if isinstance(known, dict) and isinstance(known.get("id"), str):
                report["proposals"].append({"key": key, "proposal": known["id"],
                                             "duplicate": True})
                continue
            finding = wanted.get((item.get("msg"), item.get("matter")), {})
            try:
                proposal = self.preference_proposal(item, finding, by_msg)
            except Failure as error:
                report["failures"].append({"key": key,
                                            "reason": str(error).split(";", 1)[0]})
                continue
            node = proposal.get("task") if isinstance(proposal.get("task"), dict) else proposal
            proposal_id = node.get("id") if isinstance(node, dict) else None
            if not isinstance(proposal_id, str) or not proposal_id:
                report["failures"].append({"key": key, "reason": "proposal_reply_missing_id"})
                continue
            entries[key] = {"kind": "proposal", "id": proposal_id}
            report["proposals"].append({"key": key, "proposal": proposal_id,
                                         "suppressed": bool(proposal.get("suppressed"))})

    def preference_proposal(self, item: dict, finding: dict, by_msg: dict) -> dict:
        """The proposed preference page: current sections, this matter replaced."""
        page_id, matter, statement = item["page"], item["matter"], item["statement"]
        try:
            shown = self.office("office.knowledge-show", {"id": page_id})
            page, exists = shown.get("page") or {}, True
        except Failure as error:
            if str(error) != "page_not_found":
                raise
            page, exists = {}, False
        sections: dict[str, str] = {}
        current = None
        for line in str(page.get("body") or "").splitlines(keepends=True):
            if line.startswith("## "):
                current = line[3:].strip()
                sections[current] = ""
            elif current is not None:
                sections[current] += line
        at = str((by_msg.get(item.get("msg")) or {}).get("at") or "")
        try:
            stated = dt.datetime.fromisoformat(at.replace("Z", "+00:00")).astimezone().date()
        except ValueError:
            stated = dt.datetime.now().astimezone().date()
        review = finding.get("review_by") or (stated + dt.timedelta(days=90)).isoformat()
        sections[matter] = ("statement: {}\ncites: {}\nstated_at: {}\nreview_by: {}\n".format(
            json.dumps(statement), json.dumps([item["cite"]]),
            stated.isoformat(), review))
        scope_name = page_id.removeprefix("dom-preferences-")
        title = page.get("title") or ("Global preferences" if scope_name == "global"
                                      else "{} preferences".format(scope_name))
        body = "".join("## {}\n{}".format(name, text if text.endswith("\n") else text + "\n")
                       for name, text in sections.items())
        markdown = "---\nid: {}\ntitle: {}\ntemplate: preferences\n---\n{}".format(
            page_id, json.dumps(title), body)
        payload: dict = {"kind": "knowledge_update", "markdown": markdown,
                         "base_sha256": page.get("sha256")} if exists else \
            {"kind": "knowledge_add", "markdown": markdown}
        conflicts = [c for c in item.get("conflicts") or []
                     if isinstance(c, dict) and c.get("matter") != matter]
        if conflicts:
            payload["conflicts"] = conflicts
        return self.office("office.proposal-create", {
            "title": "Preference: {}".format(matter),
            "description": "Scribe preference finding (by: {}, {}): {}\n\nAccepting replaces "
                           "the {} section of {}.".format(item.get("by"), item["cite"],
                                                           statement, matter, page_id),
            "evidence": [item["cite"]], "payload": payload, "tags": ["scribe"]})

    # ---- append ---------------------------------------------------------

    def step_append_v2(self, sid: str, sections, findings, todos=None,
                       title=None, title_by=None) -> dict:
        if not isinstance(sections, dict) or not isinstance(sections.get("summary"), str):
            raise Failure("sections_must_be_an_object_with_a_summary")
        if not isinstance(findings, list) or len(findings) > MAX_ENTRIES \
                or any(not isinstance(f, dict) for f in findings):
            raise Failure("findings_must_be_a_list_of_objects")
        clean, sides = [], []
        for finding in findings:
            if set(finding) - FINDING_KEYS - ROUTING_KEYS:
                raise Failure("unknown_finding_field")
            sides.append({k: finding[k] for k in ROUTING_KEYS if k in finding})
            clean.append({k: v for k, v in finding.items() if k in FINDING_KEYS})
        if title is not None and (not isinstance(title, str) or not title.strip()):
            raise Failure("title_required")
        if title_by is not None and title_by not in ("user", "scribe"):
            raise Failure("title_by_must_be_user_or_scribe")
        with self.client.locked(sid):
            ticket = load(self.file(sid, "v2catchup"))
            if not ticket:
                raise Failure("no_update_started; run /scribe update")
            report: dict = {"tasks": [], "proposals": [], "failures": []}
            entries = self.route_tasks(sid, ticket, clean, sides, report)
            extra_next = self.route_todos(sid, ticket, todos, entries, report)
            # Persist before the append: a pass retried after a failed
            # append must find its filed tasks here, never file them twice.
            self.remember(sid, entries)
            if extra_next:
                sections = dict(sections)
                sections["next"] = list(sections.get("next") or []) + extra_next
            payload = {"session_id": sid, "format": "v2", "through": ticket["through"],
                       "sections": sections, "findings": clean}
            if title:
                payload["title"], payload["title_by"] = title, title_by or "scribe"
            reply = self.office("office.scribe-append", payload)
            self.file_proposals(sid, ticket, reply, clean, entries, report)
            self.remember(sid, entries)
            self.drop(sid, "v2catchup")
            return self.render_v2(reply, report, bool(ticket.get("more")))

    @staticmethod
    def render_v2(reply: dict, report: dict, more: bool) -> dict:
        added = reply.get("added") or []
        lines = []
        if reply.get("written"):
            lines.append("Scribed through {} in {} ({}): {} entr{}.".format(
                reply.get("scribed_through"), reply.get("id"), reply.get("path"),
                len(added), "y" if len(added) == 1 else "ies"))
        else:
            lines.append("Nothing written to {}.".format(reply.get("id")))
        for entry in added:
            lines.append("  {} {} {}".format(entry.get("id"), entry.get("group"), entry.get("msg")))
        if reply.get("ledger"):
            lines.append("Decisions appended: {}.".format(
                ", ".join(str(e.get("id")) for e in reply["ledger"])))
        for task in report["tasks"]:
            lines.append("Filed {} task {}{}.".format(
                task["kind"], task["task"],
                " (already filed)" if task.get("duplicate") else ""))
        for proposal in report["proposals"]:
            lines.append("Preference proposal {}{}.".format(
                proposal["proposal"], " (already filed)" if proposal.get("duplicate") else ""))
        if reply.get("reported"):
            lines.append("Reported, not filed: {} marker(s).".format(len(reply["reported"])))
        if reply.get("refused"):
            lines.append("Refused {} (not written):".format(len(reply["refused"])))
            lines += ["  {} line {} {}".format(r.get("msg"), r.get("line"), r.get("code"))
                      for r in reply["refused"]]
        for failure in report["failures"]:
            lines.append("Routing failed for {}: {}.".format(failure["key"], failure["reason"]))
        if reply.get("duplicates"):
            lines.append("{} duplicate(s) skipped.".format(len(reply["duplicates"])))
        if more:
            lines.append("More messages remain: run /scribe update again.")
        return {"display": "\n".join(lines), "added": len(added),
                "refused": len(reply.get("refused") or []), "routed": report,
                "scribed_through": reply.get("scribed_through"), "more": more,
                "path": reply.get("path"), "model_call": False}


def route_v2(v1: V1, sid: str, head: str, rest: str) -> dict:
    """The v2 command words of `/scribe`, sharing the caller's client."""
    v2 = V2(v1.client)
    if head == "on":
        return v2.on(sid, rest, by="user")
    if head == "off":
        if rest:
            raise Failure("usage: /scribe off")
        return v2.off(sid)
    if head in ("update", "now"):
        if rest:
            raise Failure("usage: /scribe update")
        return v2.update(sid)
    if head == "status":
        if rest:
            raise Failure("usage: /scribe status")
        return v2.status(sid)
    if head == "tags":
        if rest:
            raise Failure("usage: /scribe tags")
        return v2.tags(sid)
    raise Failure("unknown_scribe_command")
