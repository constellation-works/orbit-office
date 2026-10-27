"""The append-only audit: `{{plugin_state}}/audit/<yyyy-mm>.jsonl`, one line per call.

A record holds who called (from the call context), what was asked (tool,
connection, method, path, redacted query) and what happened (status, bytes,
duration, the SHA-256 of the redacted body, outcome). A drafted write's
records also name the draft. Bodies are never written here.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from . import state
from .envelope import Call, closed, typed
from .redact import Redactor

MONTH_FILE = re.compile(r"^\d{4}-\d{2}\.jsonl$")
DEFAULT_LIMIT = 50
MAX_LIMIT = 500
RECORD_KEYS = {
    "time", "caller", "tool", "connection", "method", "path", "query",
    "status", "bytes", "duration_ms", "body_sha256", "redirects", "outcome",
}
# Present only on the records of a drafted write (http-draft, http-send).
OPTIONAL_KEYS = {"draft"}


@dataclass
class Record:
    """One call, filled in as it runs; written once at the end."""

    tool: str
    caller: dict[str, Any]
    connection: str | None = None
    method: str | None = None
    path: str | None = None
    query: dict[str, Any] = field(default_factory=dict)
    status: int | None = None
    bytes: int = 0
    duration_ms: int = 0
    body_sha256: str | None = None
    redirects: int = 0
    outcome: str = "ok"
    draft: str | None = None

    def to_json(self, now: datetime) -> dict[str, Any]:
        out = {
            "time": now.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z",
            "caller": self.caller,
            "tool": self.tool,
            "connection": self.connection,
            "method": self.method,
            "path": self.path,
            "query": self.query,
            "status": self.status,
            "bytes": self.bytes,
            "duration_ms": self.duration_ms,
            "body_sha256": self.body_sha256,
            "redirects": self.redirects,
            "outcome": self.outcome,
        }
        if self.draft is not None:
            out["draft"] = self.draft
        return out


def _dir(root: Path) -> Path:
    return root / "audit"


def append(root: Path, record: Record, redactor: Redactor, now: datetime | None = None) -> None:
    """Append one record to this month's file."""
    now = now or datetime.now(timezone.utc)
    path = _dir(root) / f"{now.astimezone(timezone.utc):%Y-%m}.jsonl"
    line = json.dumps(redactor.value(record.to_json(now)), ensure_ascii=False, sort_keys=True)
    state.append_line(path, line, redactor)


def tool_list(call: Call) -> dict[str, Any]:
    """Audit records, newest first."""
    raw = closed(call.input, {"limit", "connection"}, "input")
    limit = typed(raw, "limit", int, "input", DEFAULT_LIMIT)
    limit = max(1, min(limit, MAX_LIMIT))
    connection = typed(raw, "connection", str, "input", None)
    directory = _dir(call.state_root())
    records: list[dict[str, Any]] = []
    skipped = 0
    if state.dir_exists(directory):
        months = sorted((path for path in directory.iterdir() if MONTH_FILE.match(path.name)), reverse=True)
        for month in months:
            text = state.read_text(month) or ""
            for line in reversed(text.splitlines()):
                if not line.strip():
                    continue
                try:
                    record = json.loads(line)
                except ValueError:
                    skipped += 1
                    continue
                if not isinstance(record, dict) or not RECORD_KEYS <= set(record) <= RECORD_KEYS | OPTIONAL_KEYS:
                    skipped += 1
                    continue
                if connection is not None and record.get("connection") != connection:
                    continue
                records.append(record)
                if len(records) >= limit:
                    break
            if len(records) >= limit:
                break
    # Lines are appended in time order, so reading each month backwards is
    # newest first; sort by time as well, in case clocks or months overlap.
    records.sort(key=lambda record: str(record.get("time", "")), reverse=True)
    return {"records": records, "skipped": skipped}
