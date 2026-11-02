"""Claude Code client. All Library access is through `orbit tool run office.*`."""
from __future__ import annotations

import contextlib
import datetime as dt
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import socket
import stat
import subprocess
import sys
import time
import uuid

from process import Failure, NotSent, run

MAX_TRANSCRIPT = 8 * 1024 * 1024
SESSION = re.compile(r"[A-Za-z0-9_-]{1,128}\Z")
TERMINAL = {"done", "rejected", "archived", "cancelled", "canceled"}
# Thread matching uses the provider name conversation sources record as their
# `orchestrator` (the transcript adapter writes `claude`), never a crew name:
# neither the transcript nor this hook knows the crew (specs/sessions.md).
ORCHESTRATOR = "claude"
TASK_ID = re.compile(r"[A-Za-z0-9_-]{1,64}\Z")
SCAN_LIMIT = 2000
HANDOFF_CALL = 30.0
TOOLS = {"office.session-brief", "office.capture", "office.recall", "office.scribe-control",
         "office.scribe-pending", "office.scribe-append", "office.scribe-query",
         "office.status", "office.proposal-create", "office.knowledge-show",
         "orbit.task.add", "orbit.task.show"}
# Scribe v2 cadence when Office cannot be read (specs/scribe.md defaults).
DEFAULT_CADENCE = {"minutes": 20, "turns": 10}


def repo_project(cwd: str) -> str:
    """The repository name for a working directory (path rule only, no git).

    A Claude Code worktree cwd (`<repo>/.claude/worktrees/<name>`) names its
    repository; otherwise the directory's own name. This mirrors the name
    conversation sources record as their project.
    """
    parts = [part for part in cwd.rstrip("/").split("/") if part]
    if len(parts) >= 4 and parts[-2] == "worktrees" and parts[-3] == ".claude":
        return parts[-4]
    return parts[-1] if parts else cwd


def parent_of_repositories(cwd: str) -> bool:
    """Whether `cwd` holds two or more repositories in its children or grandchildren.

    Orchestrators often start in a root such as the constellation checkout,
    which names no single repository. Symlinks are not followed and the scan
    is bounded, so a large directory costs at most `SCAN_LIMIT` entries.
    """
    found = 0
    seen = 0
    level = [Path(cwd)]
    for _ in range(2):
        below = []
        for directory in level:
            try:
                entries = list(os.scandir(directory))
            except OSError:
                continue
            for entry in entries:
                seen += 1
                if seen > SCAN_LIMIT:
                    return False
                try:
                    if not entry.is_dir(follow_symlinks=False) or entry.name.startswith("."):
                        continue
                except OSError:
                    continue
                if os.path.lexists(os.path.join(entry.path, ".git")):
                    found += 1
                    if found >= 2:
                        return True
                else:
                    below.append(Path(entry.path))
        level = below
    return False


def assignment_task() -> str | None:
    """The caller's assignment task id, when Orbit set one for this session."""
    for name in ("ORBIT_ACTIVE_TASK_ID", "ORBIT_TASK_ID"):
        value = os.environ.get(name, "")
        if TASK_ID.fullmatch(value):
            return value
    return None


def session_start_brief(cwd) -> dict:
    """The SessionStart `office.session-brief` request (specs/sessions.md).

    Office applies `brief.handoff`: in `auto` a single repository names the
    project; a parent of several repositories passes the caller's assignment
    instead, and with neither Office returns the manual output.
    """
    request = {"session_start": True, "orchestrator": ORCHESTRATOR}
    if isinstance(cwd, str) and cwd:
        if not parent_of_repositories(cwd):
            request["project"] = repo_project(cwd)
        elif (task := assignment_task()) is not None:
            request["task"] = task
    return request


def render_handoff(project: str | None, brief: dict) -> str:
    """Plain text for `/handoff`: quoted, cited entries; data, never instructions."""
    sections = {s.get("kind"): s for s in brief.get("sections", []) if isinstance(s, dict)}

    def lines(kind: str) -> list[str]:
        out = []
        for item in sections.get(kind, {}).get("items", []):
            if kind == "projects":
                out.append("- {} (last handoff {})".format(item.get("text"), item.get("time")))
            else:
                marker = "(legacy) " if item.get("legacy") else ""
                out.append("- [{}] {}{} ({})".format(item.get("time"), marker, item.get("text"), item.get("id")))
        section = sections.get(kind, {})
        if section.get("truncated"):
            out.append("- … {} in total".format(section.get("total")))
        return out

    text = []
    if project is not None:
        handoff = lines("handoff")
        text.append("Handoff for {}:".format(project))
        text.extend(handoff or ["- none recorded for this project"])
    globals_ = lines("global")
    text.append("Global handoffs:")
    text.extend(globals_ or ["- none open"])
    if project is None:
        text.append("Projects with a handoff (newest first):")
        text.extend(lines("projects") or ["- none yet"])
        text.append("Run /handoff <project> to load one.")
    return "\n".join(text)


