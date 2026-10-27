"""`secret_path` connections through the real backend, against a loopback feed:
the slot holds the path and query, and nothing returned, audited or stored
shows them."""

from __future__ import annotations

import json
import urllib.parse

from tests.support import GatewayCase, Reply, Server, json_reply, schema_errors

FEED_TOKEN = "Zx9Qe7Lm2Vb8Nc4Rt6Yh"
FEED_ID = "a1b2c3d4e5f60718293a4b5c"
FEED_PATH = f"/owa/calendar/{FEED_ID}/{FEED_TOKEN}/calendar.ics"
SECRET = f"{FEED_PATH}?key={FEED_TOKEN}k"
ICS = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n"


class SecretPathCase(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.put("feed", self.server.origin + "/", scheme="secret_path")
        self.secrets = {"conn_01": SECRET}

    def get(self, **payload) -> dict:
        return self.call("http-get", {"connection": "feed", **payload}, secrets=self.secrets)

    def assert_hidden(self, data: str | bytes, where: str) -> None:
        text = data.decode("utf-8", "replace") if isinstance(data, bytes) else data
        for part in (SECRET, FEED_PATH, FEED_TOKEN, FEED_ID, urllib.parse.quote(SECRET, safe="")):
            self.assertNotIn(part, text, f"{where} shows the secret path")


class Put(SecretPathCase):
    def test_base_url_is_the_origin_only(self) -> None:
        for url in (self.server.origin + "/owa/", "https://outlook.example/calendar/"):
            with self.subTest(url=url):
                self.refused(
                    "invalid_connection", "connection-put",
                    {"name": "other", "base_url": url, "auth": {"scheme": "secret_path"}},
                )
        out = self.ok("connection-put", {"name": "origin", "base_url": "https://Outlook.Example", "auth": {"scheme": "secret_path"}})
        self.assertEqual(out["connection"]["base_url"], "https://outlook.example/")

    def test_a_secret_path_connection_cannot_draft_writes(self) -> None:
        self.refused(
            "invalid_connection", "connection-put",
            {"name": "other", "base_url": "https://outlook.example/", "auth": {"scheme": "secret_path"}, "drafts": ["POST"]},
        )

    def test_listed_with_its_slot_state_and_never_the_value(self) -> None:
        out = self.ok("connection-list", {}, secrets=self.secrets)
        [listed] = out["connections"]
        self.assertEqual((listed["auth"], listed["slot"], listed["slot_state"]), ({"scheme": "secret_path"}, "conn_01", "set"))
        self.assertNotIn("auth_state", listed)
        self.assert_hidden(self.last_stdout, "connection-list")
        schema = json.loads((self.plugin / "schemas" / "connection-list.response.json").read_text(encoding="utf-8"))
        self.assertEqual(schema_errors(listed, schema["properties"]["connections"]["items"]), [])

    def test_moving_to_another_scheme_releases_the_slot(self) -> None:
        out = self.put("feed", self.server.origin + "/", scheme="bearer", secrets=self.secrets)
        self.assertEqual((out["connection"]["slot"], out["released_slot"]), ("conn_02", "conn_01"))


class Fetch(SecretPathCase):
    def test_the_slot_path_and_query_are_appended_and_never_shown(self) -> None:
        # The feed echoes its own address, as some servers do in a header or body.
        self.server.routes[FEED_PATH] = Reply(
            200,
            (ICS + f"X-ORIGINAL-URL:{self.server.origin}{SECRET}\r\n").encode(),
            {"Content-Type": "text/calendar; charset=utf-8", "Link": f"<{FEED_PATH}>; rel=self"},
        )
        answer = self.get()
        self.assertTrue(answer["ok"], answer)
        out = answer["output"]
        [seen] = self.server.seen
        self.assertEqual(seen.path, SECRET)
        self.assertNotIn("authorization", seen.headers)
        self.assertEqual(out["status"], 200)
        self.assertEqual(out["url"], f"{self.server.origin}<secret_path>")
        self.assertIn("<secret_path>", out["body"]["text"])
        self.assertIn("<secret_path>", out["headers"]["link"])
        self.assert_hidden(self.last_stdout, "the result")
        [record] = self.audit_lines()
        self.assertEqual((record["path"], record["status"], record["outcome"]), ("<secret_path>", 200, "ok"))
        self.assert_hidden(self.state_bytes(), "the state")
        self.assertNotIn("secret_updates", answer)

    def test_the_agent_query_is_appended_after_the_slot_query(self) -> None:
        self.server.routes[FEED_PATH] = Reply(200, ICS.encode(), {"Content-Type": "text/calendar"})
        self.assertTrue(self.get(query={"since": "2026-10-01"})["ok"])
        self.assertEqual(self.server.seen[0].path, f"{SECRET}&since=2026-10-01")
        self.assertEqual(self.audit_lines()[0]["query"], {"since": "2026-10-01"})

    def test_a_request_path_is_refused(self) -> None:
        for path in ("calendar.ics", "/owa/", FEED_PATH):
            with self.subTest(path=path):
                error = self.refused("path_not_allowed", "http-get", {"connection": "feed", "path": path}, secrets=self.secrets)
                self.assertIn("<secret_path>", error["message"])
        self.assertEqual(self.server.seen, [])
        self.assertTrue(all(record["outcome"] == "path_not_allowed" for record in self.audit_lines()))
        self.assert_hidden(self.state_bytes(), "the audit")

    def test_an_unset_or_malformed_slot_is_refused_without_its_value(self) -> None:
        self.refused("secret_unset", "http-get", {"connection": "feed"})
        for value in ("calendar.ics?key=" + FEED_TOKEN, "//evil.example" + FEED_PATH, FEED_PATH + "#frag", FEED_PATH + " x"):
            with self.subTest(value=value):
                error = self.refused("secret_invalid", "http-get", {"connection": "feed"}, secrets={"conn_01": value})
                self.assertIn("<secret_path>", error["message"])
                self.assertNotIn(FEED_TOKEN, self.last_stdout)
        self.assertEqual(self.server.seen, [])

    def test_an_upstream_error_shows_no_secret(self) -> None:
        origin = self.server.origin
        self.server.close()
        error = self.refused("upstream_unreachable", "http-get", {"connection": "feed"}, secrets=self.secrets)
        self.assertTrue(error["retryable"])
        self.assertIn(origin.split("//", 1)[1], error["message"])
        self.assert_hidden(self.last_stdout, "the error")
        self.assertEqual(self.audit_lines()[0]["path"], "<secret_path>")

    def test_a_redirect_location_reads_secret_path(self) -> None:
        moved = f"{FEED_PATH}?key=rotated-{FEED_TOKEN}"
        self.server.routes[FEED_PATH] = Reply(302, b"", {"Location": "https://elsewhere.example" + moved})
        out = self.get()["output"]
        self.assertEqual(out["redirect"], {"location": "https://elsewhere.example<secret_path>", "reason": "other_origin"})
        self.assertEqual(out["url"], f"{self.server.origin}<secret_path>")
        self.assert_hidden(self.last_stdout, "the redirect")
        # A same-origin redirect is followed, and the URL it lands on is not shown either.
        self.server.routes[FEED_PATH] = Reply(302, b"", {"Location": "/moved/" + FEED_TOKEN + ".ics"})
        self.server.routes["/moved/" + FEED_TOKEN + ".ics"] = json_reply({"ok": True})
        out = self.get()["output"]
        self.assertEqual((out["status"], out["redirects"], out["url"]), (200, 1, f"{self.server.origin}<secret_path>"))
        self.assert_hidden(self.last_stdout, "the followed redirect")
        self.assert_hidden(self.state_bytes(), "the audit")
