"""The Orbit `exec` envelope (host_api 1): one request on stdin, one answer on stdout.

    {"schema_version": 1, "tool": "<ns>.<verb>", "input": {...},
     "context": {"workspace_root": ..., "agent": ..., "model": ..., "task_id": ...,
                 "job_run_id": ..., "secrets": {"<name>": {"value": ..., "version": ...}}}}

The answer is `{"ok": true, "output": {...}}` or
`{"ok": false, "error": {"code", "message", "retryable", "detail"?}}`, always
redacted first. Identity and secrets come from `context`, never from `input`.
"""

from __future__ import annotations

import json
import os
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, TextIO

from .redact import Redactor

SCHEMA_VERSION = 1


class GatewayError(Exception):
    """A structured tool error: the code is a contract, the message is for people."""

    def __init__(self, code: str, message: str, *, retryable: bool = False, detail: dict | None = None):
        super().__init__(message)
        self.code = code
        self.message = message
        self.retryable = retryable
        self.detail = detail

    def to_json(self) -> dict[str, Any]:
        error: dict[str, Any] = {"code": self.code, "message": self.message, "retryable": self.retryable}
        if self.detail is not None:
            error["detail"] = self.detail
        return error


@dataclass
class Call:
    """One tool call: its verb, input and the host-attested call context."""

    tool: str
    input: dict[str, Any]
    context: dict[str, Any]
    environ: Mapping[str, str] = field(default_factory=dict)

    @property
    def verb(self) -> str:
        return self.tool.split(".", 1)[1] if "." in self.tool else self.tool

    def secrets(self) -> dict[str, str]:
        """Slot name -> value for every declared secret that is set."""
        raw = self.context.get("secrets")
        out: dict[str, str] = {}
        if isinstance(raw, dict):
            for name, entry in raw.items():
                value = entry.get("value") if isinstance(entry, dict) else entry
                if isinstance(name, str) and isinstance(value, str) and value:
                    out[name] = value
        return out

    def caller(self) -> dict[str, Any]:
        """Who is calling, from the call context (never from input)."""

        def text(*keys: str) -> str | None:
            for key in keys:
                value = self.context.get(key)
                if isinstance(value, str) and value:
                    return value
            return None

        return {
            "agent": text("agent", "agent_kind"),
            "model": text("model"),
            "task_id": text("task_id"),
            "job_run_id": text("job_run_id"),
            "session_id": text("session_id") or self.environ.get("ORBIT_SESSION_ID") or None,
        }

    def state_root(self) -> Path:
        """`{{plugin_state}}`, which Orbit names in ORBIT_PLUGIN_STATE."""
        root = self.environ.get("ORBIT_PLUGIN_STATE") or self.context.get("plugin_state")
        if not isinstance(root, str) or not root or not os.path.isabs(root):
            raise GatewayError(
                "state_unavailable",
                "ORBIT_PLUGIN_STATE is not set; run this tool through Orbit (`orbit tool run`).",
            )
        return Path(root)


def parse(raw: str, environ: Mapping[str, str]) -> Call:
    try:
        request = json.loads(raw)
    except json.JSONDecodeError as error:
        raise GatewayError("invalid_request", f"the request is not JSON: {error.msg}") from None
    if not isinstance(request, dict):
        raise GatewayError("invalid_request", "the request must be a JSON object")
    if request.get("schema_version") != SCHEMA_VERSION:
        raise GatewayError("invalid_request", f"unsupported schema_version; this backend speaks {SCHEMA_VERSION}")
    tool = request.get("tool")
    payload = request.get("input") if request.get("input") is not None else {}
    context = request.get("context") if request.get("context") is not None else {}
    if not isinstance(tool, str) or not isinstance(payload, dict) or not isinstance(context, dict):
        raise GatewayError("invalid_request", "the request needs a string `tool` and object `input` and `context`")
    return Call(tool=tool, input=payload, context=context, environ=environ)


def emit(stream: TextIO, answer: dict[str, Any], redactor: Redactor) -> None:
    """Write one answer, redacted. The only writer of results and errors."""
    stream.write(json.dumps(redactor.value(answer), ensure_ascii=False, sort_keys=True))
    stream.write("\n")
    stream.flush()


def serve(
    raw: str,
    environ: Mapping[str, str],
    handlers: Mapping[str, Callable[[Call], dict[str, Any]]],
    stream: TextIO,
) -> None:
    """Parse one request, run its handler and emit the redacted answer."""
    redactor = Redactor()
    try:
        call = parse(raw, environ)
        redactor = redactor_for(call)
        handler = handlers.get(call.verb)
        if handler is None:
            raise GatewayError("unknown_tool", f"this backend serves {sorted(handlers)}, not '{call.verb}'")
        answer: dict[str, Any] = {"ok": True, "output": handler(call)}
    except GatewayError as error:
        answer = {"ok": False, "error": error.to_json()}
    except Exception as error:  # noqa: BLE001 - reported as a tool error, never a traceback
        answer = {"ok": False, "error": {"code": "backend_error", "message": f"{type(error).__name__}: {error}", "retryable": False}}
    emit(stream, answer, redactor)


def redactor_for(call: Call) -> Redactor:
    """The redactor for one call: every delivered slot value, and basic-auth pairs."""
    # Imported here: connections reads state, and the envelope stays usable without it.
    from .connections import basic_pairs

    secrets = call.secrets()
    try:
        pairs = basic_pairs(call, secrets)
    except GatewayError:
        pairs = []
    return Redactor(secrets.values(), pairs)


# ---------------------------------------------------------------------------
# Closed input checks. Orbit validates the schema first; the backend checks
# again, so an unknown key is refused even on a direct call.
# ---------------------------------------------------------------------------


def closed(obj: Any, allowed: set[str], where: str) -> dict[str, Any]:
    if not isinstance(obj, dict):
        raise GatewayError("invalid_argument", f"`{where}` must be an object")
    unknown = sorted(set(obj) - allowed)
    if unknown:
        raise GatewayError("invalid_argument", f"`{where}` does not accept {', '.join(map(repr, unknown))}")
    return obj


def required(obj: dict[str, Any], key: str, kind: type, where: str) -> Any:
    if key not in obj:
        raise GatewayError("invalid_argument", f"`{where}.{key}` is required")
    return typed(obj, key, kind, where, None)


def typed(obj: dict[str, Any], key: str, kind: type, where: str, default: Any) -> Any:
    if key not in obj or obj[key] is None:
        return default
    value = obj[key]
    if kind is int and (isinstance(value, bool) or not isinstance(value, int)):
        raise GatewayError("invalid_argument", f"`{where}.{key}` must be an integer")
    if not isinstance(value, kind):
        raise GatewayError("invalid_argument", f"`{where}.{key}` must be a {kind.__name__}")
    return value
