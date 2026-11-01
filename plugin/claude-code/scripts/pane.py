"""Desktop pane control entry: one JSON argument, one JSON reply.

The Claude desktop mod (mod/scribe-pane.tsx) cannot use the stdio MCP
bridge, so it runs this through `$.process` with the same structured
arguments the `scribe` MCP tool takes, plus `heartbeat`, which refreshes
the window-open heartbeat the Stop hook reads. Isolated bootstrap like
bridge.py: import only the installed plugin's sibling modules.
"""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))

import json  # noqa: E402

from process import Failure  # noqa: E402
from scribe import Scribe  # noqa: E402


def main() -> int:
    try:
        if len(sys.argv) != 2 or len(sys.argv[1]) > 65536:
            raise Failure("pane_argument_required")
        args = json.loads(sys.argv[1])
        if not isinstance(args, dict):
            raise Failure("pane_argument_must_be_object")
        client = Scribe()
        if args.get("action") == "heartbeat":
            if set(args) - {"session_id", "action"}:
                raise Failure("unknown_command_field")
            result = client.heartbeat(client.session_id(args.get("session_id")))
        else:
            client.timeout = 30.0
            result = client.command(args)
        print(json.dumps(result))
        return 0
    except Exception as error:  # fixed diagnostic codes only, like scribe.py
        message = str(error) if isinstance(error, Failure) else type(error).__name__
        print("office-scribe: " + message, file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
