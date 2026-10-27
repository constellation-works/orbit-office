"""`gateway.http-draft` and `gateway.http-send`: a write drafted by an agent,
approved by the user and sent at most once (docs/design/office/specs/gateway.md,
"Writes: draft, approve, send").

- **Draft.** A method the connection lists in `drafts`, the same path, query
  and header rules as `http-get` (plus `Content-Type`, `If-Match` and
  `If-Unmodified-Since`) and a `body`. A request holding a slot value or a
  credential is refused (`secret_in_body`, `secret_in_request`), so a draft
  never holds a secret. The draft is stored in `drafts/<id>.json` with the
  SHA-256 of its exact request (method, URL, headers and body), and one Orbit
  task tagged `gateway-draft` shows it in plain words, with the request as
  its `gateway-draft.json` artifact.
- **Send.** Takes the draft id and the digest the user saw. A stored request
  that no longer matches is refused (`draft_changed`; STD-05@1 §R21), and so
  is a draft whose task was dismissed (`draft_dismissed`). The draft is
  marked `sending` before anything goes out, and a draft that is not
  `pending` is never sent (`draft_not_pending`). A failure to connect sends
  nothing and leaves the draft `pending`. Once the request has been written,
  a timeout or a lost reply is `outcome_unknown`: recorded, audited and
  returned as such, never as a failure, and never retried (STD-03@2 §R31).
  A reply that does arrive, but whose body is cut off, is `sent` with
  `truncated` true: the write went out once, and the body is not reported
  as complete. The connection's idempotency header, when it names one,
  carries the draft id. Redirects are returned, never followed. An
  `oauth2_device` connection's access token is exchanged before the draft
  is marked `sending`, so a refused sign-in (`auth_expired`) sends nothing. A
  successful send then closes the approval task as `done` by the legal
  lifecycle steps: `done` is accepted only from `review`, and that step
  carries an execution summary.

With `http.py` and `oauth.py`, the only modules that open network connections.
"""

from __future__ import annotations

import hashlib
import http.client
import json
import os
import re
import secrets
import socket
import ssl
import time
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from . import audit, callbacks, connections, state
from .connections import DRAFT_HEADERS, Connection
from .envelope import Call, GatewayError, closed, redactor_for, required, typed
from .http import USER_AGENT, agent_headers, credentials, origin, read_limited, resolve, returned_headers, shape_body
from .redact import Redactor

FILE_VERSION = 1
DRAFT_ID = re.compile(r"^gd-[0-9a-f]{16}$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")
DRAFT_KEYS = {"connection", "path", "query", "headers", "method", "body"}
SEND_KEYS = {"draft", "sha256"}
MAX_BODY_BYTES = 256 * 1024
# How much of the body the task description shows; the artifact has all of it.
SHOWN_BODY_CHARS = 4000
TAGS = ("gateway-draft", "proposal", "proposal:gateway_draft")
ARTIFACT = "gateway-draft.json"
# Where the artifact is staged for `orbit.task.artifact.put`, which takes only
# a file inside the workspace. Removed once the put returns.
STAGING = Path(".orbit-gateway") / "outbox"
JSON_TYPE = "application/json"
TEXT_TYPE = "text/plain; charset=utf-8"

# Draft states. Only `pending` may be sent.
PENDING = "pending"
FILING = "filing"  # stored, its task not filed yet
UNFILED = "unfiled"  # filing the task failed; never sendable
SENDING = "sending"
SENT = "sent"
OUTCOME_UNKNOWN = "outcome_unknown"
DISMISSED = "dismissed"


# ---------------------------------------------------------------------------
# Storage
# ---------------------------------------------------------------------------


def _path(root: Path, draft_id: str) -> Path:
    return root / "drafts" / f"{draft_id}.json"


def _lock(root: Path, draft_id: str) -> Any:
    return state.locked(root / "drafts" / f"{draft_id}.lock")


def _load(root: Path, draft_id: str) -> dict[str, Any]:
    text = state.read_text(_path(root, draft_id))
    if text is None:
        raise GatewayError("unknown_draft", f"no draft {draft_id!r}")
    try:
        document = json.loads(text)
        request = document["request"]
        if not (
            isinstance(document, dict)
            and isinstance(document["state"], str)
            and isinstance(document["sha256"], str)
            and isinstance(request["method"], str)
            and isinstance(request["url"], str)
            and isinstance(request["headers"], dict)
        ):
            raise TypeError("unexpected field types")
    except (ValueError, KeyError, TypeError) as error:
        raise GatewayError("state_corrupt", f"draft {draft_id} is not readable: {error}") from None
    return document


