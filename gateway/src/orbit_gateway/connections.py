"""Connections: what the user grants, stored in `{{plugin_state}}/connections.json`.

A connection names a base URL, how to authenticate and which methods run
directly. Its secret lives in one of the sixteen declared slots
(`conn_01` … `conn_16`); this module only ever handles slot names. The value
is set by the app with `orbit plugin secret set gateway <slot>` (or, for
`oauth2_device`, by the sign-in through `secret_updates`) and reaches a call
in `context.secrets`.

- `secret_path`: `base_url` is the origin only, and the slot holds the path
  and query of the one resource the connection reads.
- `oauth2_device`: the connection names `token_url`, `device_url`,
  `client_id` and `scopes`; the slot holds the refresh token.
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
SCHEMES = ("bearer", "basic", "header", "none", "secret_path", "oauth2_device")
# What a `secret_path` slot reads as wherever it would appear.
SECRET_PATH = "<secret_path>"
# Path and query parts of a `secret_path` slot this long are masked on their own too.
SECRET_PART_MIN = 16
SCOPE = re.compile(r"^[\x21\x23-\x5B\x5D-\x7E]{1,256}$")
CLIENT_ID = re.compile(r"^[\x21-\x7E]{1,256}$")
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
    # oauth2_device only.
    token_url: str | None = None
    device_url: str | None = None
    client_id: str | None = None
    scopes: list[str] | None = None

    @property
    def origin(self) -> tuple[str, str]:
        parts = urllib.parse.urlsplit(self.base_url)
        return parts.scheme, parts.netloc

    def binding(self) -> tuple[Any, ...]:
        """What a slot's value is bound to besides the origin: a slot keeps its
        value only while this is unchanged. A path, a refresh token and a
        header value are not interchangeable, and a refresh token belongs to
        its token endpoint and client."""
        if self.scheme == "oauth2_device":
            token = urllib.parse.urlsplit(str(self.token_url))
            return self.scheme, token.scheme, token.netloc, self.client_id
        if self.scheme == "secret_path":
            return (self.scheme,)
        return ("value",)

    def auth_header_name(self) -> str | None:
        if self.scheme in ("bearer", "basic", "oauth2_device"):
            return "Authorization"
        if self.scheme == "header":
            return self.header
        return None

    def _auth_fields(self) -> dict[str, Any]:
        auth: dict[str, Any] = {"scheme": self.scheme}
        if self.header is not None:
            auth["header"] = self.header
        if self.username is not None:
            auth["username"] = self.username
        if self.scheme == "oauth2_device":
            auth.update(
                {
                    "token_url": self.token_url,
                    "device_url": self.device_url,
                    "client_id": self.client_id,
                    "scopes": list(self.scopes or []),
                }
            )
        return auth

    def to_json(self) -> dict[str, Any]:
        auth = self._auth_fields()
        if self.slot is not None:
            auth["secret"] = self.slot
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

    def listing(self, secrets: dict[str, str], expired: bool = False) -> dict[str, Any]:
        """What `connection-list` shows: never a value, only whether the slot is set.

        An `oauth2_device` connection also says whether it is signed in:
        `expired` means its refresh token was refused (`auth_expired`).
        """
        out = {
            "name": self.name,
            "base_url": self.base_url,
            "auth": self._auth_fields(),
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
        if self.scheme == "oauth2_device":
            if self.slot is None or self.slot not in secrets:
                out["auth_state"] = "signed_out"
            else:
                out["auth_state"] = "expired" if expired else "signed_in"
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


def normalize_endpoint(raw: Any, key: str) -> str:
    """An OAuth endpoint: https (or plain http to a loopback address), no
    credentials and no fragment."""
    if not isinstance(raw, str) or not raw.strip():
        raise _invalid(f"`auth.{key}` is required for `oauth2_device`")
    try:
        parts = urllib.parse.urlsplit(raw.strip())
        port = parts.port
    except ValueError as error:
        raise _invalid(f"`auth.{key}` is not a valid URL: {error}") from None
    scheme = parts.scheme.lower()
    host = (parts.hostname or "").lower()
    if not host:
        raise _invalid(f"`auth.{key}` needs a host")
    if parts.username is not None or parts.password is not None:
        raise _invalid(f"`auth.{key}` must not carry credentials")
    if parts.fragment or "#" in raw:
        raise _invalid(f"`auth.{key}` must not have a fragment")
    if scheme == "http":
        if not is_loopback(host):
            raise _invalid(f"`auth.{key}` must be https; plain http is allowed only to a loopback address")
    elif scheme != "https":
        raise _invalid(f"`auth.{key}` must be https (or plain http to a loopback address)")
    netloc = f"[{host}]" if ":" in host else host
    if port is not None:
        netloc = f"{netloc}:{port}"
    return urllib.parse.urlunsplit((scheme, netloc, parts.path or "/", parts.query, ""))


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


OAUTH_KEYS = ("token_url", "device_url", "client_id", "scopes")


@dataclass
class Auth:
    scheme: str
    header: str | None = None
    username: str | None = None
    token_url: str | None = None
    device_url: str | None = None
    client_id: str | None = None
    scopes: list[str] | None = None


def _oauth(auth: dict[str, Any], scheme: str) -> dict[str, Any]:
    if scheme != "oauth2_device":
        present = [key for key in OAUTH_KEYS if auth.get(key) is not None]
        if present:
            raise _invalid(f"`auth.{present[0]}` applies only to the `oauth2_device` scheme")
        return {}
    client_id = auth.get("client_id")
    if not isinstance(client_id, str) or not CLIENT_ID.match(client_id):
        raise _invalid("`auth.client_id` is required for `oauth2_device` (printable, no spaces)")
    scopes = auth.get("scopes")
    if not isinstance(scopes, list) or not scopes or len(scopes) > 32:
        raise _invalid("`auth.scopes` must list between 1 and 32 scopes")
    if not all(isinstance(scope, str) and SCOPE.match(scope) for scope in scopes):
        raise _invalid("each of `auth.scopes` must be one OAuth scope token (no spaces or quotes)")
    return {
        "token_url": normalize_endpoint(auth.get("token_url"), "token_url"),
        "device_url": normalize_endpoint(auth.get("device_url"), "device_url"),
        "client_id": client_id,
        "scopes": list(dict.fromkeys(scopes)),
    }


def _auth(raw: Any) -> Auth:
    auth = closed(raw, {"scheme", "header", "username", *OAUTH_KEYS}, "input.auth")
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
    return Auth(scheme, header, username, **_oauth(auth, scheme))


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
        token_url=auth.get("token_url"),
        device_url=auth.get("device_url"),
        client_id=auth.get("client_id"),
        scopes=list(auth["scopes"]) if isinstance(auth.get("scopes"), list) else None,
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


def _loaded(call: Call) -> list[Connection]:
    try:
        root = call.state_root()
    except GatewayError:
        return []
    return load(root)


def secret_path_labels(call: Call, secrets: dict[str, str]) -> dict[str, str]:
    """Each set `secret_path` slot, its path and query, and each long part of
    them, read as `<secret_path>` (never `[secret]`) wherever they appear."""
    labels: dict[str, str] = {}
    for connection in _loaded(call):
        value = secrets.get(connection.slot or "") if connection.scheme == "secret_path" else None
        if not value:
            continue
        labels[value] = SECRET_PATH
        path, _, query = value.partition("?")
        parts = [path, query, *path.split("/"), *(item.partition("=")[2] for item in query.split("&"))]
        for part in parts:
            if len(part) >= SECRET_PART_MIN:
                labels[part] = SECRET_PATH
    return labels


def basic_pairs(call: Call, secrets: dict[str, str]) -> list[tuple[str, str]]:
    """(username, password) for every basic-auth connection whose slot is set."""
    pairs: list[tuple[str, str]] = []
    for connection in _loaded(call):
        if connection.scheme == "basic" and connection.slot in secrets and connection.username:
            pairs.append((connection.username, secrets[connection.slot]))
    return pairs


# ---------------------------------------------------------------------------
# Tools
# ---------------------------------------------------------------------------


def _auth_path(root: Path, name: str) -> Path:
    return root / "oauth" / f"{name}.json"


def auth_status(root: Path, name: str) -> dict[str, Any]:
    """An `oauth2_device` connection's sign-in record: the slot version whose
    refresh token was refused (`expired_version`) and the version the last
    rotation replaced (`rotated_from`). Versions are Orbit's opaque stamps,
    never values."""
    text = state.read_text(_auth_path(root, name))
    if text is None:
        return {}
    try:
        record = json.loads(text)
    except ValueError:
        return {}
    return record if isinstance(record, dict) else {}


def update_auth_status(root: Path, name: str, changes: dict[str, Any], redactor: Redactor) -> None:
    """Merge `changes` into the sign-in record under its lock, so overlapping
    calls never drop each other's field."""
    with state.locked(root / "oauth" / f"{name}.lock"):
        record = {**auth_status(root, name), **changes}
        state.store_text(_auth_path(root, name), json.dumps(record, sort_keys=True) + "\n", redactor)


