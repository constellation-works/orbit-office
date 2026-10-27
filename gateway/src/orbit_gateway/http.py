"""`gateway.http-get`: one GET or HEAD through a named connection.

Resolve the connection, keep the path under its `base_url`, allow only a
short list of agent headers, add the auth from the slot, follow at most three
same-origin redirects within the base path, and return the status, a fixed
header set and the body (JSON parsed, text as text, anything else as size and SHA-256), cut at
`max_response_bytes`. Every call, refused or not, appends one audit record.

A `secret_path` connection takes no `path`: its slot holds the path and
query, appended to the origin at send time, and the URL, the audit and the
redirect it reports read `<secret_path>` instead. An `oauth2_device`
connection's auth is an access token exchanged for the slot's refresh token
on each call (`oauth.py`).

With `drafts.py` and `oauth.py`, the only modules that open network connections.
"""

from __future__ import annotations

import base64
import hashlib
import http.client
import json
import socket
import ssl
import time
import urllib.error
import urllib.parse
import urllib.request
from email.message import Message
from pathlib import Path
from typing import Any

from . import audit, connections, oauth
from .connections import AGENT_HEADERS, CREDENTIAL_HEADERS, SECRET_PATH, Connection
from .envelope import Call, GatewayError, closed, redactor_for, required, typed
from .redact import Redactor

MAX_REDIRECTS = 3
REDIRECT_STATUSES = (301, 302, 303, 307, 308)
# The response headers returned, whatever else the server sends.
RETURNED_HEADERS = (
    "cache-control",
    "content-language",
    "content-length",
    "content-range",
    "content-type",
    "date",
    "etag",
    "expires",
    "last-modified",
    "link",
    "location",
    "retry-after",
    "x-ratelimit-limit",
    "x-ratelimit-remaining",
    "x-ratelimit-reset",
)
TEXT_TYPES = ("application/xml", "application/javascript", "application/x-www-form-urlencoded", "application/x-ndjson")
USER_AGENT = "orbit-gateway/0.1"
INPUT_KEYS = {"connection", "path", "query", "headers", "method"}


def tool_get(call: Call) -> dict[str, Any]:
    root = call.state_root()
    redactor = redactor_for(call)
    record = audit.Record(tool=call.tool, caller=call.caller())
    started = time.monotonic()
    try:
        output = _get(call, root, record, redactor)
        if output.get("redirect") is not None:
            record.outcome = "redirect_not_followed"
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


def _get(call: Call, root: Path, record: audit.Record, redactor: Redactor) -> dict[str, Any]:
    raw = closed(call.input, INPUT_KEYS, "input")
    name = required(raw, "connection", str, "input")
    path = typed(raw, "path", str, "input", "")
    query = typed(raw, "query", dict, "input", {})
    headers = typed(raw, "headers", dict, "input", {})
    method = typed(raw, "method", str, "input", "GET").upper()
    record.connection, record.method, record.path, record.query = name, method, path, query

    # 1. Resolve.
    connection = connections.find(root, name)
    if method not in ("GET", "HEAD") or method not in connection.methods:
        raise GatewayError(
            "method_not_allowed",
            f"connection {name!r} runs {', '.join(connection.methods)} directly; {method} is not one of them",
        )
    if connection.scheme == "secret_path":
        record.path = SECRET_PATH
        url = secret_url(connection, path, query, call.secrets())
    else:
        url = resolve(connection.base_url, path, query)
        record.path = urllib.parse.urlsplit(url).path
    # 2. Headers.
    request_headers = agent_headers(connection, headers)
    # 3. Send.
    auth = credentials(call, root, connection, redactor)
    if auth is not None:
        request_headers[auth[0]] = auth[1]
    request_headers["User-Agent"] = USER_AGENT
    return _send(connection, method, url, request_headers, record, redactor)


# ---------------------------------------------------------------------------
# Resolve
# ---------------------------------------------------------------------------

_SEGMENT_SAFE = "!$&'()*+,;=:@-._~"


