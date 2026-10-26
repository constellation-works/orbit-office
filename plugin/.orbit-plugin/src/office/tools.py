"""Tool handlers. Each takes validated input and the call context and returns output."""

from __future__ import annotations

from typing import Any, cast

from office import __version__

# The names `spec.secrets` declares in plugin.yaml; tests hold the two equal.
SECRET_NAMES = ("meta_api_key", "baseten_token")


def status(_input: dict[str, Any], context: dict[str, Any]) -> dict[str, Any]:
    """Version and which declared secrets are set; never a value.

    Filesystem and index health join this output with the `reindex` slice,
    once the Library layout exists to check.
    """
    delivered: Any = context.get("secrets")
    secrets: dict[str, Any] = cast(dict[str, Any], delivered) if isinstance(delivered, dict) else {}
    present: set[str] = set(secrets)
    return {
        "version": __version__,
        "secrets": {name: "set" if name in present else "unset" for name in SECRET_NAMES},
    }
