"""The writer inventory (STD-05@1 §R13): every place the gateway writes text is
listed in `redact.WRITERS`, and each listed writer masks a secret.

The scan below is a structural guard over the package source: it fails when a
write call appears outside the listed writers, so a new writer cannot bypass
the redactor unnoticed. The behaviour tests then drive each kind of writer
(results, errors, audit, state files) through the real backend.
"""

from __future__ import annotations

import ast
import base64
import urllib.parse
from pathlib import Path

from tests.support import GATEWAY, TOKEN, GatewayCase, Reply, Server, json_reply

from orbit_gateway import redact

SOURCES = sorted((GATEWAY / "src" / "orbit_gateway").glob("*.py")) + sorted(
    GATEWAY.glob("plugins/*/.orbit-plugin/bin/*.py")
)
# Calls that put bytes or text somewhere outside the process.
WRITE_CALLS = {"write", "writelines", "write_text", "write_bytes", "dump", "print", "writestr", "sendall", "send"}


def writers_in(path: Path) -> set[str]:
    """`<module>.<function>` for every function in `path` that makes a write call."""
    tree = ast.parse(path.read_text(encoding="utf-8"))
    module = path.stem
    found: set[str] = set()

    def visit(node: ast.AST, function: str | None) -> None:
        for child in ast.iter_child_nodes(node):
            name = function
            if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef)):
                name = child.name if function is None else function
            if isinstance(child, ast.Call):
                target = child.func
                called = target.attr if isinstance(target, ast.Attribute) else getattr(target, "id", None)
                if called in WRITE_CALLS:
                    found.add(f"{module}.{function or '<module>'}")
            visit(child, name)

    visit(tree, None)
    return found


class Inventory(GatewayCase):
    def test_every_write_is_an_inventoried_writer(self) -> None:
        found: set[str] = set()
        for path in SOURCES:
            found |= writers_in(path)
        listed = set(redact.WRITERS) | set(redact.CREDENTIAL_WRITERS) | set(redact.UNREDACTED_WRITERS)
        self.assertEqual(
            found,
            listed,
            "a write outside redact.WRITERS bypasses the redactor (STD-05 R13); "
            "route it through a listed writer or list it",
        )

    def test_the_scan_sees_a_new_writer(self) -> None:
        planted = self.root / "planted.py"
        planted.write_text("def leak(stream, text):\n    stream.write(text)\n")
        self.assertEqual(writers_in(planted), {"planted.leak"})


class EachWriterRedacts(GatewayCase):
    """Drive each writer with the slot value and look for it everywhere."""

    def forms(self) -> list[str]:
        return [
            TOKEN,
            urllib.parse.quote(TOKEN, safe=""),
            base64.b64encode(TOKEN.encode()).decode().rstrip("="),
        ]

    def assert_absent(self, data: str | bytes, where: str) -> None:
        text = data.decode("utf-8", "replace") if isinstance(data, bytes) else data
        for form in self.forms():
            self.assertNotIn(form, text, f"{where} holds the secret")

    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.put("acme", self.server.origin + "/v2/")
        self.secrets = {"conn_01": TOKEN}

    def test_results(self) -> None:
        encoded = base64.b64encode(TOKEN.encode()).decode()
        self.server.routes["/v2/echo.json"] = json_reply(
            {"you_sent": f"Bearer {TOKEN}", TOKEN: "as a key", "b64": encoded},
            Link=f"<{self.server.origin}/v2/next?token={TOKEN}>",
        )
        self.server.routes["/v2/echo.txt"] = Reply(200, f"token={urllib.parse.quote(TOKEN)}".encode(), {"Content-Type": "text/plain"})
        out = self.ok("http-get", {"connection": "acme", "path": "echo.json"}, secrets=self.secrets)
        self.assertEqual(out["body"]["value"]["you_sent"], "Bearer [secret]")
        self.assertIn("[secret]", out["body"]["value"])
        self.assertIn("[secret]", out["headers"]["link"])
        self.assert_absent(self.last_stdout, "the result")
        out = self.ok("http-get", {"connection": "acme", "path": "echo.txt"}, secrets=self.secrets)
        self.assertEqual(out["body"]["text"], "token=[secret]")
        self.assert_absent(self.last_stdout, "the result")

    def test_errors(self) -> None:
        error = self.refused("header_not_allowed", "http-get", {"connection": "acme", "headers": {TOKEN: "x"}}, secrets=self.secrets)
        self.assertIn("[secret]", error["message"])
        self.assert_absent(self.last_stdout, "the error")
        error = self.refused("unknown_connection", "http-get", {"connection": TOKEN}, secrets=self.secrets)
        self.assert_absent(self.last_stdout, "the error")

    def test_audit(self) -> None:
        self.server.routes["/v2/users"] = json_reply([])
        self.ok("http-get", {"connection": "acme", "path": "users", "query": {"q": TOKEN}}, secrets=self.secrets)
        self.refused("path_outside_connection", "http-get", {"connection": "acme", "path": f"../{TOKEN}"}, secrets=self.secrets)
        lines = self.audit_lines()
        self.assertEqual(len(lines), 2)
        self.assertEqual(lines[0]["query"], {"q": "[secret]"})
        self.assert_absent(self.state_bytes(), "the audit")
        listed = self.ok("audit-list", {}, secrets=self.secrets)
        self.assertEqual(len(listed["records"]), 2)
        self.assert_absent(self.last_stdout, "audit-list")

    def test_state_files(self) -> None:
        # A connection whose base URL carries the slot value: what is stored is redacted.
        out = self.put("leaky", f"{self.server.origin}/{TOKEN}/", scheme="none", secrets=self.secrets)
        self.assertEqual(out["connection"]["base_url"], f"{self.server.origin}/[secret]/")
        self.assertIn(b"[secret]", (self.state / "connections.json").read_bytes())
        self.assert_absent(self.state_bytes(), "connections.json")