def resolve(base_url: str, path: str, query: dict[str, Any]) -> str:
    """Join `path` to `base_url` and refuse anything that leaves it."""
    if any(ord(char) < 0x20 or ord(char) == 0x7F for char in path) or "\\" in path:
        raise GatewayError("path_outside_connection", "the path has a control character or backslash")
    if "?" in path or "#" in path:
        raise GatewayError("invalid_argument", "put query parameters in `query`, not in `path`")
    parts = urllib.parse.urlsplit(path)
    if parts.scheme or parts.netloc or path.startswith("//"):
        raise GatewayError(
            "path_outside_connection",
            "the path must be relative to the connection's base_url, not a URL or another host",
        )
    base = urllib.parse.urlsplit(base_url)
    joined = path if path.startswith("/") else base.path + path
    resolved: list[str] = []
    raw_segments = joined.split("/")[1:]
    for index, segment in enumerate(raw_segments):
        decoded = urllib.parse.unquote(segment)
        last = index == len(raw_segments) - 1
        if "/" in decoded or "\\" in decoded:
            raise GatewayError("path_outside_connection", "the path has an encoded slash")
        if decoded == ".":
            if last:
                resolved.append("")
            continue
        if decoded == "..":
            if resolved:
                resolved.pop()
            else:
                raise GatewayError("path_outside_connection", "the path climbs above the host root")
            if last:
                resolved.append("")
            continue
        resolved.append(urllib.parse.quote(decoded, safe=_SEGMENT_SAFE))
    final_path = "/" + "/".join(resolved)
    if not (final_path + ("/" if not final_path.endswith("/") else "")).startswith(base.path):
        raise GatewayError(
            "path_outside_connection",
            f"the path resolves outside the connection's base_url ({base.path})",
        )
    encoded = encode_query(query)
    return urllib.parse.urlunsplit((base.scheme, base.netloc, final_path, encoded, ""))


def secret_url(connection: Connection, path: str, query: dict[str, Any], secrets: dict[str, str]) -> str:
    """A `secret_path` connection's URL: its origin, then the slot's path and query."""
    if path:
        raise GatewayError(
            "path_not_allowed",
            f"connection {connection.name!r} names one resource and its path is its secret ({SECRET_PATH}); "
            "leave `path` empty",
        )
    value = secrets.get(str(connection.slot))
    if not value:
        raise GatewayError(
            "secret_unset",
            f"connection {connection.name!r} has no secret in slot {connection.slot}; ask the user to set it in Settings",
        )
    base = urllib.parse.urlsplit(connection.base_url)
    url = f"{base.scheme}://{base.netloc}{value}"
    if (
        not value.startswith("/")
        or value.startswith("//")
        or "#" in value
        or "\\" in value
        or not value.isascii()
        or any(ord(char) <= 0x20 or ord(char) == 0x7F for char in value)
        or urllib.parse.urlsplit(url).netloc != base.netloc
    ):
        raise GatewayError(
            "secret_invalid",
            f"the secret in slot {connection.slot} is not a path and query starting with '/' ({SECRET_PATH}); "
            "ask the user to set it again in Settings",
        )
    encoded = encode_query(query)
    if encoded:
        url += ("&" if "?" in value else "?") + encoded
    return url


def shown_url(connection: Connection, url: str) -> str:
    """The URL as returned: a `secret_path` connection's path and query read `<secret_path>`."""
    if connection.scheme != "secret_path":
        return url
    parts = urllib.parse.urlsplit(url)
    return f"{parts.scheme}://{parts.netloc}{SECRET_PATH}"


def encode_query(query: dict[str, Any]) -> str:
    pairs: list[tuple[str, str]] = []
    for key, value in query.items():
        values = value if isinstance(value, list) else [value]
        for item in values:
            if isinstance(item, bool):
                pairs.append((key, "true" if item else "false"))
            elif isinstance(item, (str, int, float)):
                pairs.append((key, str(item)))
            else:
                raise GatewayError("invalid_argument", f"query value for {key!r} must be a string, number or boolean")
    return urllib.parse.urlencode(pairs)


# ---------------------------------------------------------------------------
# Headers
# ---------------------------------------------------------------------------


def agent_headers(
    connection: Connection, headers: dict[str, Any], allowed: tuple[str, ...] = AGENT_HEADERS
) -> dict[str, str]:
    """The agent's headers, refused unless every one is on the allowlist.

    Credential headers and the connection's own auth header are never allowed.
    """
    own = (connection.auth_header_name() or "").lower()
    out: dict[str, str] = {}
    for name, value in headers.items():
        lower = name.lower()
        if lower in CREDENTIAL_HEADERS or (own and lower == own):
            raise GatewayError(
                "header_not_allowed",
                f"{name!r} carries credentials; the gateway adds the connection's auth itself",
            )
        if lower not in allowed:
            raise GatewayError(
                "header_not_allowed",
                f"{name!r} is not allowed; an agent may set only {', '.join(allowed)}",
            )
        if not isinstance(value, str) or "\r" in value or "\n" in value:
            raise GatewayError("invalid_argument", f"header {name!r} must be a single-line string")
        try:
            value.encode("latin-1")
        except UnicodeEncodeError:
            raise GatewayError(
                "invalid_argument",
                f"header {name!r} must be encodable as latin-1; http.client cannot send it",
            ) from None
        out[name] = value
    return out


