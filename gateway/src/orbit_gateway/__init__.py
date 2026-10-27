"""Shared package for the gateway plugins (docs/design/office/specs/gateway.md).

Python 3.12 standard library only. Only `http` and `drafts` open network
connections and only `cloud` spawns a process; `make -C gateway check`
enforces both with an import scan.
"""

__version__ = "0.1.0"
