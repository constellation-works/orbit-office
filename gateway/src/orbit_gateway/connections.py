"""Connections: what the user grants, stored in `{{plugin_state}}/connections.json`.

A connection names a base URL, how to authenticate and which methods run
directly. Its secret lives in one of the sixteen declared slots
(`conn_01` … `conn_16`); this module only ever handles slot names. The value
is set by the app with `orbit plugin secret set gateway <slot>` and reaches a
call in `context.secrets`.
"""

from __future__ import annotations

import ipaddress
import json
import re
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import state
from .envelope import Call, GatewayError, closed, redactor_for, required, typed
from .redact import Redactor

SLOTS = tuple(f"conn_{index:02d}" for index in range(1, 17))
SCHEMES = ("bearer", "basic", "header", "none")
DIRECT_METHODS = ("GET", "HEAD")
DRAFT_METHODS = ("POST", "PUT", "PATCH", "DELETE")
# Headers an agent may set on a request (gateway.http-get); an auth header
# may not be one of them.
AGENT_HEADERS = ("accept", "accept-language", "if-none-match", "if-modified-since", "range")
# More headers an agent may set on a drafted write (gateway.http-draft).
DRAFT_HEADERS = AGENT_HEADERS + ("content-type", "if-match", "if-unmodified-since")
# Headers the transport owns; never a connection's auth header.
TRANSPORT_HEADERS = ("host", "content-length", "transfer-encoding", "connection", "upgrade", "te", "trailer")
# Carry credentials whatever the connection; never an idempotency header.
CREDENTIAL_HEADERS = ("authorization", "proxy-authorization", "cookie")
NAME = re.compile(r"^[a-z][a-z0-9-]{0,47}$")
HEADER_NAME = re.compile(r"^[A-Za-z0-9!#$%&'*+.^_`|~-]{1,64}$")
DEFAULT_TIMEOUT = 20
MAX_TIMEOUT = 120
DEFAULT_MAX_RESPONSE = 1_048_576
MAX_RESPONSE_CAP = 16 * 1_048_576
FILE_VERSION = 1


@dataclass
class Connection:
    name: str
    base_url: str
    scheme: str
    slot: str | None
    header: str | None
    username: str | None
    methods: list[str]
    drafts: list[str]
    timeout_seconds: int
    max_response_bytes: int
    disabled: bool
    # The header a drafted write carries its draft id in, when the API has one.
    idempotency_header: str | None = None

    @property
    def origin(self) -> tuple[str, str]:
        parts = urllib.parse.urlsplit(self.base_url)
        return parts.scheme, parts.netloc

    def auth_header_name(self) -> str | None:
        if self.scheme in ("bearer", "basic"):
            return "Authorization"
        if self.scheme == "header":
            return self.header
        return None

    def to_json(self) -> dict[str, Any]:
        auth: dict[str, Any] = {"scheme": self.scheme}
        if self.slot is not None:
            auth["secret"] = self.slot
        if self.header is not None:
            auth["header"] = self.header
        if self.username is not None:
            auth["username"] = self.username
        out = {
            "name": self.name,
            "base_url": self.base_url,
            "auth": auth,
            "methods": self.methods,
            "drafts": self.drafts,
            "timeout_seconds": self.timeout_seconds,
            "max_response_bytes": self.max_response_bytes,
            "disabled": self.disabled,
        }
        if self.idempotency_header is not None:
            out["idempotency_header"] = self.idempotency_header
        return out

    def listing(self, secrets: dict[str, str]) -> dict[str, Any]:
        """What `connection-list` shows: never a value, only whether the slot is set."""
        auth: dict[str, Any] = {"scheme": self.scheme}
        if self.header is not None:
            auth["header"] = self.header
        if self.username is not None:
            auth["username"] = self.username
        out = {
            "name": self.name,
            "base_url": self.base_url,
            "auth": auth,
            "slot": self.slot,
            "slot_state": "none" if self.slot is None else ("set" if self.slot in secrets else "unset"),
            "methods": self.methods,
            "drafts": self.drafts,
            "timeout_seconds": self.timeout_seconds,
            "max_response_bytes": self.max_response_bytes,
            "disabled": self.disabled,
        }
        if self.idempotency_header is not None:
            out["idempotency_header"] = self.idempotency_header
        return out


# ---------------------------------------------------------------------------
# Validation
# ---------------------------------------------------------------------------


def _invalid(message: str) -> GatewayError:
    return GatewayError("invalid_connection", message)


def is_loopback(host: str) -> bool:
    if host == "localhost":
        return True
    try:
        return ipaddress.ip_address(host).is_loopback
    except ValueError:
        return False