def _save(root: Path, document: dict[str, Any], redactor: Redactor) -> None:
    text = json.dumps(document, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
    state.store_text(_path(root, document["id"]), text, redactor)


def _now() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def digest(request: dict[str, Any]) -> str:
    """SHA-256 of the exact request: method, URL, headers and body."""
    exact = {key: request[key] for key in ("method", "url", "headers", "body")}
    canonical = json.dumps(exact, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _refuse_secrets(request: dict[str, Any], body_value: Any, redactor: Redactor) -> None:
    """Refuse a request the redactor would change: it holds a secret."""
    body = request["body"]
    if body is not None and (redactor.text(body) != body or redactor.value(body_value) != body_value):
        raise GatewayError(
            "secret_in_body",
            "the body contains a connection's secret or a credential; a draft never holds one. "
            "The gateway adds the connection's authentication itself when the request is sent",
        )
    if redactor.text(request["url"]) != request["url"] or redactor.value(request["headers"]) != request["headers"]:
        raise GatewayError(
            "secret_in_request",
            "the path, query or headers contain a connection's secret or a credential; a draft never holds one",
        )


def _workspace(call: Call) -> Path:
    raw = call.context.get("workspace_root") or call.environ.get("ORBIT_WORKSPACE_ROOT")
    if not isinstance(raw, str) or not raw or not os.path.isabs(raw):
        raise GatewayError(
            "workspace_unavailable",
            "a draft files an Orbit task, so it needs the calling workspace; run it through Orbit in a workspace",
        )
    return Path(raw)


# ---------------------------------------------------------------------------
# gateway.http-draft
# ---------------------------------------------------------------------------


def tool_draft(call: Call) -> dict[str, Any]:
    root = call.state_root()
    redactor = redactor_for(call)
    record = audit.Record(tool=call.tool, caller=call.caller())
    started = time.monotonic()
    try:
        output = _draft(call, root, record, redactor)
        record.outcome = "drafted"
        return output
    except GatewayError as error:
        record.outcome = error.code
        raise
    except Exception:
        record.outcome = "backend_error"
        raise
    finally:
        record.duration_ms = int((time.monotonic() - started) * 1000)
        audit.append(root, record, redactor)


def _body(raw: dict[str, Any]) -> tuple[str | None, Any, str | None]:
    """(exact body text, the input value, default content type)."""
    if raw.get("body") is None:
        return None, None, None
    value = raw["body"]
    if isinstance(value, str):
        return value, value, TEXT_TYPE
    try:
        return json.dumps(value, ensure_ascii=False, separators=(",", ":")), value, JSON_TYPE
    except (TypeError, ValueError) as error:
        raise GatewayError("invalid_argument", f"`input.body` must be a string or a JSON value: {error}") from None


def _draft(call: Call, root: Path, record: audit.Record, redactor: Redactor) -> dict[str, Any]:
    raw = closed(call.input, DRAFT_KEYS, "input")
    name = required(raw, "connection", str, "input")
    method = required(raw, "method", str, "input").upper()
    path = typed(raw, "path", str, "input", "")
    query = typed(raw, "query", dict, "input", {})
    headers = typed(raw, "headers", dict, "input", {})
    record.connection, record.method, record.path, record.query = name, method, path, query

    connection = connections.find(root, name)
    if method not in connection.drafts:
        allowed = ", ".join(connection.drafts) if connection.drafts else "no methods"
        raise GatewayError(
            "method_not_allowed",
            f"connection {name!r} may draft {allowed}; {method} is not one of them",
        )
    url = resolve(connection.base_url, path, query)
    record.path = urllib.parse.urlsplit(url).path
    request_headers = agent_headers(connection, headers, DRAFT_HEADERS)
    body, body_value, content_type = _body(raw)
    if body is not None:
        if len(body.encode("utf-8")) > MAX_BODY_BYTES:
            raise GatewayError("invalid_argument", f"the body is larger than {MAX_BODY_BYTES} bytes")
        if not any(key.lower() == "content-type" for key in request_headers):
            request_headers["Content-Type"] = str(content_type)
    request = {"method": method, "url": url, "headers": request_headers, "body": body}
    _refuse_secrets(request, body_value, redactor)
    workspace = _workspace(call)

    draft_id = "gd-" + secrets.token_hex(8)
    sha256 = digest(request)
    record.draft = draft_id
    document: dict[str, Any] = {
        "schema_version": FILE_VERSION,
        "id": draft_id,
        "state": FILING,
        "connection": connection.name,
        "request": request,
        "sha256": sha256,
        "task_id": None,
        "created_at": _now(),
        "caller": call.caller(),
        "send": None,
    }
    # Stored before the task is filed: a draft whose task was never recorded
    # is never sendable, and the request is kept for inspection (STD-03@2 §R30).
    with _lock(root, draft_id):
        _save(root, document, redactor)

    orbit = callbacks.Orbit(call.environ, workspace, redactor)
    try:
        task = orbit.call(
            "orbit.task.add",
            {
                "workspace": str(workspace),
                "title": f"Approve API write: {method} {connection.name} {record.path}",
                "description": describe(document, connection),
                "acceptance_criteria": [
                    "The user approves or dismisses this request; it is sent at most once, and only on approval."
                ],
                "tags": list(TAGS),
                "complexity": "low",
            },
        )
        task_id = task.get("id")
        if not isinstance(task_id, str) or not task_id:
            raise GatewayError(
                "orbit_outcome_unknown",
                "orbit.task.add answered without a task id; check Orbit for the task before drafting again",
            )
    except GatewayError:
        with _lock(root, draft_id):
            document["state"] = UNFILED
            _save(root, document, redactor)
        raise
    with _lock(root, draft_id):
        document["task_id"] = task_id
        document["state"] = PENDING
        _save(root, document, redactor)

    output: dict[str, Any] = {
        "draft": draft_id,
        "task_id": task_id,
        "state": PENDING,
        "sha256": sha256,
        "connection": connection.name,
        "request": request,
        "artifact": None,
    }
    try:
        _attach(orbit, workspace, document, redactor)
        output["artifact"] = ARTIFACT
    except GatewayError as error:
        # The task is filed and its description shows the request; a missing
        # artifact is reported, not a reason to draft (and file) again.
        output["artifact_error"] = error.code
    return output


def _attach(orbit: callbacks.Orbit, workspace: Path, document: dict[str, Any], redactor: Redactor) -> None:
    staged = workspace / STAGING / f"{document['id']}.json"
    artifact = {key: document[key] for key in ("schema_version", "id", "connection", "request", "sha256", "created_at")}
    try:
        state.store_text(staged, json.dumps(artifact, indent=2, sort_keys=True, ensure_ascii=False) + "\n", redactor)
    except OSError:
        raise GatewayError("artifact_staging_failed", "could not stage the draft artifact") from None
    try:
        orbit.call(
            "orbit.task.artifact.put",
            {"id": document["task_id"], "source_path": str(staged), "path": ARTIFACT},
        )
    finally:
        # This attempt created the file under the draft's own id.
        try:
            os.unlink(staged)
        except FileNotFoundError:
            pass


def _is_json(headers: dict[str, str]) -> bool:
    for key, value in headers.items():
        if key.lower() == "content-type":
            mime = value.split(";", 1)[0].strip().lower()
            return mime == JSON_TYPE or mime.endswith("+json")
    return False


def _fence(text: str) -> str:
    longest = max((len(run) for run in re.findall(r"`+", text)), default=0)
    return "`" * max(3, longest + 1)


def describe(document: dict[str, Any], connection: Connection) -> str:
    """The request in plain words, for the task the user approves or dismisses."""
    request = document["request"]
    lines = [
        "An agent drafted an API write for your approval. **Nothing has been sent.**",
        "",
        f"**{request['method']}** `{request['url']}` through the connection **{connection.name}**.",
        "",
    ]
    if request["headers"]:
        lines.append("Headers, besides the connection's own authentication:")
        lines.extend(f"- `{key}: {value}`" for key, value in sorted(request["headers"].items()))
    else:
        lines.append("No headers besides the connection's own authentication.")
    lines.append("")
    body = request["body"]
    if body is None:
        lines.append("No body.")
    else:
        size = len(body.encode("utf-8"))
        shown = body
        is_json = _is_json(request["headers"])
        if is_json:
            try:
                shown = json.dumps(json.loads(body), indent=2, ensure_ascii=False)
            except ValueError:
                pass
        cut = len(shown) > SHOWN_BODY_CHARS
        if cut:
            shown = shown[:SHOWN_BODY_CHARS]
        fence = _fence(shown)
        lines.append(f"Body ({size} bytes):")
        lines.append("")
        lines.append(f"{fence}{'json' if is_json else 'text'}")
        lines.append(shown)
        lines.append(fence)
        if cut:
            lines.append("")
            lines.append(f"The body is cut here; the whole request is in the `{ARTIFACT}` artifact.")
    lines.append("")
    lines.append("Approve sends this exact request once. Dismiss, and it is never sent.")
    if connection.idempotency_header:
        lines.append(
            f"The draft id goes in the `{connection.idempotency_header}` header, so the API can recognise a repeat."
        )
    caller = document.get("caller") or {}
    who = caller.get("agent") or "an unnamed caller"
    if caller.get("model"):
        who += f" ({caller['model']})"
    if caller.get("task_id"):
        who += f" for task {caller['task_id']}"
    lines.append("")
    lines.append(f"Draft `{document['id']}` · SHA-256 `{document['sha256']}` · drafted by {who}.")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# gateway.http-send
# ---------------------------------------------------------------------------


def tool_send(call: Call) -> dict[str, Any]:
    root = call.state_root()
    redactor = redactor_for(call)
    record = audit.Record(tool=call.tool, caller=call.caller())
    started = time.monotonic()
    try:
        output = _send(call, root, record, redactor)
        record.outcome = output["outcome"]
        return output
    except GatewayError as error:
        record.outcome = error.code
        raise
    except Exception:
        record.outcome = "backend_error"
        raise
    finally:
        record.duration_ms = int((time.monotonic() - started) * 1000)
        audit.append(root, record, redactor)


def _sendable(document: dict[str, Any], seen: str) -> None:
    if document["state"] != PENDING:
        raise GatewayError(
            "draft_not_pending",
            f"draft {document['id']} is {document['state']}, not pending; a draft is sent at most once",
            detail={"state": document["state"]},
        )
    current = digest(document["request"])
    if current != seen or current != document["sha256"]:
        raise GatewayError(
            "draft_changed",
            f"draft {document['id']} no longer matches the request that was approved; nothing was sent",
        )


def _dismissed(task: dict[str, Any]) -> bool:
    return task.get("terminal") is True or task.get("status") == "rejected"


def _send(call: Call, root: Path, record: audit.Record, redactor: Redactor) -> dict[str, Any]:
    raw = closed(call.input, SEND_KEYS, "input")
    draft_id = required(raw, "draft", str, "input")
    seen = required(raw, "sha256", str, "input").lower()
    if not DRAFT_ID.match(draft_id):
        raise GatewayError("unknown_draft", "a draft id looks like gd- and 16 hex digits")
    if not DIGEST.match(seen):
        raise GatewayError("invalid_argument", "`input.sha256` must be the draft's 64-digit hex SHA-256")
    record.draft = draft_id
    document = _load(root, draft_id)
    request = document["request"]
    parts = urllib.parse.urlsplit(request["url"])
    record.connection, record.method, record.path = document.get("connection"), request["method"], parts.path
    record.query = dict(urllib.parse.parse_qsl(parts.query))
    _sendable(document, seen)
    workspace = _workspace(call)
    task_id = document.get("task_id")
    if not isinstance(task_id, str) or not task_id:
        raise GatewayError("draft_not_pending", f"draft {draft_id} has no task", detail={"state": document["state"]})

    # The task must still be open: a dismissed draft is never sent.
    orbit = callbacks.Orbit(call.environ, workspace, redactor)
    task = orbit.call(
        "orbit.task.show",
        {"id": task_id, "fields": ["id", "status", "terminal", "execution_summary"]},
    )
    if _dismissed(task):
        with _lock(root, draft_id):
            current = _load(root, draft_id)
            if current["state"] == PENDING:
                current["state"] = DISMISSED
                _save(root, current, redactor)
        raise GatewayError(
            "draft_dismissed",
            f"the task for draft {draft_id} is {task.get('status') or 'closed'}, so the draft was dismissed; it is never sent",
        )

    # Checked again against the connection as it is now.
    connection = connections.find(root, str(document.get("connection")))
    method = request["method"]
    if method not in connection.drafts:
        raise GatewayError("method_not_allowed", f"connection {connection.name!r} no longer drafts {method}")
    if origin(request["url"]) != origin(connection.base_url) or not parts.path.startswith(
        urllib.parse.urlsplit(connection.base_url).path
    ):
        raise GatewayError(
            "path_outside_connection",
            f"the draft's URL is no longer under connection {connection.name!r}'s base_url",
        )
    body_value: Any = request["body"]
    if request["body"] is not None and _is_json(request["headers"]):
        try:
            body_value = json.loads(request["body"])
        except ValueError:
            pass
    _refuse_secrets(request, body_value, redactor)
    headers = dict(request["headers"])
    # An oauth2_device connection exchanges its refresh token here, before the
    # draft is marked sending: a refused sign-in leaves it pending.
    auth = credentials(call, root, connection, redactor)
    if auth is not None:
        headers[auth[0]] = auth[1]
    if connection.idempotency_header:
        headers[connection.idempotency_header] = draft_id
    headers["User-Agent"] = USER_AGENT

    # Mark it sending before anything goes out; only one caller gets past this.
    with _lock(root, draft_id):
        document = _load(root, draft_id)
        _sendable(document, seen)
        document["state"] = SENDING
        document["send"] = {"started_at": _now()}
        _save(root, document, redactor)

    try:
        response = _transmit(connection, method, request["url"], headers, request["body"])
    except _NotSent as failure:
        with _lock(root, draft_id):
            document["state"] = PENDING
            document["send"] = {"started_at": document["send"]["started_at"], "not_sent": failure.code}
            _save(root, document, redactor)
        raise GatewayError(failure.code, f"{failure.message}; nothing was sent and the draft is still pending", retryable=True) from None
    except _Unknown as unknown:
        with _lock(root, draft_id):
            document["state"] = OUTCOME_UNKNOWN
            document["send"] = {**document["send"], "finished_at": _now(), "outcome": OUTCOME_UNKNOWN}
            _save(root, document, redactor)
        message = (
            f"The request was sent but {unknown.reason}, so whether it took effect is unknown. "
            f"It will not be sent again: check {connection.name} before drafting it again."
        )
        output = _output(document, connection, OUTCOME_UNKNOWN, None)
        output["message"] = message
        output.update(_note_task(orbit, task_id, close=False, note=f"Outcome unknown: {message}"))
        return output

    status, data, response_headers, truncated = response
    content_type = response_headers.get("Content-Type")
    body, body_digest = shape_body(data, response_headers, truncated, redactor)
    record.status = status
    record.bytes = len(data)
    record.body_sha256 = body_digest
    record.truncated = truncated
    with _lock(root, draft_id):
        document["state"] = SENT
        document["send"] = {**document["send"], "finished_at": _now(), "outcome": SENT, "status": status}
        _save(root, document, redactor)
    output = _output(document, connection, SENT, status)
    output.update(
        {
            "content_type": content_type,
            "headers": returned_headers(response_headers),
            "body": body,
            "bytes": len(data),
            "truncated": truncated,
        }
    )
    output.update(
        _note_task(
            orbit,
            task_id,
            close=True,
            note=f"Sent once: {method} {request['url']} answered {status}.",
            status=task.get("status"),
            has_summary=_has_summary(task),
        )
    )
    return output


def _output(document: dict[str, Any], connection: Connection, outcome: str, status: int | None) -> dict[str, Any]:
    return {
        "draft": document["id"],
        "task_id": document["task_id"],
        "outcome": outcome,
        "connection": connection.name,
        "method": document["request"]["method"],
        "url": document["request"]["url"],
        "status": status,
        "content_type": None,
        "headers": {},
        "body": {"kind": "none"},
        "bytes": 0,
        "truncated": False,
    }


# `done` is reachable only from `review`, and moving to `in-progress` from
# anywhere but `backlog` needs a plan. The other open statuses therefore go
# through `backlog` first. An empty tuple means the task is already `done`.
_CLOSE_FROM: dict[str, tuple[str, ...]] = {
    "review": ("done",),
    "in-progress": ("review", "done"),
    "backlog": ("in-progress", "review", "done"),
    "proposed": ("backlog", "in-progress", "review", "done"),
    "someday": ("backlog", "in-progress", "review", "done"),
    "blocked": ("backlog", "in-progress", "review", "done"),
    "done": (),
}


def _has_summary(task: dict[str, Any]) -> bool:
    summary = task.get("execution_summary")
    return isinstance(summary, str) and bool(summary.strip())


def _note_task(
    orbit: callbacks.Orbit,
    task_id: str,
    close: bool,
    note: str,
    status: object = None,
    has_summary: bool = False,
) -> dict[str, Any]:
    """Comment on the task, and when `close` is set walk it to `done`.

    The send has happened either way, so a failed callback is reported in
    the result, never raised. A direct `status: done` is refused from every
    status but `review`, which used to leave the approval task open.
    """
    if not close:
        return _comment(orbit, task_id, note)

    steps = _CLOSE_FROM.get(status) if isinstance(status, str) else None
    if steps is None:
        # This status cannot reach `done`. Record the send and report the
        # close failure, so the task is not an untouched open approval.
        commented = _comment(orbit, task_id, note)
        if "task_error" in commented:
            return commented
        return {"task_closed": False, "task_error": "not_closable"}
    try:
        if not steps:
            orbit.call("orbit.task.update", {"id": task_id, "comment": note})
        else:
            for index, target in enumerate(steps):
                payload: dict[str, Any] = {"id": task_id, "status": target, "note": note}
                if index == 0:
                    payload["comment"] = note
                if target == "done" and not has_summary:
                    payload["execution_summary"] = note
                orbit.call("orbit.task.update", payload)
    except GatewayError as error:
        return {"task_closed": False, "task_error": error.code}
    return {"task_closed": True}


def _comment(orbit: callbacks.Orbit, task_id: str, note: str) -> dict[str, Any]:
    try:
        orbit.call("orbit.task.update", {"id": task_id, "comment": note})
    except GatewayError as error:
        return {"task_closed": False, "task_error": error.code}
    return {"task_closed": False}


# ---------------------------------------------------------------------------
# Transmit: one request, never retried
# ---------------------------------------------------------------------------


class _NotSent(Exception):
    """Nothing reached the server: the connection was never established."""

    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code
        self.message = message


class _Unknown(Exception):
    """The request was written (or may have been) and no full reply came back."""

    def __init__(self, reason: str):
        super().__init__(reason)
        self.reason = reason


def _transmit(
    connection: Connection, method: str, url: str, headers: dict[str, str], body: str | None
) -> tuple[int, bytes, Any, bool]:
    """Send one request; return (status, body bytes, headers, truncated)."""
    parts = urllib.parse.urlsplit(url)
    host = parts.hostname or ""
    timeout = connection.timeout_seconds
    deadline = time.monotonic() + timeout
    if parts.scheme == "https":
        conn: http.client.HTTPConnection = http.client.HTTPSConnection(
            host, parts.port, timeout=timeout, context=ssl.create_default_context()
        )
    else:
        conn = http.client.HTTPConnection(host, parts.port, timeout=timeout)
    target = parts.path + (f"?{parts.query}" if parts.query else "")
    try:
        # Connect (and the TLS handshake) first: a failure here sends nothing.
        try:
            conn.connect()
        except ssl.SSLError as error:
            raise _NotSent("tls_verification_failed", f"TLS failed for {host}: {error}") from None
        except (socket.timeout, TimeoutError):
            raise _NotSent("upstream_timeout", f"{host} did not accept a connection in {timeout}s") from None
        except (OSError, ValueError, http.client.HTTPException) as error:
            raise _NotSent("upstream_unreachable", f"could not reach {host}: {getattr(error, 'strerror', None) or error}") from None
        # From here on the request may have reached the server, so any
        # failure, expected or not, leaves the outcome unknown.
        try:
            conn.request(method, target, body=body.encode("utf-8") if body is not None else None, headers=headers)
            response = conn.getresponse()
        except (socket.timeout, TimeoutError):
            raise _Unknown(f"no reply came back within {timeout}s") from None
        except Exception as error:  # noqa: BLE001 - never reported as a failure (STD-03@2 §R31)
            raise _Unknown(f"the connection was lost before a reply ({type(error).__name__})") from None
        status = int(response.status)
        cut_off = False
        try:
            data = read_limited(response, connection.max_response_bytes + 1, deadline, timeout)
        except GatewayError:
            # The status is known, so the outcome is too. A read that fails
            # after the status (a chunked body ending early, a timeout)
            # drops what arrived; the body was not fully received.
            data = b""
            cut_off = True
        else:
            # Content-Length framing: HTTPResponse.read(amt) returns a short
            # read and leaves the unread count in length, instead of raising.
            # A chunked early close raises and is handled above. length is
            # None when the body is delimited by the connection closing.
            remaining = getattr(response, "length", None)
            cut_off = isinstance(remaining, int) and remaining > 0
        truncated = cut_off or len(data) > connection.max_response_bytes
        return status, data[: connection.max_response_bytes], response.headers, truncated
    finally:
        conn.close()
