"""Export a gateway plugin as an installable tree: `<out>/.orbit-plugin/` with the
shared package copied into `.orbit-plugin/lib/orbit_gateway/`.

    python3 -I scripts/export.py <plugin> <out>

The export is taken from the working tree, so it can be validated before a
commit; `<out>` must not exist yet or must be empty.
"""

from __future__ import annotations

import shutil
import sys
from pathlib import Path

GATEWAY = Path(__file__).resolve().parent.parent
IGNORE = shutil.ignore_patterns("__pycache__", "*.pyc", ".DS_Store")


def export(plugin: str, out: Path) -> Path:
    source = GATEWAY / "plugins" / plugin / ".orbit-plugin"
    if not (source / "plugin.yaml").is_file():
        raise SystemExit(f"error: no plugin named {plugin!r} under {GATEWAY / 'plugins'}")
    if out.exists() and any(out.iterdir()):
        raise SystemExit(f"error: {out} is not empty")
    root = out / ".orbit-plugin"
    shutil.copytree(source, root, ignore=IGNORE)
    shutil.copytree(GATEWAY / "src" / "orbit_gateway", root / "lib" / "orbit_gateway", ignore=IGNORE)
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print("usage: export.py <plugin> <out>", file=sys.stderr)
        return 2
    print(export(argv[1], Path(argv[2]).resolve()))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