def user_turn(record: dict) -> bool:
    if record.get("type") != "user" or record.get("isMeta") or record.get("isSidechain"):
        return False
    message = record.get("message", {})
    if not isinstance(message, dict) or message.get("role") != "user":
        return False
    content = message.get("content")
    if isinstance(content, str):
        return bool(content.strip())
    return isinstance(content, list) and any(isinstance(block, dict) and block.get("type") == "text"
        and isinstance(block.get("text"), str) and block["text"].strip() for block in content)


# Harness wrapper text in a user record is not the user's words
# (specs/scribe.md, "Pipeline"): dropped before a marker is looked for.
HARNESS_BLOCK = re.compile(
    r"<(system-reminder|task-notification|local-command-[a-z-]+|command-message|command-name"
    r"|command-args|pasted_content)(?:\s[^>]*)?>.*?(?:</\1>|\Z)", re.S)
# A desktop side session's opening brief is the orchestrator's text.
SIDE_BRIEF = re.compile(r"\s*background\s*\(?from the session that started this one\)?:?",
                        re.I)
# Marker words that name a default tag (aliases included).
MARKER_ALIASES = {"preferences": "preferences", "preference": "preferences",
                  "decisions": "decisions", "decision": "decisions"}


def user_text(record: dict) -> str:
    """The typed text of a user record, harness wrapper blocks removed."""
    content = record.get("message", {}).get("content")
    if isinstance(content, str):
        text = content
    else:
        text = "\n".join(block["text"] for block in content or []
                         if isinstance(block, dict) and block.get("type") == "text"
                         and isinstance(block.get("text"), str))
    return HARNESS_BLOCK.sub("", text)


def marked_tags(text: str, defined: set[str]) -> set[str]:
    """The defined tags a user turn marks, read as Office reads markers.

    A marker starts a line: `#preferences`/`#preference`,
    `#decision`/`#decisions`, or `#scribe <tag>`. Lines in a fenced block or
    a blockquote are not markers, and a side session's opening brief is
    not the user's turn. Office re-detects every marker at the pass, so
    this only decides whether a Stop is due (specs/scribe.md, "Cadence").
    """
    if SIDE_BRIEF.match(text):
        return set()
    found: set[str] = set()
    fenced = False
    for raw in text.splitlines():
        line = raw.strip()
        if line.startswith(("```", "~~~")):
            fenced = not fenced
            continue
        if fenced or line.startswith(">") or not line.startswith("#"):
            continue
        words = line[1:].split()
        if not words:
            continue
        word = words[0].lower()
        if word == "scribe":
            name = words[1] if len(words) > 1 else ""
            name = MARKER_ALIASES.get(name, name)
        else:
            name = MARKER_ALIASES.get(word, "")
        if name in defined:
            found.add(name)
    return found


def check_dir(path: Path, private: bool) -> None:
    """Refuse a state directory that is a symlink, foreign or too open. Never follows a link.

    `scribe/` and `session-transcripts/` are private: no group or other bits. An
    existing parent such as Orbit's `.orbit/tmp` may keep the mode Orbit gave it
    (0755 is common), but must not be group- or other-writable, since those bits
    let another account rename or replace the state beneath it.
    """
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid():
        raise Failure("unsafe_state_directory")
    if info.st_mode & (0o077 if private else 0o022):
        raise Failure("unsafe_state_directory")


def private_dir(path: Path, private: bool = True) -> Path:
    """Create/validate without following a symlink in any state component.

    A missing component is created 0700. Existing components are checked by `check_dir`.
    """
    if not path.parent.exists():
        private_dir(path.parent, private=False)
    try:
        path.mkdir(mode=0o700)
    except FileExistsError:
        pass
    check_dir(path, private)
    return path


def read_file(path: Path, limit: int, private: bool = False) -> bytes:
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid():
            raise Failure("unsafe_input_file")
        if private and info.st_mode & 0o077:
            raise Failure("unsafe_state_file")
        data = stream.read(limit + 1)
        if len(data) > limit:
            raise Failure("input_too_large")
        return data


def load(path: Path) -> dict:
    try:
        value = json.loads(read_file(path, MAX_TRANSCRIPT, private=True))
    except FileNotFoundError:
        return {}
    if not isinstance(value, dict):
        raise Failure("invalid_state")
    return value


