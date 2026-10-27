#!/usr/bin/env python3
"""Orbit `exec` backend for the `cloudcli` plugin (envelope v1).

Orbit writes one request on stdin and reads one answer on stdout
(src/orbit_gateway/envelope.py). The shared package ships beside this file in
the plugin's `lib/` (`make -C gateway export PLUGIN=cloudcli` copies it
there). This backend reaches `cloud` and never `http` or `drafts`
(`make -C gateway check` scans for it).

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
    "`orbit plugin enable cloudcli` again so Orbit records that interpreter."
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
            "message": "the cloudcli plugin needs Python %d.%d or newer; %s is %s. %s"
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
    from orbit_gateway import cloud, envelope

    handlers = {"run": cloud.tool_run}
    envelope.serve(sys.stdin.read(), os.environ, handlers, sys.stdout, redactor=cloud.redactor_for)
    return 0


if __name__ == "__main__":
    sys.exit(main())
