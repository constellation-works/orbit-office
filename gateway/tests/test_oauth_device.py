"""`oauth2_device` connections through the real backend, against a loopback
fixture authorization server: the device-code sign-in, the per-call refresh
exchange, a rotated refresh token written back through `secret_updates`, and
a revoked one answered as `auth_expired`. No token may appear anywhere but
the `secret_updates` Orbit consumes."""

from __future__ import annotations

import json
import urllib.parse
from typing import Any

from tests.support import GatewayCase, Reply, Seen, Server, json_reply, schema_errors

CLIENT_ID = "client-4f1c0de2"
DEVICE_CODE = "dc-Gq81LmZ0vXr5Tn2Wb7Yk4Hs9"
USER_CODE = "WDJB-MJHT"


class AuthServer:
    """Device authorization and token endpoints, plus one protected resource."""

    def __init__(self, server: Server):
        self.server = server
        self.approved = False
        self.device_error = "authorization_pending"
        self.issued = 0
        self.valid: set[str] = set()
        self.access: set[str] = set()
        self.rotate = True
        self.forms: list[dict[str, str]] = []
        server.routes["/devicecode"] = self.device
        server.routes["/token"] = self.token
        server.routes["/v1.0/me/calendarView"] = self.resource

    def form(self, seen: Seen) -> dict[str, str]:
        form = dict(urllib.parse.parse_qsl(seen.body.decode()))
        self.forms.append(form)
        return form

    def mint(self) -> dict[str, Any]:
        self.issued += 1
        access, refresh = f"at-{self.issued}-K9vQ2mX7pL4nR8sT", f"rt-{self.issued}-H3jW6yB1cF5gD0eZ"
        self.access.add(access)
        self.valid.add(refresh)
        return {"token_type": "Bearer", "access_token": access, "refresh_token": refresh, "expires_in": 3600}

    def device(self, seen: Seen) -> Reply:
        form = self.form(seen)
        if form.get("client_id") != CLIENT_ID:
            return json_reply({"error": "invalid_client"}, 400)
        return json_reply(
            {
                "device_code": DEVICE_CODE,
                "user_code": USER_CODE,
                "verification_uri": "https://microsoft.example/devicelogin",
                "expires_in": 900,
                "interval": 5,
            }
        )

    def token(self, seen: Seen) -> Reply:
        form = self.form(seen)
        if form.get("client_id") != CLIENT_ID:
            return json_reply({"error": "invalid_client"}, 400)
        if form.get("grant_type") == "urn:ietf:params:oauth:grant-type:device_code":
            if form.get("device_code") != DEVICE_CODE:
                return json_reply({"error": "invalid_grant"}, 400)
            if not self.approved:
                return json_reply({"error": self.device_error}, 400)
            return json_reply(self.mint())
        if form.get("grant_type") == "refresh_token":
            refresh = form.get("refresh_token", "")
            if refresh not in self.valid:
                return json_reply({"error": "invalid_grant", "error_description": f"token {refresh} revoked"}, 400)
            minted = self.mint()
            if not self.rotate:
                self.valid.discard(minted["refresh_token"])
                minted.pop("refresh_token")
            else:
                self.valid.discard(refresh)
            return json_reply(minted)
        return json_reply({"error": "unsupported_grant_type"}, 400)

    def resource(self, seen: Seen) -> Reply:
        bearer = seen.headers.get("authorization", "").removeprefix("Bearer ")
        if bearer not in self.access:
            return json_reply({"error": "unauthorized"}, 401)
        # Echo the token back, as a careless API might.
        return json_reply({"value": [{"subject": "Acme QBR"}], "debug": bearer})


