"""Tool handlers. Each takes validated input and the call context and returns output."""

from __future__ import annotations

from pathlib import Path
from typing import Any, cast

from office import __version__

# The names `spec.secrets` declares in plugin.yaml; tests hold the two equal.
SECRET_NAMES = ("meta_api_key", "baseten_token")


def status(_input: dict[str, Any], context: dict[str, Any]) -> dict[str, Any]:
    """Version, Library presence and which declared secrets are set; never a value."""
    delivered: Any = context.get("secrets")
    secrets: dict[str, Any] = cast(dict[str, Any], delivered) if isinstance(delivered, dict) else {}
    present: set[str] = set(secrets)
    workspace = context.get("workspace_root")
    library = "absent"
    if isinstance(workspace, str) and workspace:
        library = "present" if (Path(workspace) / "library").is_dir() else "absent"
    return {
        "version": __version__,
        "library": library,
        "secrets": {name: "set" if name in present else "unset" for name in SECRET_NAMES},
    }
