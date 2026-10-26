"""Protocol dispatch: envelopes, unknown tools and unknown input keys."""

from __future__ import annotations

import json

from office.protocol import serve


def call(tool: str, tool_input: dict[str, object] | None = None) -> dict[str, object]:
    raw = json.dumps({"schema_version": 1, "tool": tool, "input": tool_input or {}, "context": {}})
    parsed: dict[str, object] = json.loads(serve(raw))
    return parsed


def test_status_ok() -> None:
    response = call("office.status")
    assert response["ok"] is True


def test_unknown_tool() -> None:
    response = call("office.nope")
    assert response["ok"] is False
    error = response["error"]
    assert isinstance(error, dict)
    assert error["code"] == "unknown_tool"


def test_wrong_namespace() -> None:
    response = call("other.status")
    assert response["ok"] is False


def test_bad_json() -> None:
    parsed: dict[str, object] = json.loads(serve("not json"))
    assert parsed["ok"] is False