def normalize_base_url(raw: str) -> str:
    """`https://host[:port]/path/`, or plain http to a loopback address."""
    try:
        parts = urllib.parse.urlsplit(raw.strip())
        port = parts.port
    except ValueError as error:
        raise _invalid(f"`base_url` is not a valid URL: {error}") from None
    scheme = parts.scheme.lower()
    host = (parts.hostname or "").lower()
    if not host:
        raise _invalid("`base_url` needs a host")
    if parts.username is not None or parts.password is not None:
        raise _invalid("`base_url` must not carry credentials; the secret goes in the connection's slot")
    if parts.query or parts.fragment or "?" in raw or "#" in raw:
        raise _invalid("`base_url` must not have a query or fragment")
    if scheme == "http":
        if not is_loopback(host):
            raise _invalid("`base_url` must be https; plain http is allowed only to a loopback address")
    elif scheme != "https":
        raise _invalid("`base_url` must be https (or plain http to a loopback address)")
    path = parts.path or "/"
    segments = [urllib.parse.unquote(segment) for segment in path.split("/")]
    if any(segment in (".", "..") or "\\" in segment for segment in segments):
        raise _invalid("`base_url` must not contain dot segments or backslashes")
    if not path.endswith("/"):
        path += "/"
    netloc = f"[{host}]" if ":" in host else host
    if port is not None:
        netloc = f"{netloc}:{port}"
    return urllib.parse.urlunsplit((scheme, netloc, path, "", ""))


def _methods(raw: Any, allowed: tuple[str, ...], key: str, default: list[str]) -> list[str]:
    if raw is None:
        return list(default)
    if not isinstance(raw, list) or not all(isinstance(item, str) for item in raw):
        raise GatewayError("invalid_argument", f"`input.{key}` must be an array of method names")
    out: list[str] = []
    for item in raw:
        method = item.upper()
        if method not in allowed:
            raise _invalid(f"`{key}` may list only {', '.join(allowed)}, not {item!r}")
        if method not in out:
            out.append(method)
    return out


def _auth(raw: Any) -> tuple[str, str | None, str | None]:
    auth = closed(raw, {"scheme", "header", "username"}, "input.auth")
    scheme = required(auth, "scheme", str, "input.auth")
    if scheme not in SCHEMES:
        raise _invalid(f"`auth.scheme` must be one of {', '.join(SCHEMES)}")
    header = typed(auth, "header", str, "input.auth", None)
    username = typed(auth, "username", str, "input.auth", None)
    if scheme == "header":
        if header is None or not HEADER_NAME.match(header):
            raise _invalid("`auth.header` must name the header the secret is sent in")
        if header.lower() in AGENT_HEADERS or header.lower() in TRANSPORT_HEADERS:
            raise _invalid(f"`auth.header` cannot be {header!r}")
    elif header is not None:
        raise _invalid("`auth.header` applies only to the `header` scheme")
    if scheme == "basic":
        if not username or ":" in username or any(ord(char) < 32 for char in username):
            raise _invalid("`auth.username` is required for `basic` and must not contain ':'")
    elif username is not None:
        raise _invalid("`auth.username` applies only to the `basic` scheme")
    return scheme, header, username


def _idempotency_header(raw: Any, auth_header: str | None) -> str | None:
    if raw is None:
        return None
    if not isinstance(raw, str) or not HEADER_NAME.match(raw):
        raise _invalid("`idempotency_header` must be a header name")
    lower = raw.lower()
    if (
        lower in DRAFT_HEADERS
        or lower in TRANSPORT_HEADERS
        or lower in CREDENTIAL_HEADERS
        or lower == "user-agent"
        or (auth_header is not None and lower == auth_header.lower())
    ):
        raise _invalid(f"`idempotency_header` cannot be {raw!r}")
    return raw


def _bounded(raw: dict[str, Any], key: str, default: int, upper: int) -> int:
    value = typed(raw, key, int, "input", default)
    if not 1 <= value <= upper:
        raise _invalid(f"`{key}` must be between 1 and {upper}")
    return value


# ---------------------------------------------------------------------------
# Storage
# ---------------------------------------------------------------------------


def _path(root: Path) -> Path:
    return root / "connections.json"


def _from_json(raw: Any) -> Connection:
    auth = raw.get("auth") or {}
    methods = raw.get("methods")
    if methods is None:
        methods = ["GET"]
    return Connection(
        name=raw["name"],
        base_url=raw["base_url"],
        scheme=auth.get("scheme", "none"),
        slot=auth.get("secret"),
        header=auth.get("header"),
        username=auth.get("username"),
        methods=list(methods),
        drafts=list(raw.get("drafts") or []),
        timeout_seconds=int(raw.get("timeout_seconds") or DEFAULT_TIMEOUT),
        max_response_bytes=int(raw.get("max_response_bytes") or DEFAULT_MAX_RESPONSE),
        disabled=bool(raw.get("disabled", False)),
        idempotency_header=raw.get("idempotency_header"),
    )