def credentials(call: Call, root: Path, connection: Connection, redactor: Redactor) -> tuple[str, str] | None:
    """The auth header for one request through the connection."""
    if connection.scheme == "oauth2_device":
        return "Authorization", f"Bearer {oauth.access_token(call, root, connection, redactor)}"
    return auth_header(connection, call.secrets())


def auth_header(connection: Connection, secrets: dict[str, str]) -> tuple[str, str] | None:
    """The auth header for a connection whose slot holds the credential itself."""
    if connection.scheme in ("none", "secret_path") or connection.slot is None:
        return None
    if connection.scheme == "oauth2_device":
        raise GatewayError("backend_error", "an oauth2_device connection's header comes from credentials()")
    value = secrets.get(connection.slot)
    if not value:
        raise GatewayError(
            "secret_unset",
            f"connection {connection.name!r} has no secret in slot {connection.slot}; "
            f"ask the user to set it in Settings",
        )
    if connection.scheme == "bearer":
        return "Authorization", f"Bearer {value}"
    if connection.scheme == "basic":
        token = base64.b64encode(f"{connection.username}:{value}".encode("utf-8")).decode("ascii")
        return "Authorization", f"Basic {token}"
    return str(connection.header), value


# ---------------------------------------------------------------------------
# Send
# ---------------------------------------------------------------------------


class _KeepEveryStatus(urllib.request.HTTPErrorProcessor):
    """Return every response as is: redirects are followed here, not by urllib."""

    def http_response(self, request, response):  # type: ignore[no-untyped-def]
        return response

    https_response = http_response


def _opener() -> urllib.request.OpenerDirector:
    tls = ssl.create_default_context()  # verification and hostname checks always on
    return urllib.request.build_opener(
        urllib.request.ProxyHandler({}),
        urllib.request.HTTPSHandler(context=tls),
        _KeepEveryStatus(),
    )


def origin(url: str) -> tuple[str, str, int]:
    parts = urllib.parse.urlsplit(url)
    scheme = parts.scheme.lower()
    port = parts.port or (443 if scheme == "https" else 80)
    return scheme, (parts.hostname or "").lower(), port


def _redirect_within_base(base_url: str, target: str) -> str | None:
    """Validate and normalize the target path using the initial request's rules."""
    parts = urllib.parse.urlsplit(target)
    try:
        normalized = resolve(base_url, parts.path or "/", {})
    except GatewayError:
        return None
    path = urllib.parse.urlsplit(normalized).path
    return urllib.parse.urlunsplit((parts.scheme, parts.netloc, path, parts.query, ""))


def _send(
    connection: Connection,
    method: str,
    url: str,
    headers: dict[str, str],
    record: audit.Record,
    redactor: Redactor,
) -> dict[str, Any]:
    opener = _opener()
    home = origin(connection.base_url)
    # `timeout_seconds` bounds the whole call, redirects and body included.
    deadline = time.monotonic() + connection.timeout_seconds
    redirects = 0
    unfollowed: dict[str, Any] | None = None
    while True:
        request = urllib.request.Request(url, method=method, headers=headers)
        response = _open(opener, request, _remaining(deadline, connection.timeout_seconds))
        status = int(response.status)
        location = response.headers.get("Location")
        if status in REDIRECT_STATUSES and location:
            target = urllib.parse.urldefrag(urllib.parse.urljoin(url, location))[0]
            if origin(target) != home:
                unfollowed = {"location": shown_url(connection, target), "reason": "other_origin"}
            elif (safe_target := _redirect_within_base(connection.base_url, target)) is None:
                unfollowed = {"location": shown_url(connection, target), "reason": "outside_base"}
            elif redirects >= MAX_REDIRECTS:
                unfollowed = {"location": shown_url(connection, target), "reason": "limit"}
            else:
                response.close()
                redirects += 1
                url = safe_target
                continue
        break
    record.status = status
    record.redirects = redirects
    record.path = SECRET_PATH if connection.scheme == "secret_path" else urllib.parse.urlsplit(url).path
    try:
        data = b"" if method == "HEAD" else read_limited(response, connection.max_response_bytes + 1, deadline, connection.timeout_seconds)
        remaining = getattr(response, "length", None)
    finally:
        response.close()
    cut_off = method != "HEAD" and isinstance(remaining, int) and remaining > 0
    truncated = cut_off or len(data) > connection.max_response_bytes
    data = data[: connection.max_response_bytes]
    content_type = response.headers.get("Content-Type")
    body, digest = shape_body(data, response.headers, truncated, redactor)
    record.bytes = len(data)
    record.body_sha256 = digest
    output: dict[str, Any] = {
        "connection": connection.name,
        "method": method,
        "url": shown_url(connection, url),
        "status": status,
        "content_type": content_type,
        "headers": returned_headers(response.headers),
        "body": body,
        "bytes": len(data),
        "truncated": truncated,
        "redirects": redirects,
    }
    if unfollowed is not None:
        output["redirect"] = unfollowed
    return output


