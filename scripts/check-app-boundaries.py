#!/usr/bin/env python3
"""scripts/check-app-boundaries.py — import boundaries of the web app.

The TypeScript twin of check-dependency-direction.sh, from source files
alone (no build, no node_modules). The rules are in ARCHITECTURE.md
("Web app"); change them there and here in the same commit.

  * contracts imports nothing outside itself.
  * web never imports node:* or the server; the server never imports web.
  * web imports contracts with `import type` only, so the browser never
    resolves a bare specifier.
  * server/src/features/<f>/ imports only its own folder, server/src/orbit/,
    server/src/http/types.ts, node:* and @orbit-office/contracts.
  * web/src/screens/<s>/ imports only its own folder, web/src/api/,
    web/src/ui/, web/src/screen.ts and @orbit-office/contracts.
"""

from __future__ import annotations

import posixpath
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PACKAGES = "app/packages"
CONTRACTS = "@orbit-office/contracts"

# `import … from "x"`, `export … from "x"`, `import "x"`, `import("x")`.
SPECIFIER = re.compile(
    r"""(?:^|[\s;])(?:import|export)\s+(type\s+)?(?:[^'";]*?\sfrom\s+)?["']([^"']+)["']"""
    r"""|import\(\s*["']([^"']+)["']\s*\)""",
    re.MULTILINE,
)


def imports(text: str):
    """Yield (specifier, type_only) for every import in a source file."""
    # Drop line comments so commented-out imports are not checked.
    text = re.sub(r"(?m)^\s*//.*$", "", text)
    for match in SPECIFIER.finditer(text):
        spec = match.group(2) or match.group(3)
        yield spec, bool(match.group(1))


def resolve(source: str, spec: str) -> str:
    """Repo-relative POSIX path a relative specifier points at, extension dropped."""
    target = posixpath.normpath(posixpath.join(posixpath.dirname(source), spec))
    return re.sub(r"\.(js|ts|mjs|mts)$", "", target)


def inside(path: str, prefix: str) -> bool:
    return path == prefix or path.startswith(prefix.rstrip("/") + "/")


def folder_of(path: str, parent: str) -> str | None:
    """`parent/<name>` when path lies in a child folder of parent."""
    if not inside(path, parent):
        return None
    rest = path[len(parent) + 1 :]
    return f"{parent}/{rest.split('/')[0]}" if "/" in rest else None


def check(source: str, spec: str, type_only: bool) -> str | None:
    """An error message when this import crosses a boundary, else None."""
    package = source.split("/")[2]
    relative = spec.startswith(".")
    target = resolve(source, spec) if relative else None

    if package == "contracts":
        if not relative or not inside(target, f"{PACKAGES}/contracts"):
            return "contracts imports nothing outside itself"
        return None

    if package == "web":
        if spec.startswith("node:"):
            return "web runs in the browser: no node:* imports"
        if spec == CONTRACTS and not type_only:
            return "web imports contracts with `import type` only"
        if relative and not inside(target, f"{PACKAGES}/web"):
            return "web imports only its own package (and contracts as types)"
        screen = folder_of(source, f"{PACKAGES}/web/src/screens")
        if screen:
            allowed = [screen, f"{PACKAGES}/web/src/api", f"{PACKAGES}/web/src/ui",
                       f"{PACKAGES}/web/src/screen"]
            if relative and not any(inside(target, a) for a in allowed):
                return "a screen imports only its folder, api/, ui/ and screen.ts"
            if not relative and spec != CONTRACTS:
                return "a screen imports no packages except contracts"
        return None

    if package == "server":
        if relative and not inside(target, f"{PACKAGES}/server"):
            return "the server imports only its own package (and contracts)"
        feature = folder_of(source, f"{PACKAGES}/server/src/features")
        if feature:
            allowed = [feature, f"{PACKAGES}/server/src/orbit",
                       f"{PACKAGES}/server/src/http/types"]
            if relative and not any(inside(target, a) for a in allowed):
                return "a feature imports only its folder, orbit/ and http/types.ts"
            if not relative and not (spec.startswith("node:") or spec == CONTRACTS):
                return "a feature imports no packages except node:* and contracts"
        return None

    return f"unknown package {package!r}: add its rules here and in ARCHITECTURE.md"


def main() -> int:
    failures = []
    base = ROOT / PACKAGES
    if not base.is_dir():
        print("app-boundaries: no app/packages; nothing to check")
        return 0
    for path in sorted(base.glob("*/src/**/*.ts")):
        source = path.relative_to(ROOT).as_posix()
        for spec, type_only in imports(path.read_text(encoding="utf-8")):
            message = check(source, spec, type_only)
            if message:
                failures.append(f"app-boundaries: {source} imports {spec!r}: {message}")
    for line in failures:
        print(line, file=sys.stderr)
    if not failures:
        print("app-boundaries: ok")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
