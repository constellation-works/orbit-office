"""The audit (one line per call, no body, listed newest first) and owner-only state."""

from __future__ import annotations

import os
import stat

from tests.support import TOKEN, GatewayCase, Reply, Server, json_reply

BODY_MARKER = "body-marker-never-audited"


class Audit(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.server.routes["/v2/users"] = json_reply({"note": BODY_MARKER})
        self.put("acme", self.server.origin + "/v2/")

    def get(self, **payload):
        return self.call("http-get", {"connection": "acme", **payload}, secrets={"conn_01": TOKEN}, context={"session_id": "S-1"})

    def test_each_call_appends_one_line_with_the_spec_fields_and_no_body(self) -> None:
        self.get(path="users", query={"page": 2})
        self.get(path="../escape")
        self.get(path="users", method="POST")
        lines = self.audit_lines()
        self.assertEqual(len(lines), 3)
        first = lines[0]
        self.assertEqual(
            set(first),
            {"time", "caller", "tool", "connection", "method", "path", "query", "status", "bytes", "duration_ms", "body_sha256", "redirects", "outcome"},
        )
        self.assertEqual(
            first["caller"],
            {"agent": "claude", "model": "test-model", "task_id": "T-1", "job_run_id": "run-1", "session_id": "S-1"},
        )
        self.assertEqual(
            {key: first[key] for key in ("tool", "connection", "method", "path", "query", "status", "outcome", "redirects")},
            {
                "tool": "gateway.http-get",
                "connection": "acme",
                "method": "GET",
                "path": "/v2/users",
                "query": {"page": 2},
                "status": 200,
                "outcome": "ok",
                "redirects": 0,
            },
        )
        self.assertGreater(first["bytes"], 0)
        self.assertEqual(len(first["body_sha256"]), 64)
        self.assertIsInstance(first["duration_ms"], int)
        self.assertEqual([line["outcome"] for line in lines[1:]], ["path_outside_connection", "method_not_allowed"])
        self.assertEqual([line["status"] for line in lines[1:]], [None, None])
        self.assertNotIn(BODY_MARKER.encode(), self.state_bytes())

    def test_audit_list_returns_newest_first(self) -> None:
        for page in range(4):
            self.get(path="users", query={"page": page})
        records = self.ok("audit-list", {})["records"]
        self.assertEqual([record["query"]["page"] for record in records], [3, 2, 1, 0])
        times = [record["time"] for record in records]
        self.assertEqual(times, sorted(times, reverse=True))
        limited = self.ok("audit-list", {"limit": 2})["records"]
        self.assertEqual([record["query"]["page"] for record in limited], [3, 2])

    def test_audit_list_filters_by_connection_and_skips_damaged_lines(self) -> None:
        self.get(path="users")
        self.call("http-get", {"connection": "other", "path": "users"})
        [month] = (self.state / "audit").glob("*.jsonl")
        with month.open("a") as handle:
            handle.write("not json\n")
        out = self.ok("audit-list", {"connection": "acme"})
        self.assertEqual([record["connection"] for record in out["records"]], ["acme"])
        self.assertEqual(out["skipped"], 1)

    def test_audit_list_on_a_fresh_install_is_empty(self) -> None:
        fresh = self.root / "fresh"
        self.state = fresh
        self.assertEqual(self.ok("audit-list", {}), {"records": [], "skipped": 0})


class OwnerOnly(GatewayCase):
    def test_state_files_and_directories_are_owner_only_whatever_the_umask(self) -> None:
        server = Server(self, {"/x": Reply(200, b"ok", {"Content-Type": "text/plain"})})
        self.call("connection-put", {"name": "acme", "base_url": server.origin, "auth": {"scheme": "none"}}, umask=0o000)
        self.call("http-get", {"connection": "acme", "path": "x"}, umask=0o000)
        paths = [self.state, *self.state.rglob("*")]
        self.assertGreaterEqual(len(paths), 4, paths)
        for path in paths:
            mode = stat.S_IMODE(os.lstat(path).st_mode)
            with self.subTest(path=path.name):
                self.assertEqual(mode, 0o700 if path.is_dir() else 0o600)

    def test_loose_modes_on_existing_state_are_repaired(self) -> None:
        self.put("acme", "https://api.example.com/", scheme="none")
        connections = self.state / "connections.json"
        os.chmod(connections, 0o666)
        os.chmod(self.state, 0o777)
        self.ok("connection-list", {})
        self.assertEqual(stat.S_IMODE(os.lstat(connections).st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(os.lstat(self.state).st_mode), 0o700)

    def test_a_symlinked_state_file_is_refused(self) -> None:
        self.state.mkdir(mode=0o700)
        elsewhere = self.root / "elsewhere.json"
        elsewhere.write_text('{"schema_version": 1, "connections": []}')
        (self.state / "connections.json").symlink_to(elsewhere)
        self.refused("state_untrusted", "connection-list", {})
        self.refused("state_untrusted", "connection-put", {"name": "acme", "base_url": "https://api.example.com/", "auth": {"scheme": "none"}})
        self.assertEqual(elsewhere.read_text(), '{"schema_version": 1, "connections": []}')

    def test_a_symlinked_audit_directory_is_refused(self) -> None:
        self.put("acme", "https://api.example.com/", scheme="none")
        target = self.root / "audit-elsewhere"
        target.mkdir()
        (self.state / "audit").symlink_to(target)
        self.refused("state_untrusted", "audit-list", {})
        self.assertEqual(list(target.iterdir()), [])
