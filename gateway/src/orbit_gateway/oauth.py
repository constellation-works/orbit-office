"""OAuth 2.0 for `oauth2_device` connections: the device authorization grant
(RFC 8628) that signs a connection in, and the refresh-token exchange every
call through it makes (RFC 6749 §6). Generic: nothing here knows which API
the connection reads.

- **Sign in.** `gateway.oauth-device-start` posts the client id and scopes to
  the connection's `device_url` and returns the user code and verification
  address. The device code stays in `{{plugin_state}}/oauth/<name>.device.json`
  (owner-only) and never leaves the process. `gateway.oauth-device-poll`
  posts it to `token_url`, once per call. When the user has approved, the
  refresh token goes to the connection's slot through `secret_updates`, which
  Orbit stores with compare-and-swap; it is never returned, logged or audited.
- **Each call.** `access_token` exchanges the slot's refresh token for an
  access token held only in memory. A rotated refresh token is written back
  through `secret_updates`, which Orbit applies before it returns the call's
  result. A refused one (`invalid_grant`) is `auth_expired`, and the slot
  version it was delivered at is recorded so `connection-list` reports the
  connection as `expired` until the user signs in again. A refusal of a
  token that an overlapping call has just rotated is `auth_stale`
  (retryable), not `auth_expired`.

With `http.py` and `drafts.py`, the only modules that open network
connections. Token endpoint replies are read up to 64 KiB, never followed
through a redirect, and every value they carry is masked in the answer.
"""

from __future__ import annotations

import http.client
import json
import re
import socket
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from . import audit, connections, state
from .connections import Connection
from .envelope import Call, GatewayError, closed, redactor_for, required
from .redact import Redactor

DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code"
MAX_REPLY_BYTES = 64 * 1024
DEFAULT_INTERVAL = 5
# RFC 8628 §3.5: a `slow_down` adds five seconds to the polling interval.
SLOW_DOWN_STEP = 5
MAX_INTERVAL = 60
MAX_EXPIRES = 3600
FLOW_VERSION = 1
OAUTH_ERROR = re.compile(r"^[a-z_]{1,64}$")
USER_AGENT = "orbit-gateway/0.1"
KEYS = {"connection"}


# ---------------------------------------------------------------------------
# The token endpoint
# ---------------------------------------------------------------------------


class _KeepEveryStatus(urllib.request.HTTPErrorProcessor):
    """Return every reply as is: a token endpoint's redirect is never followed."""

    def http_response(self, request, response):  # type: ignore[no-untyped-def]
        return response

    https_response = http_response


def _post(url: str, fields: dict[str, str], timeout: int) -> tuple[int, dict[str, Any] | None]:
    """POST a form and return (status, the JSON object answered, if any)."""
    opener = urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=ssl.create_default_context()),
        _KeepEveryStatus(),
    )
    request = urllib.request.Request(
        url,
        data=urllib.parse.urlencode(fields).encode("ascii"),
        method="POST",
        headers={
            "Content-Type": "application/x-www-form-urlencoded",
            "Accept": "application/json",
            "User-Agent": USER_AGENT,
        },
    )
    host = request.host
    try:
        with opener.open(request, timeout=timeout) as response:
            status = int(response.status)
            data = response.read(MAX_REPLY_BYTES + 1)
    except urllib.error.URLError as error:
        if isinstance(error.reason, ssl.SSLError):
            raise GatewayError("tls_verification_failed", f"TLS failed for {host}: {error.reason}") from None
        if isinstance(error.reason, (socket.timeout, TimeoutError)):
            raise GatewayError("upstream_timeout", f"{host} did not answer in time", retryable=True) from None
        raise GatewayError("upstream_unreachable", f"could not reach {host}: {error.reason}", retryable=True) from None
    except (socket.timeout, TimeoutError):
        raise GatewayError("upstream_timeout", f"{host} did not answer in time", retryable=True) from None
    except (OSError, ValueError, http.client.HTTPException) as error:
        raise GatewayError("upstream_unreachable", f"could not reach {host}: {type(error).__name__}", retryable=True) from None
    if len(data) > MAX_REPLY_BYTES:
        return status, None
    try:
        value = json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, ValueError):
        return status, None
    return status, value if isinstance(value, dict) else None


def _learn(call: Call, redactor: Redactor, value: str) -> None:
    """Mask `value` in everything this call writes or answers from now on."""
    call.learned.append(value)
    redactor.add([value])


def _oauth_error(body: dict[str, Any] | None) -> str | None:
    code = body.get("error") if body is not None else None
    return code if isinstance(code, str) and OAUTH_ERROR.match(code) else None


