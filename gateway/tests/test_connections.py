"""gateway.connection-put and gateway.connection-list through the real backend."""

from __future__ import annotations

import json

from tests.support import TOKEN, GatewayCase

SLOTS = [f"conn_{index:02d}" for index in range(1, 17)]


def put_input(name: str = "acme", base_url: str = "https://api.acme.example/v2", **extra) -> dict:
    return {"name": name, "base_url": base_url, "auth": {"scheme": "bearer"}, **extra}


class Put(GatewayCase):
    def test_base_url_must_be_https_or_loopback_http(self) -> None:
        for url in (
            "http://api.acme.example/",
            "http://10.0.0.5/",
            "ftp://api.acme.example/",
            "file:///etc/passwd",
            "api.acme.example/v2",
            "https://user:pass@api.acme.example/",
            "https://api.acme.example/v2?key=1",
            "https://api.acme.example/v2/../admin/",
        ):
            with self.subTest(url=url):
                self.refused("invalid_connection", "connection-put", put_input(base_url=url))
        for url, stored in (
            ("https://API.Acme.example/v2", "https://api.acme.example/v2/"),
            ("http://127.0.0.1:8080/api", "http://127.0.0.1:8080/api/"),
            ("http://localhost:9000", "http://localhost:9000/"),
            ("http://[::1]:9000/", "http://[::1]:9000/"),
        ):
            with self.subTest(url=url):
                out = self.ok("connection-put", put_input(base_url=url))
                self.assertEqual(out["connection"]["base_url"], stored)

    def test_methods_are_get_and_head_only(self) -> None:
        for methods in (["POST"], ["GET", "PATCH"], ["DELETE"], ["OPTIONS"]):
            with self.subTest(methods=methods):
                self.refused("invalid_connection", "connection-put", put_input(methods=methods))
        out = self.ok("connection-put", put_input(methods=["get", "HEAD", "GET"], drafts=["POST", "PATCH"]))
        self.assertEqual((out["connection"]["methods"], out["connection"]["drafts"]), (["GET", "HEAD"], ["POST", "PATCH"]))
        self.refused("invalid_connection", "connection-put", put_input(drafts=["GET"]))

    def test_a_secret_value_is_never_accepted(self) -> None:
        for payload in (
            put_input(secret=TOKEN),
            put_input(token=TOKEN),
            {**put_input(), "auth": {"scheme": "bearer", "secret": TOKEN}},
            {**put_input(), "auth": {"scheme": "bearer", "value": TOKEN}},
            {**put_input(), "auth": {"scheme": "bearer", "token": TOKEN}},
            {**put_input(), "auth": {"scheme": "basic", "username": "ada", "password": TOKEN}},
        ):
            with self.subTest(payload=payload):
                self.refused("invalid_argument", "connection-put", payload)
        self.assertNotIn(TOKEN.encode(), self.state_bytes() if self.state.exists() else b"")

    def test_auth_schemes(self) -> None:
        self.refused("invalid_connection", "connection-put", {**put_input(), "auth": {"scheme": "digest"}})
        self.refused("invalid_connection", "connection-put", {**put_input(), "auth": {"scheme": "header"}})
        self.refused("invalid_connection", "connection-put", {**put_input(), "auth": {"scheme": "header", "header": "Accept"}})
        self.refused("invalid_connection", "connection-put", {**put_input(), "auth": {"scheme": "basic"}})
        self.refused("invalid_connection", "connection-put", {**put_input(), "auth": {"scheme": "basic", "username": "a:b"}})
        out = self.ok("connection-put", {**put_input(), "auth": {"scheme": "none"}})
        self.assertEqual((out["connection"]["slot"], out["connection"]["slot_state"]), (None, "none"))

    def test_assigns_a_free_slot(self) -> None:
        first = self.ok("connection-put", put_input("a"))["connection"]["slot"]
        second = self.ok("connection-put", put_input("b", "https://b.example/"))["connection"]["slot"]
        self.assertEqual((first, second), ("conn_01", "conn_02"))
        # conn_03 holds a value no connection uses: never handed to a new connection.
        third = self.ok("connection-put", put_input("c", "https://c.example/"), secrets={"conn_03": TOKEN})
        self.assertEqual(third["connection"]["slot"], "conn_04")

    def test_keeps_the_slot_for_the_same_origin_and_moves_it_when_the_origin_changes(self) -> None:
        self.ok("connection-put", put_input())
        same = self.ok("connection-put", put_input(base_url="https://api.acme.example/v3"), secrets={"conn_01": TOKEN})
        self.assertEqual((same["created"], same["connection"]["slot"], same["released_slot"]), (False, "conn_01", None))
        moved = self.ok("connection-put", put_input(base_url="https://evil.example/v2"), secrets={"conn_01": TOKEN})
        self.assertEqual((moved["connection"]["slot"], moved["released_slot"]), ("conn_02", "conn_01"))
        self.assertEqual(moved["connection"]["slot_state"], "unset")

    def test_an_idempotency_header_is_kept_and_listed(self) -> None:
        out = self.ok("connection-put", put_input(drafts=["POST"], idempotency_header="Idempotency-Key"))
        self.assertEqual(out["connection"]["idempotency_header"], "Idempotency-Key")
        [listed] = self.ok("connection-list", {})["connections"]
        self.assertEqual(listed["idempotency_header"], "Idempotency-Key")
        # Without one, the field is absent.
        out = self.ok("connection-put", put_input(name="plain"))
        self.assertNotIn("idempotency_header", out["connection"])

    def test_an_idempotency_header_cannot_carry_auth_or_an_agent_header(self) -> None:
        for header in ("Authorization", "Cookie", "Content-Type", "If-Match", "Accept", "Host", "bad header"):
            with self.subTest(header=header):
                self.refused("invalid_connection", "connection-put", put_input(idempotency_header=header))
        keyed = {"name": "keyed", "base_url": "https://api.acme.example/", "auth": {"scheme": "header", "header": "X-Api-Key"}}
        self.refused("invalid_connection", "connection-put", {**keyed, "idempotency_header": "x-api-key"})

    def test_no_free_slot(self) -> None:
        for index in range(16):
            self.ok("connection-put", put_input(f"c{index}", f"https://c{index}.example/"))
        self.refused("no_free_slot", "connection-put", put_input("one-more", "https://more.example/"))
        out = self.ok("connection-put", {**put_input("no-auth", "https://open.example/"), "auth": {"scheme": "none"}})
        self.assertIsNone(out["connection"]["slot"])

    def test_names_are_checked(self) -> None:
        for name in ("Acme", "1acme", "acme_crm", "", "a" * 49):
            with self.subTest(name=name):
                self.refused("invalid_connection", "connection-put", put_input(name=name))


class List(GatewayCase):
    def test_reports_slot_set_or_unset_and_never_a_value(self) -> None:
        self.ok("connection-put", put_input("a"))
        self.ok("connection-put", put_input("b", "https://b.example/"))
        self.ok("connection-put", {**put_input("c", "https://c.example/"), "auth": {"scheme": "none"}})
        out = self.ok("connection-list", {}, secrets={"conn_01": TOKEN, "conn_09": "another-value-123456"})
        self.assertEqual(
            [(item["name"], item["slot"], item["slot_state"]) for item in out["connections"]],
            [("a", "conn_01", "set"), ("b", "conn_02", "unset"), ("c", None, "none")],
        )
        # conn_01 and conn_02 are used; conn_09 holds a value; 13 remain.
        self.assertEqual(out["free_slots"], 13)
        self.assertNotIn(TOKEN, self.last_stdout)
        self.assertNotIn("another-value-123456", self.last_stdout)
        self.assertNotIn("[secret]", json.dumps(out))

    def test_refuses_input(self) -> None:
        self.refused("invalid_argument", "connection-list", {"all": True})
