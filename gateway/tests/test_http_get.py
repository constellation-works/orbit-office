"""gateway.http-get against loopback servers, through the real backend."""

from __future__ import annotations

import base64
import hashlib

from tests.support import TOKEN, GatewayCase, Reply, Server, json_reply


class ResponseShape(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.put("acme", self.server.origin + "/v2/", methods=["GET", "HEAD"])

    def get(self, path: str, **payload):
        return self.ok("http-get", {"connection": "acme", "path": path, **payload}, secrets={"conn_01": TOKEN})

    def test_json_is_parsed_with_status_content_type_and_fixed_headers(self) -> None:
        self.server.routes["/v2/users"] = json_reply(
            {"users": [{"id": 1, "name": "Ada"}]},
            ETag='"v1"',
            **{"Set-Cookie": "session=abc", "X-Internal": "kept-out", "Cache-Control": "no-store"},
        )
        out = self.get("users", query={"limit": 2, "active": True, "tag": ["a", "b"]})
        self.assertEqual(out["status"], 200)
        self.assertEqual(out["content_type"], "application/json")
        self.assertEqual(out["body"], {"kind": "json", "value": {"users": [{"id": 1, "name": "Ada"}]}})
        headers = dict(out["headers"])
        self.assertTrue(headers.pop("date"))
        self.assertEqual(
            headers,
            {"content-type": "application/json", "etag": '"v1"', "cache-control": "no-store", "content-length": "37"},
        )
        self.assertFalse(out["truncated"])
        self.assertEqual(out["redirects"], 0)
        [seen] = self.server.seen
        self.assertEqual(seen.method, "GET")
        self.assertEqual(seen.path, "/v2/users?limit=2&active=true&tag=a&tag=b")
        self.assertEqual(seen.headers["authorization"], f"Bearer {TOKEN}")

    def test_text_is_returned_as_text(self) -> None:
        self.server.routes["/v2/notes"] = Reply(200, "héllo, world".encode(), {"Content-Type": "text/plain; charset=utf-8"})
        out = self.get("notes")
        self.assertEqual(out["body"], {"kind": "text", "text": "héllo, world"})
        self.assertEqual(out["bytes"], len("héllo, world".encode()))

    def test_other_types_are_size_and_sha256_only(self) -> None:
        blob = bytes(range(256)) * 4
        self.server.routes["/v2/file.pdf"] = Reply(200, blob, {"Content-Type": "application/pdf"})
        out = self.get("file.pdf")
        self.assertEqual(out["body"], {"kind": "binary", "size": len(blob), "sha256": hashlib.sha256(blob).hexdigest()})

    def test_body_is_cut_at_max_response_bytes(self) -> None:
        self.put("small", self.server.origin + "/v2/", scheme="none", max_response_bytes=10)
        self.server.routes["/v2/long"] = Reply(200, b"abcdefghijklmnopqrstuvwxyz", {"Content-Type": "text/plain"})
        self.server.routes["/v2/long.json"] = json_reply({"items": list(range(50))})
        out = self.ok("http-get", {"connection": "small", "path": "long"})
        self.assertEqual(out["body"], {"kind": "text", "text": "abcdefghij"})
        self.assertTrue(out["truncated"])
        self.assertEqual(out["bytes"], 10)
        # Cut JSON cannot be parsed, so it comes back as the text that arrived.
        out = self.ok("http-get", {"connection": "small", "path": "long.json"})
        self.assertEqual(out["body"], {"kind": "text", "text": '{"items": '})
        self.assertTrue(out["truncated"])

    def test_body_at_exactly_the_limit_is_not_truncated(self) -> None:
        self.put("exact", self.server.origin + "/v2/", scheme="none", max_response_bytes=5)
        self.server.routes["/v2/five"] = Reply(200, b"12345", {"Content-Type": "text/plain"})
        out = self.ok("http-get", {"connection": "exact", "path": "five"})
        self.assertEqual(out["body"], {"kind": "text", "text": "12345"})
        self.assertFalse(out["truncated"])

    def test_head_returns_no_body(self) -> None:
        self.server.routes["/v2/users"] = json_reply({"users": []})
        out = self.get("users", method="HEAD")
        self.assertEqual(out["method"], "HEAD")
        self.assertEqual(out["body"], {"kind": "none"})
        self.assertEqual(self.server.seen[0].method, "HEAD")

    def test_error_statuses_are_returned_not_raised(self) -> None:
        self.server.routes["/v2/missing"] = json_reply({"error": "nope"}, status=404)
        out = self.get("missing")
        self.assertEqual(out["status"], 404)
        self.assertEqual(out["body"], {"kind": "json", "value": {"error": "nope"}})

    def test_allowed_headers_are_sent(self) -> None:
        self.server.routes["/v2/users"] = json_reply([])
        self.get("users", headers={"Accept": "application/json", "If-None-Match": '"v1"', "Range": "bytes=0-9"})
        seen = self.server.seen[0].headers
        self.assertEqual((seen["accept"], seen["if-none-match"], seen["range"]), ("application/json", '"v1"', "bytes=0-9"))

    def test_an_absolute_path_under_base_url_is_allowed(self) -> None:
        self.server.routes["/v2/users"] = json_reply([])
        self.get("/v2/users")
        self.get("./users")
        self.get("teams/../users")
        self.assertEqual([seen.path for seen in self.server.seen], ["/v2/users"] * 3)


class AuthSchemes(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self, {"/x": json_reply({})})

    def test_basic_sends_base64_user_and_password(self) -> None:
        self.put("basic", self.server.origin, scheme="basic", username="ada")
        self.ok("http-get", {"connection": "basic", "path": "x"}, secrets={"conn_01": TOKEN})
        expected = base64.b64encode(f"ada:{TOKEN}".encode()).decode()
        self.assertEqual(self.server.seen[0].headers["authorization"], f"Basic {expected}")

    def test_header_scheme_sends_the_named_header(self) -> None:
        self.put("keyed", self.server.origin, scheme="header", header="X-Api-Key")
        self.ok("http-get", {"connection": "keyed", "path": "x"}, secrets={"conn_01": TOKEN})
        self.assertEqual(self.server.seen[0].headers["x-api-key"], TOKEN)
        self.assertNotIn("authorization", self.server.seen[0].headers)

    def test_none_sends_no_credential(self) -> None:
        self.put("open", self.server.origin, scheme="none")
        self.ok("http-get", {"connection": "open", "path": "x"}, secrets={"conn_01": TOKEN})
        self.assertNotIn("authorization", self.server.seen[0].headers)

    def test_an_unset_slot_is_refused_and_sends_nothing(self) -> None:
        self.put("acme", self.server.origin)
        self.refused("secret_unset", "http-get", {"connection": "acme", "path": "x"})
        self.assertEqual(self.server.seen, [])


class Refusals(GatewayCase):
    """Every refusal happens before a request is sent."""

    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.other = Server(self)
        self.put("acme", self.server.origin + "/v2/", scheme="header", header="X-Api-Key")
        self.put("getonly", self.server.origin + "/v2/", scheme="none", methods=["GET"])

    def assert_refused(self, code: str, **payload) -> None:
        self.refused(code, "http-get", {"connection": "acme", "path": "users", **payload}, secrets={"conn_01": TOKEN})
        self.assertEqual(self.server.seen, [], f"{payload} sent a request")
        self.assertEqual(self.other.seen, [], f"{payload} sent a request elsewhere")

    def test_unknown_connection(self) -> None:
        self.assert_refused("unknown_connection", connection="nobody")

    def test_disabled_connection_is_unknown(self) -> None:
        self.put("off", self.server.origin + "/v2/", scheme="none", disabled=True)
        self.assert_refused("unknown_connection", connection="off")

    def test_method_not_allowed(self) -> None:
        for method in ("POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "CONNECT"):
            with self.subTest(method=method):
                self.assert_refused("method_not_allowed", method=method)

    def test_head_is_refused_when_the_connection_lists_only_get(self) -> None:
        self.assert_refused("method_not_allowed", connection="getonly", method="HEAD")

    def test_path_outside_connection(self) -> None:
        other = self.other.origin
        for path in (
            f"{other}/steal",  # absolute URL to another host
            f"{self.server.origin}/v2/users",  # absolute URL, even to the same host
            "//127.0.0.1:1/steal",  # network-path reference: another host
            "https:users",  # a scheme
            "../admin",  # `..` climbing out of base_url
            "users/../../admin",
            "%2e%2e/admin",  # encoded dot segment
            "users/%2E%2E/%2e%2e/admin",
            "..%2fadmin",  # encoded slash
            "/admin",  # an absolute path outside base_url
            "/v2x/users",
            "users\\..\\..\\admin",
        ):
            with self.subTest(path=path):
                self.assert_refused("path_outside_connection", path=path)

    def test_header_not_allowed(self) -> None:
        for header in ("Authorization", "authorization", "Cookie", "Proxy-Authorization", "X-Api-Key", "x-api-key", "X-Custom", "Host"):
            with self.subTest(header=header):
                self.assert_refused("header_not_allowed", headers={header: "value"})

    def test_unknown_input_keys_are_refused(self) -> None:
        self.assert_refused("invalid_argument", body="{}")


class Redirects(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.other = Server(self)
        self.put("acme", self.server.origin + "/v2/")

    def get(self, path: str):
        return self.ok("http-get", {"connection": "acme", "path": path}, secrets={"conn_01": TOKEN})

    def test_same_origin_redirects_are_followed_up_to_three(self) -> None:
        self.server.routes.update(
            {
                "/v2/a": Reply(302, headers={"Location": "/v2/b"}),
                "/v2/b": Reply(301, headers={"Location": f"{self.server.origin}/v2/c"}),
                "/v2/c": Reply(307, headers={"Location": "d"}),
                "/v2/d": json_reply({"done": True}),
            }
        )
        out = self.get("a")
        self.assertEqual((out["status"], out["redirects"], out["body"]["value"]), (200, 3, {"done": True}))
        self.assertNotIn("redirect", out)
        self.assertEqual([seen.path for seen in self.server.seen], ["/v2/a", "/v2/b", "/v2/c", "/v2/d"])
        # The credential goes to its own origin on every hop.
        self.assertTrue(all(seen.headers["authorization"] == f"Bearer {TOKEN}" for seen in self.server.seen))

    def test_a_fourth_redirect_is_returned_not_followed(self) -> None:
        self.server.routes["/v2/a"] = Reply(302, headers={"Location": "/v2/b"})
        self.server.routes["/v2/b"] = Reply(302, headers={"Location": "/v2/c"})
        self.server.routes["/v2/c"] = Reply(302, headers={"Location": "/v2/d"})
        self.server.routes["/v2/d"] = Reply(302, headers={"Location": "/v2/e"})
        out = self.get("a")
        self.assertEqual(out["status"], 302)
        self.assertEqual(out["redirects"], 3)
        self.assertEqual(out["redirect"], {"location": f"{self.server.origin}/v2/e", "reason": "limit"})
        self.assertEqual([seen.path for seen in self.server.seen], ["/v2/a", "/v2/b", "/v2/c", "/v2/d"])

    def test_another_origin_is_returned_and_receives_nothing(self) -> None:
        target = f"{self.other.origin}/collect"
        self.server.routes["/v2/a"] = Reply(302, headers={"Location": target})
        self.other.routes["/collect"] = json_reply({"stolen": True})
        out = self.get("a")
        self.assertEqual(out["status"], 302)
        self.assertEqual(out["redirect"], {"location": target, "reason": "other_origin"})
        self.assertEqual(out["headers"]["location"], target)
        self.assertEqual(self.other.seen, [], "the other origin received a request")

    def test_another_origin_after_a_followed_hop(self) -> None:
        self.server.routes["/v2/a"] = Reply(302, headers={"Location": "/v2/b"})
        self.server.routes["/v2/b"] = Reply(308, headers={"Location": f"{self.other.origin}/x"})
        out = self.get("a")
        self.assertEqual((out["redirects"], out["redirect"]["reason"]), (1, "other_origin"))
        self.assertEqual(self.other.seen, [])

    def test_same_host_on_another_port_is_another_origin(self) -> None:
        moved = self.other.origin.replace("127.0.0.1", "localhost")
        self.server.routes["/v2/a"] = Reply(302, headers={"Location": f"{moved}/x"})
        out = self.get("a")
        self.assertEqual(out["redirect"]["reason"], "other_origin")
        self.assertEqual(self.other.seen, [])


class Unreachable(GatewayCase):
    def test_a_closed_port_is_upstream_unreachable(self) -> None:
        server = Server(self)
        origin = server.origin
        server.close()
        self.put("gone", origin + "/", scheme="none")
        error = self.refused("upstream_unreachable", "http-get", {"connection": "gone"})
        self.assertTrue(error["retryable"])