def _refused(what: str, status: int, body: dict[str, Any] | None) -> GatewayError:
    code = _oauth_error(body)
    detail: dict[str, Any] = {"status": status}
    if code is not None:
        detail["oauth_error"] = code
    return GatewayError(
        "auth_failed",
        f"the {what} answered {status}{f' ({code})' if code else ''}; check the connection's OAuth settings",
        detail=detail,
    )


# ---------------------------------------------------------------------------
# Each call: refresh token -> access token
# ---------------------------------------------------------------------------


def access_token(call: Call, root: Path, connection: Connection, redactor: Redactor) -> str:
    """An access token for this call, exchanged for the slot's refresh token."""
    slot = str(connection.slot)
    refresh = call.secrets().get(slot)
    if not refresh:
        raise GatewayError(
            "secret_unset",
            f"connection {connection.name!r} is not signed in; ask the user to sign in in Settings",
        )
    version = call.secret_version(slot)
    status, body = _post(
        str(connection.token_url),
        {
            "grant_type": "refresh_token",
            "refresh_token": refresh,
            "client_id": str(connection.client_id),
            "scope": " ".join(connection.scopes or []),
        },
        connection.timeout_seconds,
    )
    token = body.get("access_token") if body is not None and status == 200 else None
    if isinstance(token, str) and token:
        _learn(call, redactor, token)
        new = body.get("refresh_token") if body is not None else None
        if isinstance(new, str) and new and new != refresh:
            # Written back before the answer returns; Orbit applies it even
            # when the request below then fails.
            _learn(call, redactor, new)
            call.update_secret(slot, new)
            connections.update_auth_status(root, connection.name, {"rotated_from": version}, redactor)
        token_type = body.get("token_type", "Bearer") if body is not None else "Bearer"
        if not isinstance(token_type, str) or token_type.lower() != "bearer":
            raise GatewayError("auth_failed", "the token endpoint issued a token that is not a bearer token")
        return token
    if status in (400, 401) and _oauth_error(body) == "invalid_grant":
        if version is not None and connections.auth_status(root, connection.name).get("rotated_from") == version:
            raise GatewayError(
                "auth_stale",
                f"a call that overlapped this one already renewed {connection.name!r}'s sign-in; try again",
                retryable=True,
            )
        connections.update_auth_status(root, connection.name, {"expired_version": version}, redactor)
        raise GatewayError(
            "auth_expired",
            f"connection {connection.name!r}'s sign-in was revoked or has expired; "
            "ask the user to sign in again in Settings",
        )
    raise _refused("token endpoint", status, body)


# ---------------------------------------------------------------------------
# Sign in: gateway.oauth-device-start and gateway.oauth-device-poll
# ---------------------------------------------------------------------------


def _flow_path(root: Path, name: str) -> Path:
    return root / "oauth" / f"{name}.device.json"


def _binding(connection: Connection) -> list[Any]:
    return [connection.token_url, connection.client_id, connection.slot]


def _device_connection(root: Path, name: str) -> Connection:
    connection = connections.find(root, name)
    if connection.scheme != "oauth2_device" or connection.slot is None:
        raise GatewayError(
            "invalid_connection",
            f"connection {name!r} does not sign in with oauth2_device; only those connections use the device-code flow",
        )
    return connection


def _audited(call: Call, verb: Any) -> dict[str, Any]:
    """Run one sign-in step with its audit record (`verb` returns the output
    and whether the step is worth a record: a still-pending poll is not)."""
    root = call.state_root()
    redactor = redactor_for(call)
    record = audit.Record(tool=call.tool, caller=call.caller())
    started = time.monotonic()
    keep = True
    try:
        output, keep = verb(call, root, record, redactor)
        record.outcome = output.get("state", "ok")
        return output
    except GatewayError as error:
        record.outcome = error.code
        raise
    except Exception:
        record.outcome = "backend_error"
        raise
    finally:
        if keep:
            record.duration_ms = int((time.monotonic() - started) * 1000)
            audit.append(root, record, redactor)


