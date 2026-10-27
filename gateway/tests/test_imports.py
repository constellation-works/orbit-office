"""The import-direction scan (scripts/check_imports.py) that `make check` runs."""

from __future__ import annotations

import shutil
import unittest
from pathlib import Path

from tests.support import GATEWAY, GatewayCase

import check_imports  # noqa: E402  (scripts/check_imports.py)


class Scan(GatewayCase):
    def copy(self) -> Path:
        tree = self.root / "gateway"
        shutil.copytree(GATEWAY / "src", tree / "src", ignore=shutil.ignore_patterns("__pycache__"))
        shutil.copytree(GATEWAY / "plugins", tree / "plugins", ignore=shutil.ignore_patterns("__pycache__"))
        return tree

    def plant(self, path: Path, line: str) -> None:
        path.write_text(path.read_text() + f"\n{line}\n")

    def test_the_tree_is_clean(self) -> None:
        self.assertEqual(check_imports.scan(), [])

    def test_a_third_party_import_is_reported(self) -> None:
        tree = self.copy()
        self.plant(tree / "src/orbit_gateway/redact.py", "import requests")
        self.plant(tree / "plugins/gateway/.orbit-plugin/bin/backend.py", "from yaml import safe_load")
        errors = check_imports.scan(tree)
        self.assertEqual(len(errors), 2, errors)
        self.assertTrue(all("not in the standard library" in error for error in errors))

    def test_network_and_process_imports_are_confined(self) -> None:
        tree = self.copy()
        self.plant(tree / "src/orbit_gateway/audit.py", "import urllib.request")
        self.plant(tree / "src/orbit_gateway/state.py", "import subprocess")
        self.plant(tree / "src/orbit_gateway/http.py", "import subprocess")
        errors = check_imports.scan(tree)
        self.assertEqual(len(errors), 3, errors)

    def test_the_gateway_backend_never_reaches_cloud(self) -> None:
        tree = self.copy()
        (tree / "src/orbit_gateway/cloud.py").write_text("import subprocess\n")
        self.assertEqual(check_imports.scan(tree), [])
        # Reached indirectly, through another module of the package.
        self.plant(tree / "src/orbit_gateway/audit.py", "from . import cloud")
        errors = check_imports.scan(tree)
        self.assertEqual(errors, ["plugins/gateway/.orbit-plugin/bin/backend.py: the gateway backend reaches orbit_gateway.cloud"])

    def test_the_cloudcli_backend_never_reaches_http(self) -> None:
        tree = self.copy()
        backend = tree / "plugins/cloudcli/.orbit-plugin/bin/backend.py"
        backend.parent.mkdir(parents=True, exist_ok=True)
        backend.write_text("from orbit_gateway import envelope, redact\n")
        self.assertEqual(check_imports.scan(tree), [])
        backend.write_text("from orbit_gateway import envelope, connections\n")
        # connections is fine; envelope is fine; neither reaches http.
        self.assertEqual(check_imports.scan(tree), [])
        backend.write_text("from orbit_gateway import http\n")
        errors = check_imports.scan(tree)
        self.assertIn("plugins/cloudcli/.orbit-plugin/bin/backend.py: the cloudcli backend reaches orbit_gateway.http", errors)

    def test_the_cloudcli_backend_never_reaches_http_through_cloud(self) -> None:
        tree = self.copy()
        self.assertEqual(check_imports.scan(tree), [])
        self.plant(tree / "src/orbit_gateway/cloud.py", "from . import drafts")
        errors = check_imports.scan(tree)
        backend = "plugins/cloudcli/.orbit-plugin/bin/backend.py"
        self.assertIn(f"{backend}: the cloudcli backend reaches orbit_gateway.drafts", errors)
        self.assertIn(f"{backend}: the cloudcli backend reaches orbit_gateway.http", errors)


if __name__ == "__main__":
    unittest.main()