def _remaining(deadline: float, timeout: int) -> float:
    left = deadline - time.monotonic()
    if left <= 0:
        raise GatewayError("upstream_timeout", f"the call did not finish in {timeout}s", retryable=True)
    return left


def _open(opener: urllib.request.OpenerDirector, request: urllib.request.Request, timeout: float) -> Any:
    try:
        return opener.open(request, timeout=timeout)
    except urllib.error.URLError as error:
        reason = error.reason
        if isinstance(reason, ssl.SSLError):
            raise GatewayError("tls_verification_failed", f"TLS failed for {request.host}: {reason}") from None
        if isinstance(reason, (socket.timeout, TimeoutError)):
            raise GatewayError("upstream_timeout", f"{request.host} did not answer in time", retryable=True) from None
        raise GatewayError("upstream_unreachable", f"could not reach {request.host}: {reason}", retryable=True) from None
    except (socket.timeout, TimeoutError):
        raise GatewayError("upstream_timeout", f"{request.host} did not answer in time", retryable=True) from None
    except (OSError, ValueError, http.client.HTTPException) as error:
        raise GatewayError("upstream_unreachable", f"could not reach {request.host}: {error!r}", retryable=True) from None


def read_limited(response: Any, limit: int, deadline: float, timeout: int) -> bytes:
    """Read up to `limit` bytes, giving up at the call's deadline."""
    chunks: list[bytes] = []
    size = 0
    try:
        while size < limit:
            # Each read is bounded by the socket timeout set at open; the
            # deadline is checked between reads.
            _remaining(deadline, timeout)
            chunk = response.read(min(65536, limit - size))
            if not chunk:
                break
            chunks.append(chunk)
            size += len(chunk)
        return b"".join(chunks)
    except (socket.timeout, TimeoutError):
        raise GatewayError("upstream_timeout", f"the response did not finish in {timeout}s", retryable=True) from None
    except (OSError, http.client.HTTPException) as error:
        raise GatewayError("upstream_unreachable", f"the response was cut off: {error!r}", retryable=True) from None


# ---------------------------------------------------------------------------
# Response
# ---------------------------------------------------------------------------


def returned_headers(headers: Message) -> dict[str, str]:
    out: dict[str, str] = {}
    for name in RETURNED_HEADERS:
        values = headers.get_all(name)
        if values:
            out[name] = ", ".join(str(value) for value in values)
    return out


def _kind(content_type: str | None) -> str:
    if not content_type:
        return "unknown"
    mime = content_type.split(";", 1)[0].strip().lower()
    if mime == "application/json" or mime.endswith("+json"):
        return "json"
    if mime.startswith("text/") or mime in TEXT_TYPES or mime.endswith("+xml"):
        return "text"
    return "binary"


def _charset(headers: Message) -> str:
    charset = headers.get_content_charset() or "utf-8"
    try:
        "".encode(charset)
    except LookupError:
        return "utf-8"
    return charset


def shape_body(data: bytes, headers: Message, truncated: bool, redactor: Redactor) -> tuple[dict[str, Any], str | None]:
    """The body as returned, and the SHA-256 of what is returned (redacted)."""
    if not data:
        return {"kind": "none"}, None
    kind = _kind(headers.get("Content-Type"))
    if kind == "unknown":
        try:
            data.decode("utf-8")
            kind = "text"
        except UnicodeDecodeError:
            kind = "binary"
    if kind == "binary":
        digest = hashlib.sha256(data).hexdigest()
        return {"kind": "binary", "size": len(data), "sha256": digest}, digest
    text = data.decode(_charset(headers), errors="replace")
    if kind == "json" and not truncated:
        try:
            value = redactor.value(json.loads(text))
        except (ValueError, RecursionError):
            pass
        else:
            canonical = json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
            return {"kind": "json", "value": value}, hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    redacted = redactor.text(text)
    return {"kind": "text", "text": redacted}, hashlib.sha256(redacted.encode("utf-8")).hexdigest()
