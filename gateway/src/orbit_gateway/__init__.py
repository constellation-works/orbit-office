"""Shared package for the gateway plugins (docs/design/office/specs/gateway.md).

Python 3.12 standard library only. Only `http`, `drafts` and `oauth` open
network connections, and only `cloud` (the cloud CLIs) and `callbacks` (Orbit's
`orbit tool run`) spawn a process; `make -C gateway check` enforces both with
an import scan.
"""

__version__ = "0.1.0"
