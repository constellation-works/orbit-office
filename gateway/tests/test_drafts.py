"""gateway.http-draft and gateway.http-send through the real backend, with a
fake `orbit` for the callbacks and loopback servers that count what arrives."""

from __future__ import annotations

import base64
import hashlib
import json
import os
import re
import stat
import urllib.parse

from tests.support import TOKEN, CutoffServer, GatewayCase, Reply, Server, SilentServer, json_reply

DRAFT_ID = re.compile(r"^gd-[0-9a-f]{16}$")


def request_sha256(request: dict) -> str:
    """The digest as the spec defines it: SHA-256 of method, URL, headers and body."""
    exact = {key: request[key] for key in ("method", "url", "headers", "body")}
    return hashlib.sha256(json.dumps(exact, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


# The standard library has no JSON Schema validator, so this checks the keywords
# the plugin's published schemas use. A schema that adds a keyword it does not
# check is refused, never passed through unchecked.
CHECKED_KEYWORDS = {"type", "required", "properties", "additionalProperties", "items", "minimum"}
ANNOTATIONS = {"$schema", "title", "description"}
JSON_TYPES = {
    "array": lambda value: isinstance(value, list),
    "boolean": lambda value: isinstance(value, bool),
    "integer": lambda value: isinstance(value, int) and not isinstance(value, bool),
    "null": lambda value: value is None,
    "object": lambda value: isinstance(value, dict),
    "string": lambda value: isinstance(value, str),
}


def schema_errors(value: object, schema: dict, where: str = "output") -> list[str]:
    """Why `value` does not conform to `schema`; empty when it does."""
    unknown = set(schema) - CHECKED_KEYWORDS - ANNOTATIONS
    if unknown or not isinstance(schema.get("additionalProperties", True), bool):
        raise ValueError(f"{where}: schema uses a keyword or value this checker does not implement: {sorted(unknown)}")
    if "type" in schema:
        kinds = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        if not any(JSON_TYPES[kind](value) for kind in kinds):
            return [f"{where}: expected {' or '.join(kinds)}, got {value!r}"]
    errors: list[str] = []
    if "minimum" in schema and isinstance(value, int) and value < schema["minimum"]:
        errors.append(f"{where}: {value} is below {schema['minimum']}")
    if isinstance(value, dict):
        properties = schema.get("properties", {})
        for name in schema.get("required", []):
            if name not in value:
                errors.append(f"{where}: missing {name!r}")
        for name, item in value.items():
            if name in properties:
                errors += schema_errors(item, properties[name], f"{where}.{name}")
            elif schema.get("additionalProperties", True) is False:
                errors.append(f"{where}: unexpected {name!r}")
    if isinstance(value, list) and "items" in schema:
        for index, item in enumerate(value):
            errors += schema_errors(item, schema["items"], f"{where}[{index}]")
    return errors


class DraftCase(GatewayCase):
    def setUp(self) -> None:
        super().setUp()
        self.server = Server(self)
        self.server.routes["/v2/deals/42"] = json_reply({"id": 42, "stage": "won"})
        self.put("acme", self.server.origin + "/v2/", drafts=["POST", "PATCH"], idempotency_header="Idempotency-Key")
        self.secrets = {"conn_01": TOKEN}

    def draft(self, **payload) -> dict:
        body = {"connection": "acme", "method": "PATCH", "path": "deals/42", "body": {"stage": "won"}, **payload}
        return self.ok("http-draft", body, secrets=self.secrets)

    def send(self, out: dict, sha256: str | None = None) -> dict:
        return self.call("http-send", {"draft": out["draft"], "sha256": sha256 or out["sha256"]}, secrets=self.secrets)

    def stored(self, draft_id: str) -> dict:
        return json.loads((self.state / "drafts" / f"{draft_id}.json").read_text())

    def draft_files(self) -> list[str]:
        directory = self.state / "drafts"
        return sorted(path.name for path in directory.glob("*.json")) if directory.exists() else []


class Draft(DraftCase):
    def test_a_method_outside_the_connections_drafts_is_refused(self) -> None:
        for method in ("PUT", "DELETE", "GET", "TRACE"):
            with self.subTest(method=method):
                self.refused("method_not_allowed", "http-draft", {"connection": "acme", "method": method, "path": "deals/42"}, secrets=self.secrets)
        self.assertEqual(self.orbit.calls(), [], "a refused draft files no task")
        self.assertEqual(self.draft_files(), [])
        self.assertEqual(self.server.seen, [])

    def test_a_body_holding_a_slot_value_is_refused_as_secret_in_body(self) -> None:
        for body in (
            {"note": TOKEN},
            f"token={TOKEN}",
            {"nested": [{"key": urllib.parse.quote(TOKEN, safe="")}]},
            {"b64": base64.b64encode(TOKEN.encode()).decode()},
            {"leak": "ghp_" + "a1" * 15},  # a credential shape the gateway does not know
        ):
            with self.subTest(body=body):
                error = self.refused("secret_in_body", "http-draft", {"connection": "acme", "method": "PATCH", "path": "deals/42", "body": body}, secrets=self.secrets)
                self.assertNotIn(TOKEN, self.last_stdout)
                self.assertNotIn(TOKEN, error["message"])
        self.refused("secret_in_request", "http-draft", {"connection": "acme", "method": "PATCH", "path": f"deals/{TOKEN}"}, secrets=self.secrets)
        self.assertEqual(self.orbit.calls(), [])
        self.assertEqual(self.draft_files(), [])
        self.assertNotIn(TOKEN.encode(), self.state_bytes())

    def test_draft_input_is_checked_like_http_get(self) -> None:
        self.refused("unknown_connection", "http-draft", {"connection": "nobody", "method": "POST"})
        self.refused("path_outside_connection", "http-draft", {"connection": "acme", "method": "POST", "path": "../admin"})
        self.refused("header_not_allowed", "http-draft", {"connection": "acme", "method": "POST", "headers": {"Authorization": "x"}})
        self.refused("header_not_allowed", "http-draft", {"connection": "acme", "method": "POST", "headers": {"Idempotency-Key": "x"}})
        self.refused("invalid_argument", "http-draft", {"connection": "acme", "method": "POST", "status": "sent"})
        self.assertEqual(self.orbit.calls(), [])

    def test_a_valid_draft_stores_the_request_and_its_sha256_and_files_one_task(self) -> None:
        out = self.draft(headers={"If-Match": '"v3"'}, query={"notify": False})
        self.assertRegex(out["draft"], DRAFT_ID)
        self.assertEqual((out["state"], out["task_id"], out["artifact"]), ("pending", "T-100", "gateway-draft.json"))
        request = {
            "method": "PATCH",
            "url": f"{self.server.origin}/v2/deals/42?notify=false",
            "headers": {"If-Match": '"v3"', "Content-Type": "application/json"},
            "body": '{"stage":"won"}',
        }
        self.assertEqual(out["request"], request)
        self.assertEqual(out["sha256"], request_sha256(request))

        # The draft file: the exact request, its digest and its task, owner-only.
        path = self.state / "drafts" / f"{out['draft']}.json"
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)
        self.assertEqual(stat.S_IMODE(path.parent.stat().st_mode), 0o700)
        stored = self.stored(out["draft"])
        self.assertEqual((stored["state"], stored["request"], stored["sha256"], stored["task_id"]), ("pending", request, out["sha256"], "T-100"))

        # Exactly one gateway-draft task, then its artifact; nothing else.
        self.assertEqual([call["tool"] for call in self.orbit.calls()], ["orbit.task.add", "orbit.task.artifact.put"])
        [add] = self.orbit.calls("orbit.task.add")
        self.assertEqual(add["argv"], ["tool", "run", "orbit.task.add", "--json", "--full", "--input"])
        self.assertIn("gateway-draft", add["input"]["tags"])
        self.assertIn("proposal", add["input"]["tags"])
        self.assertEqual(add["input"]["workspace"], str(self.root))
        description = add["input"]["description"]
        for words in (
            "Nothing has been sent",
            f"**PATCH** `{request['url']}`",
            "**acme**",
            "`If-Match: \"v3\"`",
            '"stage": "won"',
            out["draft"],
            out["sha256"],
            "Idempotency-Key",
            "drafted by claude (test-model) for task T-1",
        ):
            self.assertIn(words, description)
        [put] = self.orbit.calls("orbit.task.artifact.put")
        self.assertEqual((put["input"]["id"], put["input"]["path"]), ("T-100", "gateway-draft.json"))
        self.assertTrue(put["input"]["source_path"].startswith(str(self.root) + os.sep), "the artifact is staged in the workspace")
        self.assertEqual(json.loads(put["artifact"])["request"], request)
        self.assertEqual(json.loads(put["artifact"])["sha256"], out["sha256"])
        self.assertEqual(put["artifact_mode"], 0o600)
        self.assertFalse(os.path.exists(put["input"]["source_path"]), "the staged artifact is removed")

        # The callback environment is the allowlist plus the descriptor marker.
        env = add["env"]
        self.assertEqual(env["ORBIT_PLUGIN_CALLBACK"], "descriptor-required")
        self.assertEqual(env["ORBIT_PLUGIN"], "gateway")
        self.assertNotIn("GATEWAY_TEST_AMBIENT", env)
        self.assertNotIn("ORBIT_PLUGIN_STATE", env)
        self.assertEqual(add["cwd"], str(self.root))

        [line] = self.audit_lines()
        self.assertEqual(
            {key: line[key] for key in ("tool", "connection", "method", "path", "status", "outcome", "draft")},
            {"tool": "gateway.http-draft", "connection": "acme", "method": "PATCH", "path": "/v2/deals/42", "status": None, "outcome": "drafted", "draft": out["draft"]},
        )
        self.assertEqual(self.server.seen, [], "drafting sends nothing")

    def test_a_text_body_and_no_body(self) -> None:
        text = self.draft(method="POST", path="notes", body="hello\nworld")
        self.assertEqual(text["request"]["headers"], {"Content-Type": "text/plain; charset=utf-8"})
        self.assertEqual(text["request"]["body"], "hello\nworld")
        empty = self.draft(method="POST", path="ping", body=None)
        self.assertEqual((empty["request"]["headers"], empty["request"]["body"]), ({}, None))
        self.assertIn("No body.", self.orbit.calls("orbit.task.add")[-1]["input"]["description"])

    def test_a_refused_task_add_leaves_the_draft_unsendable(self) -> None:
        self.orbit.reply("orbit.task.add", stderr={"error": "denied", "code": "policy_denied"}, exit=1)
        self.refused("orbit_policy_denied", "http-draft", {"connection": "acme", "method": "POST", "path": "deals"}, secrets=self.secrets)
        [name] = self.draft_files()
        stored = json.loads((self.state / "drafts" / name).read_text())
        self.assertEqual((stored["state"], stored["task_id"]), ("unfiled", None))
        self.refused("draft_not_pending", "http-send", {"draft": stored["id"], "sha256": stored["sha256"]}, secrets=self.secrets)
        self.assertEqual(self.server.seen, [])
        self.assertEqual([line["outcome"] for line in self.audit_lines()], ["orbit_policy_denied", "draft_not_pending"])

    def test_a_lost_task_add_reply_is_outcome_unknown_not_a_refusal(self) -> None:
        self.orbit.reply("orbit.task.add", stderr="crashed", exit=101)
        error = self.refused("orbit_outcome_unknown", "http-draft", {"connection": "acme", "method": "POST", "path": "deals"}, secrets=self.secrets)
        self.assertIn("Check Orbit", error["message"])
        self.assertEqual(len(self.orbit.calls("orbit.task.add")), 1, "a callback is never retried")

    def test_a_failed_artifact_put_still_reports_the_filed_task(self) -> None:
        self.orbit.reply("orbit.task.artifact.put", stderr={"error": "too big", "code": "invalid_input"}, exit=1)
        out = self.draft()
        self.assertEqual((out["task_id"], out["artifact"], out["artifact_error"]), ("T-100", None, "orbit_invalid_input"))
        self.assertEqual(len(self.orbit.calls("orbit.task.add")), 1)
        self.assertEqual(self.stored(out["draft"])["state"], "pending")
        self.assertEqual(list((self.root / ".orbit-gateway" / "outbox").iterdir()), [])

    def test_an_unwritable_staging_directory_still_reports_the_filed_task(self) -> None:
        outbox = self.root / ".orbit-gateway" / "outbox"
        outbox.mkdir(parents=True, mode=0o700)
        outbox.chmod(0o500)
        self.addCleanup(outbox.chmod, 0o700)
        self.assertFalse(os.access(outbox, os.W_OK), "the staging directory must be unwritable")

        out = self.draft()
        self.assertEqual((out["task_id"], out["artifact"], out["artifact_error"]), ("T-100", None, "artifact_staging_failed"))
        self.assertEqual([call["tool"] for call in self.orbit.calls()], ["orbit.task.add"])
        stored = self.stored(out["draft"])
        self.assertEqual((stored["state"], stored["task_id"]), ("pending", "T-100"))
        self.assertEqual(list(outbox.iterdir()), [])


class Send(DraftCase):
    def test_a_matching_send_goes_out_exactly_once(self) -> None:
        out = self.draft(headers={"If-Match": '"v3"'})
        answer = self.send(out)
        self.assertTrue(answer["ok"], answer)
        sent = answer["output"]
        self.assertEqual((sent["outcome"], sent["status"], sent["task_closed"]), ("sent", 200, True))
        self.assertEqual(sent["body"], {"kind": "json", "value": {"id": 42, "stage": "won"}})
        [seen] = self.server.seen
        self.assertEqual((seen.method, seen.path, seen.body), ("PATCH", "/v2/deals/42", b'{"stage":"won"}'))
        self.assertEqual(seen.headers["authorization"], f"Bearer {TOKEN}")
        self.assertEqual(seen.headers["idempotency-key"], out["draft"])
        self.assertEqual(seen.headers["if-match"], '"v3"')
        self.assertEqual(seen.headers["content-type"], "application/json")
        self.assertEqual(self.stored(out["draft"])["state"], "sent")

        # It checked the task was open (a proposed approval), then walked
        # proposed → backlog → in-progress → review → done. A direct done
        # is refused from proposed.
        self.assertEqual(self.orbit.calls("orbit.task.show")[0]["input"]["id"], "T-100")
        closes = self.orbit.calls("orbit.task.update")
        self.assertEqual([call["input"]["status"] for call in closes], ["backlog", "in-progress", "review", "done"])
        self.assertTrue(all(call["input"]["id"] == "T-100" for call in closes))
        self.assertTrue(closes[-1]["input"]["execution_summary"].startswith("Sent once:"))

        # A second send of the same draft is refused and sends nothing.
        error = self.refused("draft_not_pending", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual(error["detail"], {"state": "sent"})
        self.assertEqual(len(self.server.seen), 1)

        send_lines = [line for line in self.audit_lines() if line["tool"] == "gateway.http-send"]
        self.assertEqual([(line["outcome"], line["status"], line["draft"]) for line in send_lines], [("sent", 200, out["draft"]), ("draft_not_pending", None, out["draft"])])
        self.assertEqual(send_lines[0]["body_sha256"] is not None, True)

    def test_a_successful_send_reaches_done_when_orbit_refuses_an_illegal_close(self) -> None:
        """Orbit accepts `done` only from `review`, and that step needs an
        execution summary. Fails while the send asks for `done` from the
        open status: the callback refuses, `task_closed` is false and the
        approval task stays open."""
        starts = {
            "proposed": ["backlog", "in-progress", "review", "done"],
            "someday": ["backlog", "in-progress", "review", "done"],
            "blocked": ["backlog", "in-progress", "review", "done"],
            "backlog": ["in-progress", "review", "done"],
            "in-progress": ["review", "done"],
            "review": ["done"],
        }
        for status, expected in starts.items():
            with self.subTest(status=status):
                self.orbit.enforce_lifecycle(status=status)
                out = self.draft()
                seen = len(self.server.seen)
                logged = len(self.orbit.calls("orbit.task.update"))
                answer = self.send(out)
                self.assertTrue(answer["ok"], answer)
                sent = answer["output"]
                self.assertNotIn("task_error", sent)
                self.assertEqual(sent["task_closed"], True, sent)
                updates = self.orbit.calls("orbit.task.update")[logged:]
                self.assertEqual([call["input"].get("status") for call in updates], expected)
                done = updates[-1]["input"]
                self.assertEqual(done["id"], "T-100")
                self.assertTrue(done["execution_summary"].startswith("Sent once:"))
                self.assertEqual(done["execution_summary"], done["note"])
                self.assertIn("Sent once:", updates[0]["input"]["comment"])
                self.assertEqual(self.orbit.task_status(), "done")
                self.assertEqual(len(self.server.seen), seen + 1)
                self.refused(
                    "draft_not_pending",
                    "http-send",
                    {"draft": out["draft"], "sha256": out["sha256"]},
                    secrets=self.secrets,
                )
                self.assertEqual(len(self.server.seen), seen + 1, "a closed send is not sent again")

    def test_a_send_that_loses_the_race_to_another_sends_nothing(self) -> None:
        out = self.draft()
        # While this send checks the task, another send marks the draft sending.
        path = self.state / "drafts" / f"{out['draft']}.json"
        self.orbit.reply("orbit.task.show", stdout={"id": "T-100", "status": "proposed", "terminal": False}, mark={"path": str(path), "state": "sending"})
        error = self.refused("draft_not_pending", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual(error["detail"], {"state": "sending"})
        self.assertEqual(self.server.seen, [])

    def test_a_digest_that_no_longer_matches_is_draft_changed_and_sends_nothing(self) -> None:
        out = self.draft()
        self.refused("draft_changed", "http-send", {"draft": out["draft"], "sha256": "0" * 64}, secrets=self.secrets)
        # The stored request changed after the user saw it.
        path = self.state / "drafts" / f"{out['draft']}.json"
        document = json.loads(path.read_text())
        document["request"]["body"] = '{"stage":"lost"}'
        path.write_text(json.dumps(document))
        self.refused("draft_changed", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        # Nor does the changed request's own digest pass: the draft keeps the one it was filed with.
        self.refused("draft_changed", "http-send", {"draft": out["draft"], "sha256": request_sha256(document["request"])}, secrets=self.secrets)
        self.assertEqual(self.server.seen, [])
        self.assertEqual(self.stored(out["draft"])["state"], "pending")
        self.assertEqual(self.orbit.calls("orbit.task.update"), [])

    def test_dismissing_the_task_leaves_the_draft_unsent(self) -> None:
        out = self.draft()
        self.orbit.reply("orbit.task.show", stdout={"id": "T-100", "status": "rejected", "terminal": True})
        self.refused("draft_dismissed", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual(self.stored(out["draft"])["state"], "dismissed")
        # Even if the task were reopened, a dismissed draft is never sent.
        self.orbit.reply("orbit.task.show", stdout={"id": "T-100", "status": "proposed", "terminal": False})
        self.refused("draft_not_pending", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual(self.server.seen, [])

    def test_an_unconfirmed_task_state_sends_nothing(self) -> None:
        out = self.draft()
        self.orbit.reply("orbit.task.show", stderr="no reply", exit=1)
        self.refused("orbit_unavailable", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual(self.server.seen, [])
        self.assertEqual(self.stored(out["draft"])["state"], "pending")

    def test_a_server_that_cannot_be_reached_receives_nothing_and_the_draft_stays_pending(self) -> None:
        gone = Server(self)
        origin = gone.origin
        gone.close()
        self.put("gone", origin + "/", scheme="none", drafts=["POST"])
        out = self.ok("http-draft", {"connection": "gone", "method": "POST", "body": {}})
        self.refused("upstream_unreachable", "http-send", {"draft": out["draft"], "sha256": out["sha256"]})
        self.assertEqual(self.stored(out["draft"])["state"], "pending")
        self.assertEqual(self.orbit.calls("orbit.task.update"), [])

    def test_unknown_drafts_and_bad_input_are_refused(self) -> None:
        self.refused("unknown_draft", "http-send", {"draft": "gd-0000000000000000", "sha256": "0" * 64})
        self.refused("unknown_draft", "http-send", {"draft": "../connections", "sha256": "0" * 64})
        self.refused("invalid_argument", "http-send", {"draft": "gd-0000000000000000", "sha256": "abc"})
        self.refused("invalid_argument", "http-send", {"draft": "gd-0000000000000000", "sha256": "0" * 64, "force": True})

    def test_a_redirect_is_returned_not_followed(self) -> None:
        self.server.routes["/v2/deals/42"] = Reply(307, headers={"Location": "/v2/deals/43"})
        out = self.draft()
        sent = self.ok("http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual((sent["status"], sent["headers"]["location"]), (307, "/v2/deals/43"))
        self.assertEqual([seen.path for seen in self.server.seen], ["/v2/deals/42"])

    def test_a_secret_set_after_drafting_is_refused_at_send(self) -> None:
        out = self.ok("http-draft", {"connection": "acme", "method": "POST", "path": "deals", "body": {"code": "later-secret-0123456789"}}, secrets={})
        self.refused("secret_in_body", "http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets={"conn_01": "later-secret-0123456789"})
        self.assertEqual(self.server.seen, [])

    def test_a_reply_closed_after_the_headers_is_truncated_and_sent_once(self) -> None:
        self._assert_cutoff(b"")

    def test_a_reply_closed_mid_body_is_truncated_and_sent_once(self) -> None:
        self._assert_cutoff(b'{"id":42')

    def _assert_cutoff(self, body: bytes) -> None:
        """A Content-Length the server does not finish. Fails while a short
        read is reported as a complete body (`truncated` false)."""
        declared = 32
        cutoff = CutoffServer(self, body=body, declared=declared)
        self.put("cut", cutoff.origin + "/", scheme="none", drafts=["POST"])
        out = self.ok("http-draft", {"connection": "cut", "method": "POST", "path": "orders", "body": {"qty": 1}})
        sent = self.ok("http-send", {"draft": out["draft"], "sha256": out["sha256"]})
        self.assertEqual((sent["outcome"], sent["status"], sent["truncated"]), ("sent", 200, True))
        self.assertNotEqual((sent["body"]["kind"], sent["truncated"]), ("none", False))
        if body:
            self.assertEqual(sent["bytes"], len(body))
            self.assertEqual(sent["body"], {"kind": "text", "text": body.decode()})
        else:
            self.assertEqual((sent["body"], sent["bytes"]), ({"kind": "none"}, 0))

        [line] = [line for line in self.audit_lines() if line["tool"] == "gateway.http-send" and line["outcome"] == "sent"]
        self.assertIs(line["truncated"], True)
        self.assertNotEqual(line["bytes"], declared)
        if body:
            self.assertEqual(line["bytes"], len(body))
            self.assertEqual(len(line["body_sha256"]), 64)
        else:
            self.assertEqual((line["bytes"], line["body_sha256"]), (0, None))
        listed = self.ok("audit-list", {"connection": "cut"})
        self.assertEqual(listed["skipped"], 0)
        [row] = [row for row in listed["records"] if row["tool"] == "gateway.http-send" and row["outcome"] == "sent"]
        self.assertIs(row["truncated"], True)
        published = json.loads((self.plugin / "schemas" / "audit-list.response.json").read_text())
        self.assertEqual(schema_errors(listed, published), [], "the published schema accepts the listing")

        self.assertEqual(self.stored(out["draft"])["state"], "sent")
        self.assertEqual(len(cutoff.requests), 1)
        self.assertTrue(cutoff.requests[0].startswith(b"POST /orders HTTP/1.1"))
        self.refused("draft_not_pending", "http-send", {"draft": out["draft"], "sha256": out["sha256"]})
        self.assertEqual(len(cutoff.requests), 1, "a cut-off reply is not sent again")


class OutcomeUnknown(GatewayCase):
    def test_a_request_accepted_and_never_answered_is_outcome_unknown_and_not_retried(self) -> None:
        silent = SilentServer(self)
        self.put("slow", silent.origin + "/", scheme="none", drafts=["POST"], timeout_seconds=1)
        out = self.ok("http-draft", {"connection": "slow", "method": "POST", "path": "orders", "body": {"qty": 1}})
        answer = self.call("http-send", {"draft": out["draft"], "sha256": out["sha256"]})
        self.assertTrue(answer["ok"], f"an unknown outcome is not a failure: {answer}")
        result = answer["output"]
        self.assertEqual((result["outcome"], result["status"]), ("outcome_unknown", None))
        self.assertIn("unknown", result["message"])
        self.assertNotIn("fail", result["message"].lower())
        self.assertEqual(len(silent.requests), 1)
        self.assertTrue(silent.requests[0].startswith(b"POST /orders HTTP/1.1"))
        self.assertEqual(self.stored_state(out["draft"]), "outcome_unknown")

        # Recorded, commented on, left open; a second send is refused.
        [update] = self.orbit.calls("orbit.task.update")
        self.assertNotIn("status", update["input"])
        self.assertIn("Outcome unknown", update["input"]["comment"])
        self.refused("draft_not_pending", "http-send", {"draft": out["draft"], "sha256": out["sha256"]})
        self.assertEqual(len(silent.requests), 1, "never retried")

        [first, _] = [line for line in self.audit_lines() if line["tool"] == "gateway.http-send"]
        self.assertEqual((first["outcome"], first["status"], first["draft"]), ("outcome_unknown", None, out["draft"]))

    def stored_state(self, draft_id: str) -> str:
        return json.loads((self.state / "drafts" / f"{draft_id}.json").read_text())["state"]


class Redaction(DraftCase):
    def test_send_results_audit_and_drafts_pass_through_the_redactor(self) -> None:
        encoded = base64.b64encode(TOKEN.encode()).decode()
        self.server.routes["/v2/deals/42"] = json_reply(
            {"echo": f"Bearer {TOKEN}", "raw": TOKEN, "b64": encoded, TOKEN: "as a key"},
            Link=f"<{self.server.origin}/v2/next?token={TOKEN}>",
        )
        out = self.draft()
        sent = self.ok("http-send", {"draft": out["draft"], "sha256": out["sha256"]}, secrets=self.secrets)
        self.assertEqual(sent["body"]["value"]["echo"], "Bearer [secret]")
        self.assertEqual(sent["body"]["value"]["raw"], "[secret]")
        self.assertIn("[secret]", sent["headers"]["link"])
        for form in (TOKEN, urllib.parse.quote(TOKEN, safe=""), encoded.rstrip("=")):
            self.assertNotIn(form, self.last_stdout, "the send result")
            self.assertNotIn(form.encode(), self.state_bytes(), "drafts and audit")
            self.assertNotIn(form, self.orbit.log.read_text(), "the callbacks")