def publish(path: Path, data: bytes) -> None:
    """Owner-only atomic replacement; never follow the destination."""
    if path.is_symlink():
        raise Failure("unsafe_state_file")
    tmp = path.with_name("." + path.name + "." + uuid.uuid4().hex)
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(tmp, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        if tmp.exists():
            tmp.unlink()  # this invocation created this exact temporary file


def save(path: Path, value: dict) -> None:
    publish(path, json.dumps(value, sort_keys=True).encode())


class Scribe:
    def __init__(self):
        # A local absolute checkout is itself a supported Orbit workspace selector.
        # Remote selectors are deliberately refused: raw transcripts stay here.
        raw = os.environ.get("CLAUDE_PLUGIN_OPTION_WORKSPACE", "")
        if not raw or not Path(raw).is_absolute():
            raise Failure("configure_local_office_workspace")
        self.workspace = Path(raw).resolve(strict=True)
        if not self.workspace.is_dir():
            raise Failure("configure_local_office_workspace")
        excluded = json.loads(os.environ.get("CLAUDE_PLUGIN_OPTION_EXCLUDED_PROJECTS", "[]"))
        if not isinstance(excluded, list) or any(not isinstance(p, str) or not Path(p).expanduser().is_absolute() for p in excluded):
            raise Failure("excluded_projects_must_be_absolute_paths")
        self.excluded = [Path(p).expanduser().resolve() for p in excluded]
        self.default_on = os.environ.get("CLAUDE_PLUGIN_OPTION_DEFAULT_ON", "false").lower() == "true"
        # Scribe v1: `/scribe` is the only trigger. The automatic Stop and
        # SessionEnd passes run only when the user turns this option on.
        self.auto_capture = os.environ.get("CLAUDE_PLUGIN_OPTION_AUTO_CAPTURE", "false").lower() == "true"
        self.timeout = max(0.05, min(2.0, float(os.environ.get("CLAUDE_PLUGIN_OPTION_TIMEOUT", "1"))))
        self.started = time.monotonic()
        # Do not create local state for recall-only calls.
        self.state = self.workspace / ".orbit/tmp/scribe"
        self.staging = self.workspace / ".orbit/tmp/session-transcripts"
        # Office's shared control record, read once per end-of-turn hook.
        self.shared: dict | None = None

    def setup(self):
        # Workspace/.orbit may already be public metadata. Orbit may have created
        # .orbit/tmp at its own mode, so that parent only has to be ours, unlinked and
        # not writable by others; scribe/ and session-transcripts/ are the private state.
        orbit = self.workspace / ".orbit"
        if orbit.is_symlink():
            raise Failure("unsafe_state_directory")
        orbit.mkdir(mode=0o700, exist_ok=True)
        private_dir(self.workspace / ".orbit/tmp", private=False)
        private_dir(self.state)
        private_dir(self.staging)

    def call(self, tool: str, payload: dict, cap: float = 2.0) -> dict:
        """One Orbit tool call within the shared budget; `cap` bounds this call alone."""
        if tool not in TOOLS:
            raise Failure("tool_not_allowed")
        remaining = self.timeout - (time.monotonic() - self.started)
        if remaining <= 0:
            raise NotSent("hook_deadline; request_not_sent")
        return run(["orbit", "tool", "run", tool, "--json", "--full", "--workspace", str(self.workspace),
                    "--input", json.dumps(payload)], str(self.workspace), min(cap, remaining))

    def excluded_project(self, project: str) -> bool:
        if not isinstance(project, str) or not Path(project).is_absolute():
            raise Failure("missing_absolute_project")
        actual = Path(project).resolve()
        return any(actual == root or root in actual.parents for root in self.excluded)

    def session_id(self, value: str) -> str:
        if not isinstance(value, str) or not SESSION.fullmatch(value):
            raise Failure("invalid_session_id")
        return value

    @contextlib.contextmanager
    def locked(self, sid: str):
        self.setup()
        fd = os.open(self.state / (sid + ".lock"), os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        try:
            if not stat.S_ISREG(os.fstat(fd).st_mode) or os.fstat(fd).st_uid != os.getuid():
                raise Failure("unsafe_state_lock")
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                raise Failure("session_busy; retry_later") from None
            yield
        finally:
            os.close(fd)

    def control(self, sid: str, op: str, **fields) -> dict:
        """One `office.scribe-control` call: the Office-owned record both hosts share."""
        return self.call("office.scribe-control", {"op": op, "session_id": sid, **fields})

    def shared_state(self, sid: str) -> dict | None:
        """Office's record for this session, or None when Orbit cannot answer.

        Hooks fail open: an unreachable Orbit, an older office plugin without
        the tool, or the hook deadline fall back to the local cache files.
        """
        try:
            state = self.control(sid, "get")
        except Failure:
            return None
        return state if isinstance(state.get("window_open"), bool) else None

    def window_open(self, sid: str, shared: dict | None = None) -> bool:
        """True while a Scribe window heartbeat for this session is fresh.

        Office's record is authoritative (`window_open`: a heartbeat under two
        minutes old); the local heartbeat file is the cache used when Office
        cannot be read, and is refreshed by this host's own pane.
        """
        if shared is not None and shared.get("window_open") is True:
            return True
        try:
            heartbeat = self.state / (sid + ".window-open")
            if heartbeat.is_symlink():
                return False
            return time.time() - heartbeat.stat().st_mtime < 120
        except OSError:
            return False

    def heartbeat(self, sid: str) -> dict:
        """Record the window-open heartbeat in Office and refresh the local cache."""
        self.session_id(sid)
        self.setup()
        publish(self.state / (sid + ".window-open"), b"open\n")
        shared = self.control(sid, "heartbeat")
        return {"window_open": shared.get("window_open") is True}

    def mirror(self, sid: str, on: bool) -> None:
        """Write the shared switch into the local flag file (its cache)."""
        flag = self.state / sid
        if flag.is_symlink():
            raise Failure("unsafe_switch_file")
        if on:
            publish(flag, b"on\n")
        elif flag.exists():
            flag.unlink()  # owned session switch, not captured evidence

    def switch(self, sid: str, state: dict, shared: dict | None = None) -> bool:
        flag = self.state / sid
        if shared is not None and isinstance(shared.get("on"), bool):
            # Office's switch wins; the flag file mirrors it.
            self.mirror(sid, shared["on"])
            state["initialized"] = True
        elif not state.get("initialized"):
            if self.default_on:
                publish(flag, b"on\n")
            state["initialized"] = True
        if flag.is_symlink():
            raise Failure("unsafe_switch_file")
        if flag.exists():
            read_file(flag, 16, private=True)
        return flag.exists()

    def snapshot(self, context: dict, mode: str) -> dict | None:
        sid = self.session_id(context.get("session_id"))
        # Pin physical input under the current user's provider root, not arbitrary files.
        root = (Path.home() / ".claude/projects").resolve(strict=True)
        source = Path(context.get("transcript_path", "")).resolve(strict=True)
        if root not in source.parents or source.suffix != ".jsonl":
            raise Failure("transcript_outside_provider_root")
        data = read_file(source, MAX_TRANSCRIPT)
        if mode == "live":
            data = data[:data.rfind(b"\n") + 1]
        if not data:
            return None
        records = [json.loads(line) for line in data.splitlines() if line.strip()]
        if any(not isinstance(r, dict) for r in records):
            raise Failure("invalid_transcript_record")
        # Check every cwd, including a changed project later in the transcript.
        projects = [context.get("cwd")] + [r["cwd"] for r in records if r.get("cwd")]
        if any(self.excluded_project(p) for p in projects):
            return None
        conversation = [r for r in records if r.get("type") in ("user", "assistant")]
        if not conversation or any(r.get("sessionId") != sid for r in conversation):
            raise Failure("mixed_or_missing_session")
        digest = hashlib.sha256(data).hexdigest()
        # Distinct from the retired constellation stager; immutable bytes per digest/mode.
        base = self.staging / ("provider-scribe-" + sid + "-" + digest + "-" + mode)
        path = base.with_suffix(".jsonl")
        descriptor = base.with_suffix(".json")
        if not descriptor.exists():
            publish(path, data)
            save(descriptor, {"session_id": sid, "transcript_path": str(path), "transcript_sha256": digest,
                              "completed_at": dt.datetime.now(dt.timezone.utc).isoformat(), "mode": mode,
                              "byte_length": len(data), "project": context["cwd"]})
        return {"descriptor": str(descriptor), "path": str(path), "digest": digest, "length": len(data),
                "turns": sum(user_turn(r) for r in conversation), "mode": mode}

    def queue(self, sid: str, state: dict, snap: dict, force: bool = False) -> dict:
        key = snap["digest"] if snap["mode"] == "final" else str(snap["length"])
        key = snap["mode"] + ":" + key
        if key in state.get("queued", []):
            return {"queued": False, "reason": "already_queued"}
        predecessor = None
        pending = state.get("pending")
        if pending:
            task = self.call("orbit.task.show", {"id": pending, "model": "claude", "fields": ["id", "status"]})
            if task.get("status") not in TERMINAL:
                if snap["mode"] != "final":
                    return {"queued": False, "reason": "pass_pending", "task": pending}
                predecessor = pending
            state.pop("pending", None)
        if state.get("queue_unknown"):
            raise Failure("queue_outcome_unknown; inspect_Orbit_before_retry")
        now = time.time()
        if not force and snap["mode"] == "live" and state.get("last_queued_at") is not None:
            # While a Scribe window (web screen or desktop pane) is open on
            # this session it refreshes a window-open heartbeat; the turn
            # bound tightens to 2 (specs/recall.md, "Pace while the window
            # is open"). The time bound is unchanged.
            turns_needed = 2 if self.window_open(sid, self.shared) else 8
            if now - state["last_queued_at"] < 300 and snap["turns"] - state.get("last_turns", 0) < turns_needed:
                return {"queued": False, "reason": "debounced"}
        # An indeterminate mutation is never blindly retried. Staged descriptor survives.
        state["queue_unknown"] = True
        save(self.state / (sid + ".json"), state)
        instructions = (Path(__file__).parent.parent / "extraction.md").read_text()
        try:
            task = self.call("orbit.task.add", {
                "workspace": str(self.workspace), "model": "claude", "title": "Extract conversation evidence",
                "description": instructions + "\n\nLocal session id: " + json.dumps(sid) +
                    "\nInitial descriptor: " + json.dumps(snap["descriptor"]) +
                    "\nBefore finishing, also process newer provider-scribe descriptors for this session.\n",
                "acceptance_criteria": ["Capture each supported staged session snapshot, or record an explicit refusal without private text."],
                "complexity": "low", "type": "chore", "priority": "low", "origin": "automation",
                "tags": ["office", "office-conversation-extract", "owner:agent", "no-diff-expected", "os:" + ("macos" if sys.platform == "darwin" else "linux")],
                "requires_labels": ["office-scribe-" + socket.gethostname()],
                "required_tools": ["office.capture", "orbit.task.show", "orbit.task.artifact.put"],
                **({"relations": [{"type": "blocked_by", "target": predecessor}]} if predecessor else {})})
        except NotSent:
            state.pop("queue_unknown", None)
            save(self.state / (sid + ".json"), state)
            raise
        if not isinstance(task.get("id"), str):
            raise Failure("queue_reply_missing_id; outcome_unknown")
        state.pop("queue_unknown", None)
        state["pending"] = task["id"]
        state.setdefault("queued", []).append(key)
        state["last_queued_at"] = now
        state["last_turns"] = snap["turns"]
        save(self.state / (sid + ".json"), state)
        return {"queued": True, "task": task["id"]}

    def cadence(self, state: dict) -> dict:
        """The `scribe.cadence` in force: Office status, else the cache.

        `office.status` returns the effective block even for an
        uncustomized Library; a hook that cannot read it keeps the last
        cadence it saw, else the shipped defaults. The same read caches
        the tags whose `trigger` is `marker` (`v2_marker_tags`); without a
        read there are none, so every tag waits for the cadence.
        """
        try:
            block = self.call("office.status", {}).get("scribe") or {}
            value = block.get("cadence") or {}
            minutes, turns = value.get("minutes"), value.get("turns")
            if (isinstance(minutes, int) and 5 <= minutes <= 240
                    and isinstance(turns, int) and 2 <= turns <= 100):
                state["v2_cadence"] = {"minutes": minutes, "turns": turns}
            tags = block.get("tags")
            if isinstance(tags, list):
                state["v2_marker_tags"] = sorted(
                    tag["name"] for tag in tags if isinstance(tag, dict)
                    and isinstance(tag.get("name"), str) and tag.get("trigger") == "marker")
        except Failure:
            pass
        cached = state.get("v2_cadence")
        return dict(cached) if isinstance(cached, dict) else dict(DEFAULT_CADENCE)

    def count_turns(self, context: dict) -> int:
        """User turns in the live transcript, read as `snapshot` pins it."""
        return self.read_turns(context)[0]

    def read_turns(self, context: dict) -> tuple[int, str]:
        """User turns in the live transcript and the last one's typed text."""
        root = (Path.home() / ".claude/projects").resolve(strict=True)
        source = Path(context.get("transcript_path", "")).resolve(strict=True)
        if root not in source.parents or source.suffix != ".jsonl":
            raise Failure("transcript_outside_provider_root")
        data = read_file(source, MAX_TRANSCRIPT)
        data = data[:data.rfind(b"\n") + 1]
        count, last = 0, ""
        for line in data.splitlines():
            if not line.strip():
                continue
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if isinstance(record, dict) and user_turn(record):
                count += 1
                last = user_text(record)
        return count, last

    def pass_command(self, sid: str) -> list[str]:
        """The headless scribe run a due Stop hook starts.

        `SCRIBE_PASS_COMMAND` (a JSON argv list, `{session_id}` replaced)
        overrides it for tests and other hosts; the default is a headless
        Claude run whose only job is this session's `/scribe update`.
        """
        raw = os.environ.get("SCRIBE_PASS_COMMAND", "")
        if raw:
            argv = json.loads(raw)
            if isinstance(argv, list) and argv and all(isinstance(a, str) for a in argv):
                return [a.replace("{session_id}", sid) for a in argv]
            raise Failure("invalid_pass_command")
        prompt = ("Run one scribe v2 pass. Call the office scribe tool with "
                  + json.dumps({"session_id": sid, "args": "update"})
                  + " and follow the agent request in its reply exactly. "
                  "Do nothing else.")
        return ["claude", "-p", prompt]

    def start_v2_pass(self, sid: str, state: dict, turns: int) -> dict:
        """Start one headless pass under the per-session pass lock.

        The child inherits the locked descriptor, so the lock is held
        while it runs and a second hook finds the pass already running.
        The hook never waits for the child (specs/scribe.md, "Cadence").
        """
        fd = os.open(self.state / (sid + ".pass.lock"),
                     os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
        try:
            if not stat.S_ISREG(os.fstat(fd).st_mode) or os.fstat(fd).st_uid != os.getuid():
                raise Failure("unsafe_state_lock")
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError:
                return {"started": False, "reason": "pass_running"}
            log = os.open(self.state / (sid + ".pass.log"),
                          os.O_WRONLY | os.O_CREAT | os.O_APPEND | os.O_NOFOLLOW, 0o600)
            try:
                child = subprocess.Popen(
                    self.pass_command(sid), cwd=str(self.workspace),
                    stdin=subprocess.DEVNULL, stdout=log, stderr=log,
                    start_new_session=True, pass_fds=(fd,))
            except OSError:
                return {"started": False, "reason": "pass_spawn_failed"}
            finally:
                os.close(log)
            state["v2_last_pass_at"] = time.time()
            state["v2_last_pass_turns"] = turns
            save(self.state / (sid + ".json"), state)
            return {"started": True, "pid": child.pid}
        finally:
            os.close(fd)

    def v2_hook(self, event: str, context: dict) -> dict:
        """Scribe v2 Stop/SessionEnd: a pass only for an on session.

        Every session defaults off: without Office's record saying on,
        nothing is staged, started or written. A due Stop (the cadence's
        minutes or user turns since the last pass started, a pending
        Capture-now, or a new last user turn that marks a tag whose
        `trigger` is `marker`) and every SessionEnd start at most one
        headless pass and return at once. There is no timer.
        """
        sid = self.session_id(context.get("session_id"))
        try:
            if self.excluded_project(context.get("cwd")):
                return {"excluded": True}
            # A session this plugin has never seen cannot be on (turning
            # on writes state here first), so its Stop stays free: no
            # Office call, no state. SessionStart records the context
            # before any Stop of a live session.
            seen = any(os.path.lexists(self.state / name)
                       for name in (sid, sid + ".json", sid + ".context.json"))
            if not seen:
                return {"on": False}
            with self.locked(sid):
                state = load(self.state / (sid + ".json"))
                shared = self.shared_state(sid)
                save(self.state / (sid + ".context.json"),
                     {k: context[k] for k in ("session_id", "cwd", "transcript_path") if k in context})
                if shared is None or shared.get("on") is not True:
                    save(self.state / (sid + ".json"), state)
                    return {"on": False}
                self.mirror(sid, True)
                turns, last_turn = self.read_turns(context)
                cadence = self.cadence(state)
                now_requested = shared.get("now_requested") is True
                last_at = state.get("v2_last_pass_at")
                last_turns = state.get("v2_last_pass_turns", 0)
                # Only a turn after the last pass started can mark one due,
                # so a marked turn starts one pass, not one per Stop.
                marked = sorted(marked_tags(last_turn, set(state.get("v2_marker_tags") or ()))
                                ) if turns > last_turns else []
                due = (event == "SessionEnd" or now_requested or last_at is None
                       or time.time() - last_at >= cadence["minutes"] * 60
                       or turns - last_turns >= cadence["turns"] or bool(marked))
                if not due:
                    save(self.state / (sid + ".json"), state)
                    return {"on": True, "started": False, "reason": "cadence_not_reached",
                            "cadence": cadence}
                result = self.start_v2_pass(sid, state, turns)
                if marked:
                    result["marked"] = marked
                if result.get("started") and now_requested:
                    with contextlib.suppress(Failure):
                        self.control(sid, "consume_now",
                                     requested_at=shared.get("now_requested_at"))
                return {"on": True, "cadence": cadence, **result}
        except Failure as error:
            # Provider hooks fail open; the fixed code names the refusal.
            return {"started": False, "reason": str(error).split(";", 1)[0]}

    def hook(self, event: str, context: dict) -> dict:
        sid = self.session_id(context.get("session_id"))
        if event in ("Stop", "SessionEnd") and not self.auto_capture:
            # Scribe v2 (specs/scribe.md): opt-in per session, Stop-hook
            # cadence, no timer. The legacy queue below runs only with
            # the legacy auto_capture option on.
            return self.v2_hook(event, context)
        if self.excluded_project(context.get("cwd")):
            return {"excluded": True}
        with self.locked(sid):
            state = load(self.state / (sid + ".json"))
            # The end-of-turn hook reads Office's shared record once per turn
            # (the window and /scribe write it); other events keep the cache.
            self.shared = self.shared_state(sid) if event == "Stop" else None
            on = self.switch(sid, state, self.shared)
            # Persist the real hook context for structured /scribe calls (never shell args).
            save(self.state / (sid + ".context.json"), {k: context[k] for k in ("session_id", "cwd", "transcript_path") if k in context})
            save(self.state / (sid + ".json"), state)
            if event == "SessionStart":
                return {"brief": self.call("office.session-brief", session_start_brief(context.get("cwd")))}
            if self.shared is not None and self.shared.get("on") is None:
                # First turn Office sees: publish this host's switch (the
                # plugin default) so the window shows the same state.
                with contextlib.suppress(Failure):
                    self.control(sid, "set_switch", on=on)
            now_requested = self.shared is not None and self.shared.get("now_requested") is True
            if event == "Stop" and not on and not now_requested:
                return {"on": False}
            snap = self.snapshot(context, "final" if event == "SessionEnd" else "live")
            if not snap:
                return {"queued": False}
            if event == "Stop" and now_requested:
                result = self.capture_now(sid, state, snap, self.shared.get("now_requested_at"))
            else:
                result = self.queue(sid, state, snap, force=event == "SessionEnd")
            if event == "Stop" and on:
                self.apply_shared_title(sid, state, context, snap)
            return result

    def capture_now(self, sid: str, state: dict, snap: dict, requested_at) -> dict:
        """Run a pending Capture-now request as a forced pass, then consume it.

        Consumption names the request read this turn, so a newer request made
        meanwhile survives. While an earlier pass is still pending the request
        stays pending and runs at a later turn. A consumption that fails after
        queuing is retried next turn and deduplicates on the same snapshot.
        """
        result = self.queue(sid, state, snap, force=True)
        if result.get("queued") or result.get("reason") == "already_queued":
            consumed = self.control(sid, "consume_now", requested_at=requested_at)
            result["capture_now"] = "consumed" if consumed.get("consumed") else "already_consumed"
        else:
            result["capture_now"] = "pending"
        return result

    def apply_shared_title(self, sid: str, state: dict, context: dict, snap: dict) -> None:
        """Apply a title set in another host (the window) once, as a user title."""
        title = (self.shared or {}).get("title")
        if not isinstance(title, str) or not title.strip() or title == state.get("applied_title"):
            return
        with contextlib.suppress(Failure):
            self.call("office.capture", {"session_transcript_path": snap["path"], "session_id": sid,
                "project": context["cwd"], "title": title, "title_by": "user"})
            state["applied_title"] = title
            save(self.state / (sid + ".json"), state)

    def handoff(self, args: dict) -> dict:
        """`/handoff [<project>]`: the merged project + global handoff, or the projects."""
        if set(args) - {"project"}:
            raise Failure("unknown_command_field")
        raw = args.get("project", "")
        if not isinstance(raw, str):
            raise Failure("project_must_be_text")
        project = raw.strip() or None
        if project is not None and (len(project) > 256 or any(c.isspace() for c in project)
                                    or not project.isprintable()):
            raise Failure("project_must_be_one_name")
        self.timeout = HANDOFF_CALL
        request = {"view": "handoffs"}
        if project is not None:
            request.update(project=project, orchestrator=ORCHESTRATOR)
        brief = self.call("office.session-brief", request, cap=HANDOFF_CALL)
        return {"display": render_handoff(project, brief), "brief": brief}

    def command(self, args: dict) -> dict:
        if set(args) - {"session_id", "action", "title", "since"}:
            raise Failure("unknown_command_field")
        action = args.get("action")
        if action not in {"on", "off", "now", "status", "title", "backfill"}:
            raise Failure("unknown_scribe_action")
        sid = self.session_id(args.get("session_id"))
        if ("title" in args and action != "title") or ("since" in args and action != "backfill"):
            raise Failure("field_not_valid_for_action")
        if action == "status":
            # Observational even before SessionStart; do not initialize a switch.
            if (self.workspace / ".orbit").is_symlink():
                raise Failure("unsafe_state_directory")
            for path, private in ((self.workspace / ".orbit/tmp", False), (self.state, True)):
                if os.path.lexists(path):  # a dangling symlink is refused too
                    check_dir(path, private)
            state = load(self.state / (sid + ".json"))
            flag = self.state / sid
            if flag.is_symlink():
                raise Failure("unsafe_switch_file")
            queued = state.get("last_queued_at")
            on = flag.exists() if state.get("initialized") else self.default_on
            # Office's shared record is authoritative when it answers; status
            # still reads (and writes nothing locally) when it cannot.
            shared = self.shared_state(sid)
            if shared is not None and isinstance(shared.get("on"), bool):
                on = shared["on"]
            return {"on": on, "last_queued_task": state.get("pending"), "task_status": "not_checked",
                    "queue_outcome_unknown": state.get("queue_unknown", False),
                    "last_queued_at": dt.datetime.fromtimestamp(queued, dt.timezone.utc).isoformat() if queued else None,
                    "shared_state": shared is not None,
                    "on_by": shared.get("on_by") if shared else None,
                    "scope": shared.get("scope") if shared else None,
                    "now_requested": bool(shared and shared.get("now_requested")),
                    "window_open": self.window_open(sid, shared)}
        if action == "backfill":
            return self.backfill(args.get("since"))
        with self.locked(sid):
            context = load(self.state / (sid + ".context.json"))
            if self.excluded_project(context.get("cwd")):
                return {"excluded": True}
            state = load(self.state / (sid + ".json"))
            on = self.switch(sid, state)
            if action in ("on", "off"):
                # Office first: it is the shared switch; the flag file mirrors it.
                # The pane is the user's own host, so its turn-on is the user's.
                if action == "on":
                    self.control(sid, "set_switch", on=True, on_by="user")
                else:
                    self.control(sid, "set_switch", on=False)
                self.mirror(sid, action == "on")
                save(self.state / (sid + ".json"), state)
                return {"on": action == "on"}
            if action == "title" and (not isinstance(args.get("title"), str) or not args["title"].strip()):
                raise Failure("title_required")
            if action == "title":
                self.control(sid, "set_title", title=args["title"])
            else:
                requested = self.control(sid, "request_now")
            snap = self.snapshot(context, "live")
            if not snap:
                return {"queued": False, "reason": "no_eligible_records"}
            if action == "title":
                result = self.call("office.capture", {"session_transcript_path": snap["path"], "session_id": sid,
                    "project": context["cwd"], "title": args["title"], "title_by": "user"})
                state["applied_title"] = args["title"].strip()
                save(self.state / (sid + ".json"), state)
                return result
            return self.capture_now(sid, state, snap, requested.get("now_requested_at"))

    def backfill(self, since: str | None) -> dict:
        after = dt.date.fromisoformat(since) if since is not None else None
        root = Path.home() / ".claude/projects"
        self.setup()
        count = 0
        refused = 0
        # Dates come from the transcript, not mutable filesystem mtimes.
        candidates = []
        for path in root.glob("*/*.jsonl"):
            if time.monotonic() - self.started >= self.timeout:
                raise Failure("backfill_deadline; rerun_to_continue")
            try:
                raw = read_file(path, MAX_TRANSCRIPT)
                records = [json.loads(line) for line in raw.splitlines() if line.strip()]
                first = next((r for r in records if r.get("type") in ("user", "assistant")), None)
                if not first or not first.get("cwd") or self.excluded_project(first["cwd"]):
                    continue
                started = dt.datetime.fromisoformat(first["timestamp"].replace("Z", "+00:00"))
                if started.tzinfo is None:
                    raise Failure("timestamp_needs_offset")
                day = started.astimezone(dt.timezone.utc).date()
                if after and day < after:
                    continue
                candidates.append((started, path, first))
            except (Failure, OSError, ValueError, TypeError, KeyError, AttributeError):
                refused += 1
        for _, path, first in sorted(candidates, key=lambda v: v[0]):
            sid = self.session_id(first["sessionId"])
            context = {"session_id": sid, "cwd": first["cwd"], "transcript_path": str(path)}
            with self.locked(sid):
                state = load(self.state / (sid + ".json"))
                snap = self.snapshot(context, "final")
                if snap:
                    count += int(self.queue(sid, state, snap, force=True)["queued"])
        return {"queued_count": count, "refused_count": refused}


def _cancel(_signum, _frame):
    raise Failure("hook_cancelled; mutation outcome may be unknown")


def main() -> int:
    signal.signal(signal.SIGTERM, _cancel)
    signal.signal(signal.SIGINT, _cancel)
    hook = len(sys.argv) == 3 and sys.argv[1] == "hook"
    try:
        raw = sys.stdin.buffer.read(65537)
        if len(raw) > 65536:
            raise Failure("hook_input_too_large")
        payload = json.loads(raw)
        if not isinstance(payload, dict):
            raise Failure("hook_input_must_be_object")
        client = Scribe()
        result = client.hook(sys.argv[2], payload) if hook else client.command(payload)
        if hook and sys.argv[2] == "SessionStart" and "brief" in result:
            print(json.dumps({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": json.dumps(result["brief"])}}))
        elif not hook:
            print(json.dumps(result))
        return 0
    except Exception as error:  # provider hooks must fail open on every ordinary failure
        # Exception types/fixed Failure codes only: never paths, transcript text or Orbit stderr.
        message = str(error) if isinstance(error, Failure) else type(error).__name__
        print("office-scribe: " + message, file=sys.stderr)
        return 0 if hook else 1


if __name__ == "__main__":
    raise SystemExit(main())