def load(root: Path) -> list[Connection]:
    text = state.read_text(_path(root))
    if text is None:
        return []
    try:
        document = json.loads(text)
        return [_from_json(item) for item in document["connections"]]
    except (ValueError, KeyError, TypeError, AttributeError) as error:
        raise GatewayError("state_corrupt", f"connections.json is not readable: {error}") from None


def save(root: Path, connections: list[Connection], redactor: Redactor) -> None:
    document = {"schema_version": FILE_VERSION, "connections": [item.to_json() for item in connections]}
    state.store_text(_path(root), json.dumps(document, indent=2, sort_keys=True) + "\n", redactor)


def find(root: Path, name: str) -> Connection:
    """An enabled connection by name, or `unknown_connection`."""
    for connection in load(root):
        if connection.name == name and not connection.disabled:
            return connection
    raise GatewayError(
        "unknown_connection",
        f"no enabled connection named {name!r}; see gateway.connection-list, and ask the user to add one in Settings",
    )


def basic_pairs(call: Call, secrets: dict[str, str]) -> list[tuple[str, str]]:
    """(username, password) for every basic-auth connection whose slot is set."""
    try:
        root = call.state_root()
    except GatewayError:
        return []
    pairs: list[tuple[str, str]] = []
    for connection in load(root):
        if connection.scheme == "basic" and connection.slot in secrets and connection.username:
            pairs.append((connection.username, secrets[connection.slot]))
    return pairs


# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------


def tool_list(call: Call) -> dict[str, Any]:
    closed(call.input, set(), "input")
    secrets = call.secrets()
    connections = load(call.state_root())
    used = {item.slot for item in connections if item.slot}
    free = [slot for slot in SLOTS if slot not in used and slot not in secrets]
    return {
        "connections": [item.listing(secrets) for item in sorted(connections, key=lambda c: c.name)],
        "free_slots": len(free),
    }


PUT_KEYS = {
    "name", "base_url", "auth", "methods", "drafts", "timeout_seconds", "max_response_bytes", "disabled",
    "idempotency_header",
}


def tool_put(call: Call) -> dict[str, Any]:
    raw = closed(call.input, PUT_KEYS, "input")
    name = required(raw, "name", str, "input")
    if not NAME.match(name):
        raise _invalid("`name` must be lowercase letters, digits and '-', starting with a letter (at most 48)")
    base_url = normalize_base_url(required(raw, "base_url", str, "input"))
    scheme, header, username = _auth(required(raw, "auth", dict, "input"))
    methods = _methods(raw.get("methods"), DIRECT_METHODS, "methods", ["GET"])
    drafts = _methods(raw.get("drafts"), DRAFT_METHODS, "drafts", [])
    timeout = _bounded(raw, "timeout_seconds", DEFAULT_TIMEOUT, MAX_TIMEOUT)
    max_bytes = _bounded(raw, "max_response_bytes", DEFAULT_MAX_RESPONSE, MAX_RESPONSE_CAP)
    disabled = typed(raw, "disabled", bool, "input", False)
    auth_name = header if scheme == "header" else ("Authorization" if scheme in ("bearer", "basic") else None)
    idempotency = _idempotency_header(raw.get("idempotency_header"), auth_name)

    secrets = call.secrets()
    root = call.state_root()
    redactor = redactor_for(call)
    with state.locked(root / "connections.lock"):
        connections = load(root)
        existing = next((item for item in connections if item.name == name), None)
        others = [item for item in connections if item.name != name]
        candidate = Connection(
            name=name,
            base_url=base_url,
            scheme=scheme,
            slot=None,
            header=header,
            username=username,
            methods=methods,
            drafts=drafts,
            timeout_seconds=timeout,
            max_response_bytes=max_bytes,
            disabled=disabled,
            idempotency_header=idempotency,
        )
        released: str | None = None
        if scheme != "none":
            # Keep the slot only while the origin is unchanged, so a stored
            # secret is never sent to a host other than the one it was set for.
            if existing is not None and existing.slot and existing.origin == candidate.origin:
                candidate.slot = existing.slot
            else:
                candidate.slot = _free_slot(others, secrets, existing)
        if existing is not None and existing.slot and existing.slot != candidate.slot:
            released = existing.slot
        save(root, sorted(others + [candidate], key=lambda c: c.name), redactor)
    return {
        "connection": candidate.listing(secrets),
        "created": existing is None,
        "released_slot": released,
    }


def _free_slot(others: list[Connection], secrets: dict[str, str], existing: Connection | None) -> str:
    """A slot no other connection uses and that holds no value (a stale value
    left in a slot would otherwise be sent to the new connection's host)."""
    used = {item.slot for item in others if item.slot}
    if existing is not None and existing.slot:
        used.add(existing.slot)
    for slot in SLOTS:
        if slot not in used and slot not in secrets:
            return slot
    raise GatewayError(
        "no_free_slot",
        "all sixteen secret slots are in use or still hold a value; remove an unused slot's "
        "value with `orbit plugin secret rm gateway <slot>`",
    )
