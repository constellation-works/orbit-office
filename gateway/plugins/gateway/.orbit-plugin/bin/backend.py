#!/usr/bin/env python3
"""Orbit `exec` backend for the `gateway` plugin (envelope v1).

Orbit writes one request on stdin and reads one answer on stdout
(src/orbit_gateway/envelope.py). The shared package ships beside this file in
the plugin's `lib/` (`make -C gateway export` copies it there).

This file must stay parseable by old interpreters, so the version check can
answer `python_too_old` instead of a syntax error: keep it free of syntax
newer than Python 3.7 and import nothing from `lib/` before the check.
"""

import json
import os
import sys

REQUIRED = (3, 12)
FIX = (
    "Re-run the orbit-office install, which puts its pinned Python "
    "(~/.local/share/orbit-office/tools/bin/python3) first on PATH, then run "
    "`orbit plugin enable gateway` again so Orbit records that interpreter."
)


def _refuse_old_python():
    # Drain the request so Orbit reads this answer rather than EPIPE; it is never echoed.
    try:
        sys.stdin.read()
    except Exception:  # noqa: BLE001 - the answer below is all that matters
        pass
    found = ".".join(str(part) for part in sys.version_info[:3])
    answer = {
        "ok": False,
        "error": {
            "code": "python_too_old",
            "message": "the gateway plugin needs Python %d.%d or newer; %s is %s. %s"
            % (REQUIRED[0], REQUIRED[1], sys.executable or "python3", found, FIX),
            "retryable": False,
        },
    }
    sys.stdout.write(json.dumps(answer) + "\n")
    sys.stdout.flush()
    return 0


def main():
    if tuple(sys.version_info[:2]) < REQUIRED:
        return _refuse_old_python()
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.realpath(__file__))), "lib"))
    from orbit_gateway import audit, connections, drafts, envelope, http, oauth

    handlers = {
        "http-get": http.tool_get,
        "http-draft": drafts.tool_draft,
        "http-send": drafts.tool_send,
        "connection-list": connections.tool_list,
        "connection-put": connections.tool_put,
        "oauth-device-start": oauth.tool_start,
        "oauth-device-poll": oauth.tool_poll,
        "audit-list": audit.tool_list,
    }
    envelope.serve(sys.stdin.read(), os.environ, handlers, sys.stdout)
    return 0


if __name__ == "__main__":
    sys.exit(main())
