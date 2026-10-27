"""Import-direction scan for the gateway tree (ARCHITECTURE.md "Gateway plugins").

    python3 -I scripts/check_imports.py

Fails (exit 1, one line per violation) when:
- a module imports anything outside the Python standard library and
  `orbit_gateway`;
- a module other than `http.py`, `drafts.py` or `oauth.py` imports a network module, or a
  module other than `cloud.py` (the cloud CLIs) or `callbacks.py` (Orbit's own
  `orbit tool run` callbacks) imports `subprocess`;
- the gateway backend reaches `orbit_gateway.cloud`, or the cloudcli backend
  reaches `orbit_gateway.http`, `orbit_gateway.drafts` or `orbit_gateway.oauth`,
  directly or through another module of the package.
"""

from __future__ import annotations

import ast
import sys
from pathlib import Path

GATEWAY = Path(__file__).resolve().parent.parent
PACKAGE = "orbit_gateway"
NETWORK = {"socket", "ssl", "http", "urllib.request", "ftplib", "smtplib", "imaplib", "poplib", "asyncio"}
NETWORK_MODULES = {"http", "drafts", "oauth"}
PROCESS = {"subprocess", "multiprocessing", "pty"}
PROCESS_MODULES = {"cloud", "callbacks"}
FORBIDDEN = {"gateway": {"cloud"}, "cloudcli": {"http", "drafts", "oauth"}}


def imports(path: Path) -> list[tuple[int, str]]:
    """(line, absolute module name) for every import in a file."""
    tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
    in_package = PACKAGE in path.parts
    out: list[tuple[int, str]] = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            out.extend((node.lineno, alias.name) for alias in node.names)
        elif isinstance(node, ast.ImportFrom):
            if node.level:
                if not in_package:
                    out.append((node.lineno, "." * node.level + (node.module or "")))
                    continue
                base = PACKAGE if not node.module else f"{PACKAGE}.{node.module}"
                if node.module is None:
                    out.extend((node.lineno, f"{PACKAGE}.{alias.name}") for alias in node.names)
                else:
                    out.append((node.lineno, base))
            elif node.module == PACKAGE:
                out.extend((node.lineno, f"{PACKAGE}.{alias.name}") for alias in node.names)
            else:
                out.append((node.lineno, node.module or ""))
    return out


def is_stdlib(name: str) -> bool:
    return name.split(".")[0] in sys.stdlib_module_names


def matches(name: str, prefixes: set[str]) -> bool:
    return any(name == prefix or name.startswith(prefix + ".") for prefix in prefixes)


def package_module(name: str) -> str | None:
    if name == PACKAGE:
        return "__init__"
    if name.startswith(PACKAGE + "."):
        return name.split(".")[1]
    return None


def scan(root: Path = GATEWAY) -> list[str]:
    errors: list[str] = []
    package_dir = root / "src" / PACKAGE
    graph: dict[str, set[str]] = {}
    for path in sorted(package_dir.glob("*.py")):
        module = path.stem
        graph[module] = set()
        for line, name in imports(path):
            where = f"{path.relative_to(root)}:{line}"
            target = package_module(name)
            if target is not None:
                graph[module].add(target)
            elif not is_stdlib(name):
                errors.append(f"{where}: imports {name!r}, which is not in the standard library")
            if matches(name, NETWORK) and module not in NETWORK_MODULES:
                errors.append(f"{where}: imports {name!r}; only {sorted(NETWORK_MODULES)} may open network connections")
            if matches(name, PROCESS) and module not in PROCESS_MODULES:
                errors.append(f"{where}: imports {name!r}; only {sorted(PROCESS_MODULES)} may spawn a process")
    for backend in sorted(root.glob("plugins/*/.orbit-plugin/bin/*.py")):
        plugin = backend.parts[-4]
        reached: set[str] = set()
        pending: list[str] = []
        for line, name in imports(backend):
            where = f"{backend.relative_to(root)}:{line}"
            target = package_module(name)
            if target is not None:
                pending.append(target)
            elif not is_stdlib(name):
                errors.append(f"{where}: imports {name!r}, which is not in the standard library")
            elif matches(name, NETWORK | PROCESS):
                errors.append(f"{where}: imports {name!r}; a backend reaches the network or processes only through the package")
        while pending:
            module = pending.pop()
            if module in reached:
                continue
            reached.add(module)
            pending.extend(graph.get(module, ()))
        for module in sorted(reached & FORBIDDEN.get(plugin, set())):
            errors.append(f"{backend.relative_to(root)}: the {plugin} backend reaches {PACKAGE}.{module}")
    return errors


def main() -> int:
    errors = scan()
    for error in errors:
        print(error, file=sys.stderr)
    if errors:
        return 1
    print("import scan: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
