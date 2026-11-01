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
import sys
import time
import uuid

from process import Failure, NotSent, run

MAX_TRANSCRIPT = 8 * 1024 * 1024
SESSION = re.compile(r"[A-Za-z0-9_-]{1,128}\Z")
TERMINAL = {"done", "rejected", "archived", "cancelled", "canceled"}
TOOLS = {"office.session-brief", "office.capture", "office.recall", "orbit.task.add", "orbit.task.show"}


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


def private_dir(path: Path) -> Path:
    """Create/validate without following a symlink in any state component."""
    if not path.parent.exists():
        private_dir(path.parent)
    try:
        path.mkdir(mode=0o700)
    except FileExistsError:
        pass
    info = path.lstat()
    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
        raise Failure("unsafe_state_directory")
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
        self.timeout = max(0.05, min(2.0, float(os.environ.get("CLAUDE_PLUGIN_OPTION_TIMEOUT", "1"))))
        self.started = time.monotonic()
        # Do not create local state for recall-only calls.
        self.state = self.workspace / ".orbit/tmp/scribe"
        self.staging = self.workspace / ".orbit/tmp/session-transcripts"

    def setup(self):
        # Workspace/.orbit may already be public metadata; tmp/state are private.
        orbit = self.workspace / ".orbit"
        if orbit.is_symlink():
            raise Failure("unsafe_state_directory")
        orbit.mkdir(mode=0o700, exist_ok=True)
        private_dir(self.workspace / ".orbit/tmp")
        private_dir(self.state)
        private_dir(self.staging)

    def call(self, tool: str, payload: dict) -> dict:
        if tool not in TOOLS:
            raise Failure("tool_not_allowed")
        remaining = self.timeout - (time.monotonic() - self.started)
        if remaining <= 0:
            raise NotSent("hook_deadline; request_not_sent")
        return run(["orbit", "tool", "run", tool, "--json", "--full", "--workspace", str(self.workspace),
                    "--input", json.dumps(payload)], str(self.workspace), min(2.0, remaining))

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

    def window_open(self, sid: str) -> bool:
        """True while a Scribe window heartbeat for this session is fresh."""
        try:
            heartbeat = self.state / (sid + ".window-open")
            if heartbeat.is_symlink():
                return False
            return time.time() - heartbeat.stat().st_mtime < 120
        except OSError:
            return False

    def heartbeat(self, sid: str) -> dict:
        """Refresh the window-open heartbeat the Stop hook reads."""
        self.session_id(sid)
        self.setup()
        publish(self.state / (sid + ".window-open"), b"open\n")
        return {"window_open": True}

    def switch(self, sid: str, state: dict) -> bool:
        flag = self.state / sid
        if not state.get("initialized"):
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
            turns_needed = 2 if self.window_open(sid) else 8
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

    def hook(self, event: str, context: dict) -> dict:
        sid = self.session_id(context.get("session_id"))
        if self.excluded_project(context.get("cwd")):
            return {"excluded": True}
        with self.locked(sid):
            state = load(self.state / (sid + ".json"))
            on = self.switch(sid, state)
            # Persist the real hook context for structured /scribe calls (never shell args).
            save(self.state / (sid + ".context.json"), {k: context[k] for k in ("session_id", "cwd", "transcript_path") if k in context})
            save(self.state / (sid + ".json"), state)
            if event == "SessionStart":
                return {"brief": self.call("office.session-brief", {})}
            if event == "Stop" and not on:
                return {"on": False}
            snap = self.snapshot(context, "final" if event == "SessionEnd" else "live")
            return self.queue(sid, state, snap, force=event == "SessionEnd") if snap else {"queued": False}

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
            for path in (self.workspace / ".orbit", self.workspace / ".orbit/tmp", self.state):
                if path.is_symlink():
                    raise Failure("unsafe_state_directory")
                if path.exists() and path != self.workspace / ".orbit":
                    info = path.stat()
                    if not stat.S_ISDIR(info.st_mode) or info.st_uid != os.getuid() or info.st_mode & 0o077:
                        raise Failure("unsafe_state_directory")
            state = load(self.state / (sid + ".json"))
            flag = self.state / sid
            if flag.is_symlink():
                raise Failure("unsafe_switch_file")
            queued = state.get("last_queued_at")
            return {"on": flag.exists() if state.get("initialized") else self.default_on,
                    "last_queued_task": state.get("pending"), "task_status": "not_checked",
                    "queue_outcome_unknown": state.get("queue_unknown", False),
                    "last_queued_at": dt.datetime.fromtimestamp(queued, dt.timezone.utc).isoformat() if queued else None}
        if action == "backfill":
            return self.backfill(args.get("since"))
        with self.locked(sid):
            context = load(self.state / (sid + ".context.json"))
            if self.excluded_project(context.get("cwd")):
                return {"excluded": True}
            state = load(self.state / (sid + ".json"))
            on = self.switch(sid, state)
            if action in ("on", "off"):
                if action == "on":
                    publish(self.state / sid, b"on\n")
                elif (self.state / sid).exists():
                    (self.state / sid).unlink()  # owned session switch, not captured evidence
                save(self.state / (sid + ".json"), state)
                return {"on": action == "on"}
            if action == "title" and (not isinstance(args.get("title"), str) or not args["title"].strip()):
                raise Failure("title_required")
            snap = self.snapshot(context, "live")
            if not snap:
                return {"queued": False, "reason": "no_eligible_records"}
            if action == "title":
                return self.call("office.capture", {"session_transcript_path": snap["path"], "session_id": sid,
                    "project": context["cwd"], "title": args["title"], "title_by": "user"})
            return self.queue(sid, state, snap, force=True)

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