def _bounded_int(value: Any, default: int, upper: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 1:
        return default
    return min(value, upper)


def tool_start(call: Call) -> dict[str, Any]:
    return _audited(call, _start)


def _start(call: Call, root: Path, record: audit.Record, redactor: Redactor) -> tuple[dict[str, Any], bool]:
    raw = closed(call.input, KEYS, "input")
    name = required(raw, "connection", str, "input")
    record.connection, record.method = name, "POST"
    connection = _device_connection(root, name)
    record.path = urllib.parse.urlsplit(str(connection.device_url)).path
    status, body = _post(
        str(connection.device_url),
        {"client_id": str(connection.client_id), "scope": " ".join(connection.scopes or [])},
        connection.timeout_seconds,
    )
    record.status = status
    if status != 200 or body is None:
        raise _refused("device authorization endpoint", status, body)
    device_code, user_code = body.get("device_code"), body.get("user_code")
    verification = body.get("verification_uri") or body.get("verification_url")
    if not all(isinstance(item, str) and item for item in (device_code, user_code, verification)):
        raise GatewayError("auth_failed", "the device authorization endpoint answered without a device code, user code or address")
    try:
        verification = connections.normalize_endpoint(verification, "verification_uri")
    except GatewayError:
        raise GatewayError("auth_failed", "the verification address the endpoint gave is not an https URL") from None
    complete = body.get("verification_uri_complete")
    expires_in = _bounded_int(body.get("expires_in"), 900, MAX_EXPIRES)
    interval = _bounded_int(body.get("interval"), DEFAULT_INTERVAL, MAX_INTERVAL)
    flow = {
        "schema_version": FLOW_VERSION,
        "connection": name,
        "binding": _binding(connection),
        "device_code": device_code,
        "interval": interval,
        "expires_at": time.time() + expires_in,
    }
    # Stored before the code is masked: the flow file is the one place it is kept.
    state.store_text(_flow_path(root, name), json.dumps(flow, sort_keys=True) + "\n", redactor)
    _learn(call, redactor, str(device_code))
    output: dict[str, Any] = {
        "connection": name,
        "state": "pending",
        "user_code": user_code,
        "verification_uri": verification,
        "expires_in": expires_in,
        "interval": interval,
    }
    if isinstance(complete, str) and complete.startswith(("https://", "http://")):
        output["verification_uri_complete"] = complete
    return output, True


def _load_flow(root: Path, connection: Connection) -> dict[str, Any] | None:
    text = state.read_text(_flow_path(root, connection.name))
    if text is None:
        return None
    try:
        flow = json.loads(text)
    except ValueError:
        flow = None
    if (
        not isinstance(flow, dict)
        or flow.get("binding") != _binding(connection)
        or not isinstance(flow.get("device_code"), str)
        or not isinstance(flow.get("expires_at"), (int, float))
    ):
        # Started for another endpoint, client or slot, or unreadable.
        state.remove(_flow_path(root, connection.name))
        return None
    return flow


def tool_poll(call: Call) -> dict[str, Any]:
    return _audited(call, _poll)


def _poll(call: Call, root: Path, record: audit.Record, redactor: Redactor) -> tuple[dict[str, Any], bool]:
    raw = closed(call.input, KEYS, "input")
    name = required(raw, "connection", str, "input")
    record.connection, record.method = name, "POST"
    connection = _device_connection(root, name)
    record.path = urllib.parse.urlsplit(str(connection.token_url)).path
    flow = _load_flow(root, connection)
    if flow is None:
        raise GatewayError(
            "device_flow_missing",
            f"no sign-in is in progress for {name!r}; start one with gateway.oauth-device-start",
        )
    device_code = flow["device_code"]
    # The flow file keeps the code, so it is rewritten with a redactor that does not mask it.
    store_redactor = redactor_for(call)
    _learn(call, redactor, device_code)
    path = _flow_path(root, name)
    if time.time() >= flow["expires_at"]:
        state.remove(path)
        return {"connection": name, "state": "expired"}, True
    status, body = _post(
        str(connection.token_url),
        {"grant_type": DEVICE_GRANT, "device_code": device_code, "client_id": str(connection.client_id)},
        connection.timeout_seconds,
    )
    record.status = status
    token = body.get("access_token") if body is not None and status == 200 else None
    if isinstance(token, str) and token:
        _learn(call, redactor, token)
        refresh = body.get("refresh_token") if body is not None else None
        state.remove(path)
        if not isinstance(refresh, str) or not refresh:
            raise GatewayError(
                "auth_failed",
                "the sign-in succeeded without a refresh token, so the gateway cannot renew it; "
                "add the scope the API needs for one (offline_access for Microsoft)",
            )
        _learn(call, redactor, refresh)
        call.update_secret(str(connection.slot), refresh)
        return {"connection": name, "state": "connected"}, True
    error = _oauth_error(body)
    interval = _bounded_int(flow.get("interval"), DEFAULT_INTERVAL, MAX_INTERVAL)
    if error == "authorization_pending":
        return {"connection": name, "state": "pending", "interval": interval}, False
    if error == "slow_down":
        flow["interval"] = min(interval + SLOW_DOWN_STEP, MAX_INTERVAL)
        state.store_text(path, json.dumps(flow, sort_keys=True) + "\n", store_redactor)
        return {"connection": name, "state": "pending", "interval": flow["interval"]}, False
    if error in ("access_denied", "expired_token"):
        state.remove(path)
        return {"connection": name, "state": "denied" if error == "access_denied" else "expired"}, True
    state.remove(path)
    raise _refused("token endpoint", status, body)