class DeviceCase(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.auth = AuthServer(self.server)
        origin = self.server.origin
        out = self.ok("connection-put", self.connection())
        self.assertEqual(out["connection"]["auth_state"], "signed_out")
        self.origin = origin

    def connection(self, **auth: Any) -> dict[str, Any]:
        origin = self.server.origin
        return {
            "name": "outlook",
            "base_url": origin + "/v1.0/",
            "auth": {
                "scheme": "oauth2_device",
                "token_url": origin + "/token",
                "device_url": origin + "/devicecode",
                "client_id": CLIENT_ID,
                "scopes": ["Calendars.Read", "offline_access"],
                **auth,
            },
        }

    def slot(self, value: str, version: str) -> dict[str, Any]:
        return {"secrets": {"conn_01": {"value": value, "version": version}}}

    def tokens(self) -> list[str]:
        return [DEVICE_CODE] + [
            f"{kind}-{index}-{tail}"
            for index in range(1, self.auth.issued + 1)
            for kind, tail in (("at", "K9vQ2mX7pL4nR8sT"), ("rt", "H3jW6yB1cF5gD0eZ"))
        ]

    def assert_no_token(self, data: str | bytes, where: str) -> None:
        text = data.decode("utf-8", "replace") if isinstance(data, bytes) else data
        for token in self.tokens():
            self.assertNotIn(token, text, f"{where} holds a token")

    def outside_updates(self, answer: dict[str, Any]) -> str:
        """The answer without `secret_updates`, which only Orbit reads."""
        return json.dumps({key: value for key, value in answer.items() if key != "secret_updates"})

    def sign_in(self) -> str:
        self.ok("oauth-device-start", {"connection": "outlook"})
        self.auth.approved = True
        answer = self.call("oauth-device-poll", {"connection": "outlook"})
        self.assertEqual(answer["output"]["state"], "connected")
        return answer["secret_updates"]["conn_01"]["value"]


class Put(DeviceCase):
    def test_endpoints_must_be_https_and_the_fields_complete(self) -> None:
        for auth in (
            {"token_url": "http://login.example/token"},
            {"device_url": "ftp://login.example/device"},
            {"token_url": "https://user:pw@login.example/token"},
            {"client_id": ""},
            {"client_id": "has space"},
            {"scopes": []},
            {"scopes": ["Calendars.Read offline_access"]},
            {"scopes": "Calendars.Read"},
        ):
            with self.subTest(auth=auth):
                self.refused("invalid_connection", "connection-put", self.connection(**auth))
        bearer = {"name": "other", "base_url": "https://api.example/", "auth": {"scheme": "bearer", "client_id": CLIENT_ID}}
        self.refused("invalid_connection", "connection-put", bearer)

    def test_listing_shows_the_oauth_fields_and_never_a_token(self) -> None:
        out = self.ok("connection-list", {})
        [listed] = out["connections"]
        self.assertEqual(
            listed["auth"],
            {
                "scheme": "oauth2_device",
                "token_url": self.origin + "/token",
                "device_url": self.origin + "/devicecode",
                "client_id": CLIENT_ID,
                "scopes": ["Calendars.Read", "offline_access"],
            },
        )
        self.assertEqual((listed["slot"], listed["slot_state"], listed["auth_state"]), ("conn_01", "unset", "signed_out"))
        schema = json.loads((self.plugin / "schemas" / "connection-list.response.json").read_text(encoding="utf-8"))
        self.assertEqual(schema_errors(listed, schema["properties"]["connections"]["items"]), [])

    def test_a_new_token_endpoint_or_client_releases_the_slot(self) -> None:
        same = self.ok("connection-put", self.connection(scopes=["Calendars.Read", "offline_access", "Mail.Read"]))
        self.assertEqual((same["connection"]["slot"], same["released_slot"]), ("conn_01", None))
        moved = self.ok("connection-put", self.connection(client_id="another-client"))
        self.assertEqual((moved["connection"]["slot"], moved["released_slot"]), ("conn_02", "conn_01"))


class SignIn(DeviceCase):
    def test_the_device_code_flow_stores_the_refresh_token_only_through_the_slot(self) -> None:
        start = self.call("oauth-device-start", {"connection": "outlook"})
        self.assertTrue(start["ok"], start)
        self.assertEqual(
            start["output"],
            {
                "connection": "outlook",
                "state": "pending",
                "user_code": USER_CODE,
                "verification_uri": "https://microsoft.example/devicelogin",
                "expires_in": 900,
                "interval": 5,
            },
        )
        self.assertNotIn("secret_updates", start)
        self.assert_no_token(self.last_stdout, "the start result")
        self.assertEqual(self.auth.forms[-1], {"client_id": CLIENT_ID, "scope": "Calendars.Read offline_access"})
        schema = json.loads((self.plugin / "schemas" / "oauth-device-start.response.json").read_text(encoding="utf-8"))
        self.assertEqual(schema_errors(start["output"], schema), [])

        # Not approved yet: pending, nothing stored, and no audit line per poll.
        audited = len(self.audit_lines())
        pending = self.call("oauth-device-poll", {"connection": "outlook"})
        self.assertEqual(pending["output"], {"connection": "outlook", "state": "pending", "interval": 5})
        self.assertNotIn("secret_updates", pending)
        self.assertEqual(len(self.audit_lines()), audited)
        self.auth.device_error = "slow_down"
        slowed = self.ok("oauth-device-poll", {"connection": "outlook"})
        self.assertEqual(slowed["interval"], 10)

        self.auth.approved = True
        done = self.call("oauth-device-poll", {"connection": "outlook"})
        self.assertEqual(done["output"], {"connection": "outlook", "state": "connected"})
        refresh = "rt-1-H3jW6yB1cF5gD0eZ"
        # The refresh token reaches the slot through Orbit, only while it is unset.
        self.assertEqual(done["secret_updates"], {"conn_01": {"value": refresh, "expected_version": None}})
        self.assertEqual(self.auth.forms[-1]["device_code"], DEVICE_CODE)
        self.assert_no_token(self.outside_updates(done), "the poll result")
        schema = json.loads((self.plugin / "schemas" / "oauth-device-poll.response.json").read_text(encoding="utf-8"))
        self.assertEqual(schema_errors(done["output"], schema), [])

        # The device code is gone; nothing in state, the audit or a callback holds a token.
        self.assertFalse((self.state / "oauth" / "outlook.device.json").exists())
        self.refused("device_flow_missing", "oauth-device-poll", {"connection": "outlook"})
        self.assert_no_token(self.state_bytes(), "the state")
        self.assertEqual([line["outcome"] for line in self.audit_lines()], ["pending", "connected", "device_flow_missing"])
        self.assertEqual(self.orbit.calls(), [])

        listed = self.ok("connection-list", {}, context=self.slot(refresh, "v1"))["connections"][0]
        self.assertEqual((listed["slot_state"], listed["auth_state"]), ("set", "signed_in"))

    def test_signing_in_again_replaces_only_the_version_it_was_delivered(self) -> None:
        self.ok("oauth-device-start", {"connection": "outlook"}, context=self.slot("rt-revoked-0000000000", "v7"))
        self.auth.approved = True
        done = self.call("oauth-device-poll", {"connection": "outlook"}, context=self.slot("rt-revoked-0000000000", "v7"))
        self.assertEqual(done["secret_updates"]["conn_01"]["expected_version"], "v7")

    def test_denied_and_expired_sign_ins_end_the_flow(self) -> None:
        for error, state in (("access_denied", "denied"), ("expired_token", "expired")):
            with self.subTest(error=error):
                self.ok("oauth-device-start", {"connection": "outlook"})
                self.auth.device_error = error
                answer = self.call("oauth-device-poll", {"connection": "outlook"})
                self.assertEqual(answer["output"]["state"], state)
                self.assertNotIn("secret_updates", answer)
                self.refused("device_flow_missing", "oauth-device-poll", {"connection": "outlook"})

    def test_only_oauth2_device_connections_sign_in(self) -> None:
        self.put("plain", self.server.origin + "/v2/")
        self.refused("invalid_connection", "oauth-device-start", {"connection": "plain"})
        self.refused("unknown_connection", "oauth-device-start", {"connection": "missing"})
        self.refused("invalid_argument", "oauth-device-start", {"connection": "outlook", "device_code": "x"})

    def test_a_sign_in_without_a_refresh_token_is_refused(self) -> None:
        self.ok("oauth-device-start", {"connection": "outlook"})
        self.auth.approved = True
        original = self.auth.mint
        self.auth.mint = lambda: {key: value for key, value in original().items() if key != "refresh_token"}  # type: ignore[method-assign]
        answer = self.call("oauth-device-poll", {"connection": "outlook"})
        self.assertEqual(answer["error"]["code"], "auth_failed")
        self.assertNotIn("secret_updates", answer)
        self.assert_no_token(self.last_stdout, "the refusal")


class EachCall(DeviceCase):
    def test_exchanges_per_call_and_writes_back_a_rotated_refresh_token(self) -> None:
        refresh = self.sign_in()
        answer = self.call("http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(refresh, "v1"))
        self.assertTrue(answer["ok"], answer)
        self.assertEqual(answer["output"]["body"]["value"], {"value": [{"subject": "Acme QBR"}], "debug": "[secret]"})
        exchange = self.auth.forms[-1]
        self.assertEqual(
            exchange,
            {"grant_type": "refresh_token", "refresh_token": refresh, "client_id": CLIENT_ID, "scope": "Calendars.Read offline_access"},
        )
        seen = self.server.seen[-1]
        self.assertEqual(seen.headers["authorization"], "Bearer at-2-K9vQ2mX7pL4nR8sT")
        rotated = "rt-2-H3jW6yB1cF5gD0eZ"
        self.assertEqual(answer["secret_updates"], {"conn_01": {"value": rotated, "expected_version": "v1"}})
        self.assert_no_token(self.outside_updates(answer), "the result")

        # The next call carries the rotated token; the old one is refused upstream.
        answer = self.call("http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(rotated, "v2"))
        self.assertTrue(answer["ok"], answer)
        self.assertEqual(answer["secret_updates"]["conn_01"]["expected_version"], "v2")
        self.assert_no_token(self.outside_updates(answer), "the result")
        self.assert_no_token(self.state_bytes(), "the state and audit")
        records = [line for line in self.audit_lines() if line["tool"] == "gateway.http-get"]
        self.assertEqual([(line["path"], line["status"], line["outcome"]) for line in records], [("/v1.0/me/calendarView", 200, "ok")] * 2)

    def test_a_token_that_is_not_rotated_is_not_written_back(self) -> None:
        refresh = self.sign_in()
        self.auth.rotate = False
        answer = self.call("http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(refresh, "v1"))
        self.assertTrue(answer["ok"], answer)
        self.assertNotIn("secret_updates", answer)

    def test_a_revoked_refresh_token_is_auth_expired_until_signed_in_again(self) -> None:
        refresh = self.sign_in()
        self.auth.valid.clear()  # revoked at the provider
        answer = self.call("http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(refresh, "v1"))
        self.assertEqual(answer["error"]["code"], "auth_expired")
        self.assertNotIn("secret_updates", answer)
        self.assert_no_token(self.last_stdout, "the refusal")
        self.assertEqual(self.server.seen[-1].path, "/token", "nothing reaches the API")
        self.assertEqual(self.audit_lines()[-1]["outcome"], "auth_expired")
        listed = self.ok("connection-list", {}, context=self.slot(refresh, "v1"))["connections"][0]
        self.assertEqual(listed["auth_state"], "expired")
        # Signed in again: a new version in the slot.
        self.assertEqual(self.sign_in(), "rt-2-H3jW6yB1cF5gD0eZ")
        listed = self.ok("connection-list", {}, context=self.slot("rt-2-H3jW6yB1cF5gD0eZ", "v2"))["connections"][0]
        self.assertEqual(listed["auth_state"], "signed_in")
        self.assert_no_token(self.state_bytes(), "the state and audit")

    def test_an_overlapping_call_with_the_rotated_token_is_auth_stale(self) -> None:
        refresh = self.sign_in()
        first = self.call("http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(refresh, "v1"))
        self.assertIn("secret_updates", first)
        # Delivered the same version before Orbit stored the rotation.
        late = self.call("http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(refresh, "v1"))
        self.assertEqual(late["error"]["code"], "auth_stale")
        self.assertTrue(late["error"]["retryable"])
        listed = self.ok("connection-list", {}, context=self.slot(refresh, "v1"))["connections"][0]
        self.assertEqual(listed["auth_state"], "signed_in")

    def test_not_signed_in_is_secret_unset(self) -> None:
        self.refused("secret_unset", "http-get", {"connection": "outlook", "path": "me/calendarView"})
        self.assertEqual(self.server.seen, [])

    def test_a_token_endpoint_failure_is_auth_failed_without_the_description(self) -> None:
        refresh = self.sign_in()
        self.server.routes["/token"] = json_reply({"error": "invalid_client", "error_description": f"bad {refresh}"}, 401)
        error = self.refused("auth_failed", "http-get", {"connection": "outlook", "path": "me/calendarView"}, context=self.slot(refresh, "v1"))
        self.assertEqual(error["detail"], {"status": 401, "oauth_error": "invalid_client"})
        self.assert_no_token(self.last_stdout, "the refusal")

    def test_a_draft_send_exchanges_before_sending(self) -> None:
        self.ok("connection-put", {**self.connection(), "drafts": ["PATCH"]})
        refresh = self.sign_in()
        self.server.routes["/v1.0/me/events/1"] = self.auth.resource
        draft = self.ok("http-draft", {"connection": "outlook", "method": "PATCH", "path": "me/events/1", "body": {"subject": "x"}}, context=self.slot(refresh, "v1"))
        self.auth.valid.clear()
        refused = self.call("http-send", {"draft": draft["draft"], "sha256": draft["sha256"]}, context=self.slot(refresh, "v1"))
        self.assertEqual(refused["error"]["code"], "auth_expired")
        stored = json.loads((self.state / "drafts" / f"{draft['draft']}.json").read_text())
        self.assertEqual(stored["state"], "pending", "nothing was sent")
        self.assertFalse(any(seen.path == "/v1.0/me/events/1" for seen in self.server.seen))