def is_expired(call: Call, root: Path, connection: Connection) -> bool:
    """True when the slot still holds the refresh token that was refused."""
    if connection.scheme != "oauth2_device" or connection.slot is None:
        return False
    version = call.secret_version(connection.slot)
    expired = auth_status(root, connection.name).get("expired_version")
    return version is not None and expired == version


def tool_list(call: Call) -> dict[str, Any]:
    closed(call.input, set(), "input")
    secrets = call.secrets()
    root = call.state_root()
    connections = load(root)
    used = {item.slot for item in connections if item.slot}
    free = [slot for slot in SLOTS if slot not in used and slot not in secrets]
    return {
        "connections": [
            item.listing(secrets, is_expired(call, root, item)) for item in sorted(connections, key=lambda c: c.name)
        ],
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
    auth = _auth(required(raw, "auth", dict, "input"))
    methods = _methods(raw.get("methods"), DIRECT_METHODS, "methods", ["GET"])
    drafts = _methods(raw.get("drafts"), DRAFT_METHODS, "drafts", [])
    if auth.scheme == "secret_path":
        if urllib.parse.urlsplit(base_url).path != "/":
            raise _invalid(
                "a `secret_path` connection's `base_url` is the origin only (https://host/); "
                "the path and query go in its secret"
            )
        if drafts:
            raise _invalid("a `secret_path` connection reads one resource; it cannot draft writes")
    timeout = _bounded(raw, "timeout_seconds", DEFAULT_TIMEOUT, MAX_TIMEOUT)
    max_bytes = _bounded(raw, "max_response_bytes", DEFAULT_MAX_RESPONSE, MAX_RESPONSE_CAP)
    disabled = typed(raw, "disabled", bool, "input", False)
    auth_name = auth.header if auth.scheme == "header" else (
        "Authorization" if auth.scheme in ("bearer", "basic", "oauth2_device") else None
    )
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
            scheme=auth.scheme,
            slot=None,
            header=auth.header,
            username=auth.username,
            methods=methods,
            drafts=drafts,
            timeout_seconds=timeout,
            max_response_bytes=max_bytes,
            disabled=disabled,
            idempotency_header=idempotency,
            token_url=auth.token_url,
            device_url=auth.device_url,
            client_id=auth.client_id,
            scopes=auth.scopes,
        )
        released: str | None = None
        if auth.scheme != "none":
            # Keep the slot only while the origin (and what the value is bound
            # to) is unchanged, so a stored secret is never sent to a host
            # other than the one it was set for, nor read as another kind.
            if (
                existing is not None
                and existing.slot
                and existing.origin == candidate.origin
                and existing.binding() == candidate.binding()
            ):
                candidate.slot = existing.slot
            else:
                candidate.slot = _free_slot(others, secrets, existing)
        if existing is not None and existing.slot and existing.slot != candidate.slot:
            released = existing.slot
        save(root, sorted(others + [candidate], key=lambda c: c.name), redactor)
    return {
        "connection": candidate.listing(secrets, is_expired(call, root, candidate)),
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
