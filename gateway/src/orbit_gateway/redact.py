"""The one redactor, run at the durability boundary (STD-05@1 §R13, §R14).

Every writer that persists or returns text passes it through a `Redactor`
first. The writers are listed in `WRITERS`; a test checks that list against
the package source and drives each writer with a secret.

Two kinds of match, both replaced with `[secret]`:

- **Values.** Every slot value delivered for the call, its URL-encoded and
  base64 forms, and for `basic` auth the base64 of `user:password`.
- **Credential shapes** with high confidence only: `Bearer` and `Basic`
  header values, AWS access key ids, PEM private-key blocks and the provider
  key prefixes `ghp_`, `github_pat_`, `xox[abprs]-` and `sk-`. Field names
  are never masked because of what they are called, and no shape matches
  inside an ordinary identifier.
"""

from __future__ import annotations

import base64
import binascii
import re
import urllib.parse
from collections.abc import Iterable

MASK = "[secret]"

# Every function that writes text the gateway returns or persists, by
# `<module>.<function>`. Each one redacts before it writes. The inventory test
# (tests/test_redaction_writers.py) fails if a write happens anywhere else.
WRITERS: dict[str, str] = {
    "envelope.emit": "results and errors on stdout",
    "state.store_text": (
        "state files (connections.json, drafts/<id>.json) and the draft artifact staged for orbit.task.artifact.put"
    ),
    "state.append_line": "append-only state files (audit/<yyyy-mm>.jsonl)",
}

# Orbit callbacks (`callbacks.Orbit.call`) pass their input to `orbit tool run`
# as an argument, not through a write call; they redact it the same way.

# The one writer whose job is the secret itself: the cloud CLIs' credential
# files, owner-only, created in a fresh 0700 directory per call and deleted
# after it (STD-05@1 §R12: a private file, never argv or the environment).
CREDENTIAL_WRITERS: dict[str, str] = {
    "cloud._write_private": "the per-call credential and config files under {{plugin_state}}/calls/<call>/",
}

# Writers that never see input or a secret, so there is nothing to redact.
UNREDACTED_WRITERS: dict[str, str] = {
    "backend._refuse_old_python": (
        "the python_too_old answer: a fixed message built from the interpreter "
        "version, written before the request is read"
    ),
}

# A token character in an Authorization value (RFC 6750 b64token).
_TOKEN = r"[A-Za-z0-9\-._~+/]"
# Not preceded by an identifier character: a shape never matches inside one.
_START = r"(?<![A-Za-z0-9_])"
# Not followed by one either.
_END = r"(?![A-Za-z0-9_])"

_BEARER = re.compile(r"(?i)\b(bearer)(\s+)(" + _TOKEN + r"{16,}=*)")
_BASIC = re.compile(r"(?i)\b(basic)(\s+)([A-Za-z0-9+/]{8,}={0,2})" + _END)
_SHAPES: tuple[re.Pattern[str], ...] = (
    # AWS access key ids.
    re.compile(_START + r"(?:AKIA|ASIA)[A-Z0-9]{16}" + _END),
    # PEM private-key blocks, whatever the key type.
    re.compile(
        r"-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY-----.*?-----END (?:[A-Z0-9]+ )*PRIVATE KEY-----",
        re.DOTALL,
    ),
    # Provider key prefixes.
    re.compile(_START + r"ghp_[A-Za-z0-9]{20,}" + _END),
    re.compile(_START + r"github_pat_[A-Za-z0-9_]{20,}"),
    re.compile(_START + r"xox[abprs]-[A-Za-z0-9-]{10,}" + _END),
    re.compile(_START + r"sk-(?=[A-Za-z0-9_-]*[0-9])[A-Za-z0-9_-]{20,}" + _END),
)


def _b64_forms(raw: bytes) -> set[str]:
    standard = base64.b64encode(raw).decode("ascii")
    urlsafe = base64.urlsafe_b64encode(raw).decode("ascii")
    return {standard, standard.rstrip("="), urlsafe, urlsafe.rstrip("=")}


def value_forms(value: str) -> set[str]:
    """A secret value and the encoded forms it can travel in."""
    if not value:
        return set()
    forms = {value, urllib.parse.quote(value, safe=""), urllib.parse.quote_plus(value)}
    forms |= _b64_forms(value.encode("utf-8"))
    return {form for form in forms if form}


class Redactor:
    """Masks known secret values and high-confidence credential shapes."""

    def __init__(self, values: Iterable[str] = (), basic_pairs: Iterable[tuple[str, str]] = ()):
        forms: set[str] = set()
        for value in values:
            forms |= value_forms(value)
        for user, password in basic_pairs:
            if password:
                forms |= _b64_forms(f"{user}:{password}".encode("utf-8"))
        # Longest first, so a value inside a longer form never splits it.
        ordered = sorted(forms, key=len, reverse=True)
        self._values = re.compile("|".join(re.escape(form) for form in ordered)) if ordered else None

    def text(self, text: str) -> str:
        """Redact one string."""
        if self._values is not None:
            text = self._values.sub(MASK, text)
        text = _BEARER.sub(_mask_bearer, text)
        text = _BASIC.sub(_mask_basic, text)
        for shape in _SHAPES:
            text = shape.sub(MASK, text)
        return text

    def value(self, value: object) -> object:
        """Redact every string in a JSON value, object keys included."""
        if isinstance(value, str):
            return self.text(value)
        if isinstance(value, dict):
            out: dict[str, object] = {}
            for key, item in value.items():
                new_key = self.text(key) if isinstance(key, str) else key
                while new_key in out:
                    new_key = f"{new_key}_"
                out[new_key] = self.value(item)
            return out
        if isinstance(value, (list, tuple)):
            return [self.value(item) for item in value]
        return value


def _mask_bearer(match: re.Match[str]) -> str:
    token = match.group(3)
    # High confidence only: an opaque token carries a digit or a JWT dot.
    if not re.search(r"[0-9.]", token):
        return match.group(0)
    return f"{match.group(1)}{match.group(2)}{MASK}"


def _mask_basic(match: re.Match[str]) -> str:
    # High confidence only: the value must decode to `user:password`.
    try:
        decoded = base64.b64decode(match.group(3), validate=True).decode("utf-8")
    except (binascii.Error, ValueError):
        return match.group(0)
    if ":" not in decoded or not decoded.isprintable():
        return match.group(0)
    return f"{match.group(1)}{match.group(2)}{MASK}"
