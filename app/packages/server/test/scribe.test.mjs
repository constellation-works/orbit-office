/**
 * The Scribe feature through its HTTP routes, on the fixture outputs
 * (scribe-fixture.mjs): the one-card-per-project feed and its `previous`
 * chain, the Preferences and Decisions panels, the Tags editor's reads
 * and saves (including Orbit refusing the save as `not_permitted`), the
 * legacy handoff list, and the ask box.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";

import { createScribeFeature } from "../dist/features/scribe/index.js";
import { configSetInput, operatorCommand, tagSaveOf } from "../dist/features/scribe/tags.js";
import { OrbitExitError } from "../dist/orbit/client.js";
import { agreement, cards, decisions, handoffs, office, preferences, scribeServer, statusTags } from "./scribe-fixture.mjs";

const get = async (url) => { const response = await fetch(url); return { status: response.status, body: await response.json() }; };
const post = async (url, body) => {
  const response = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: response.status, body: await response.json() };
};

/** A feature on a loopback server over any fake client. */
async function serve(t, client) {
  const feature = createScribeFeature(client);
  const server = createServer((req, res) => {
    const route = feature.routes.find((r) => r.method === req.method && r.path === req.url.split("?")[0]);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end("{}"); }
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise((done) => server.close(done)));
  return `http://127.0.0.1:${server.address().port}`;
}

test("the feed shows exactly one card per project plus one global card, with its sections and tag counts", async (t) => {
  const { url, calls } = await scribeServer(t);
  const { status, body } = await get(`${url}/api/scribe/feed`);
  assert.equal(status, 200);
  assert.deepEqual(calls.map((call) => [call.name, call.input]), [["office.session-brief", { view: "list" }]]);
  assert.deepEqual(body.cards.map((card) => [card.id, card.scope]), [
    ["SCR-20261029-0900-cccc01", "global"],
    ["SCR-20261031-0900-aaaa02", "project:orbit-office"],
    ["SCR-20261028-0900-dddd01", "project:secret-project"],
  ], "the older orbit-office SCR is not a card");
  assert.equal(body.count, 3);
  assert.equal(body.files_searched, 4);
  const latest = body.cards[1];
  assert.equal(latest.title, "Ship the scribe feed");
  assert.equal(latest.updated, "2026-10-31T09:00:00Z");
  assert.equal(latest.previous, "SCR-20261030-0900-aaaa01");
  assert.equal(latest.summary, "(scribe) Rework the Scribe tab into the one-card feed.");
  assert.deepEqual(latest.next, ["ORB-21120 — finish the Tags editor. owner: agent."]);
  assert.deepEqual(latest.outcome, ["The cards feed reads view list."]);
  assert.equal(latest.tag_entries, 3);
  assert.deepEqual(latest.tag_counts, { Decisions: 2, Preferences: 1 });
  assert.deepEqual(latest.tags.map((entry) => entry.citation), [
    "SCR-20261031-0900-aaaa02#T1", "SCR-20261031-0900-aaaa02#T2", "SCR-20261031-0900-aaaa02#T3",
  ]);
  const hidden = body.cards[2];
  assert.deepEqual([hidden.withheld, hidden.summary, hidden.tags, hidden.tag_counts, hidden.next], [true, null, [], {}, []]);
});

test("one SCR by id returns it with its previous; older records are reached only by walking the chain", async (t) => {
  const { url, calls } = await scribeServer(t);
  const first = await get(`${url}/api/scribe/feed?id=SCR-20261031-0900-aaaa02`);
  assert.equal(first.status, 200);
  assert.deepEqual(calls.at(-1), { name: "office.session-brief", input: { view: "list", id: "SCR-20261031-0900-aaaa02" } });
  assert.equal(first.body.scr.id, "SCR-20261031-0900-aaaa02");
  assert.equal(first.body.previous.id, "SCR-20261030-0900-aaaa01");
  const end = await get(`${url}/api/scribe/feed?id=${first.body.previous.id}`);
  assert.equal(end.body.scr.id, "SCR-20261030-0900-aaaa01");
  assert.equal(end.body.previous, null, "the chain ends; there is no other way to an older SCR");
  assert.deepEqual(await get(`${url}/api/scribe/feed?id=a%20b`), { status: 400, body: { error: "invalid_input" } });
  assert.deepEqual(await get(`${url}/api/scribe/feed?view=list`), { status: 400, body: { error: "invalid_input" } }, "an unknown parameter is refused");
});

test("Preferences and Decisions read office.scribe-query and shape its rows for reading", async (t) => {
  const { url, calls } = await scribeServer(t);
  const prefs = await get(`${url}/api/scribe/preferences`);
  assert.equal(prefs.status, 200);
  assert.deepEqual(calls.at(-1), { name: "office.scribe-query", input: { mode: "preferences" } });
  assert.deepEqual(prefs.body.preferences.map((row) => [row.citation, row.statement, row.stale]),
    preferences.preferences.map((row) => [row.citation, row.statement, row.stale]));
  assert.deepEqual(prefs.body.preferences[0].proposed, [{ entry: "SCR-20261031-0900-aaaa02#T1", proposal: "prop-1" }]);
  assert.equal(prefs.body.today, "2026-11-01");

  const ledger = await get(`${url}/api/scribe/decisions`);
  assert.equal(ledger.status, 200);
  assert.deepEqual(calls.at(-1), { name: "office.scribe-query", input: { mode: "decisions" } });
  assert.deepEqual(ledger.body.decisions.map((row) => row.id), ["dec-orbit-office-8", "dec-orbit-office-7"], "newest first");
  assert.equal(ledger.body.decisions[1].because, "the feed is unusable", "the stated reason's text");
  assert.equal(ledger.body.decisions[0].because, null, "no reason was stated");
  assert.deepEqual(ledger.body.decisions.map((row) => [row.current, row.reversed_by]), [[true, null], [true, null]]);
  assert.equal(ledger.body.count, decisions.count);
});

test("the Tags editor reads the effective tags from office.status", async (t) => {
  const { url, calls } = await scribeServer(t);
  const { status, body } = await get(`${url}/api/scribe/tags`);
  assert.equal(status, 200);
  assert.deepEqual(calls.map((call) => [call.name, call.input]), [["office.status", {}]]);
  assert.deepEqual(body.tags, statusTags);
  assert.deepEqual(body.cadence, { minutes: 20, turns: 10 });
  assert.equal(body.tags_customized, true);
});

/** A client whose config-set succeeds and whose status reflects the save. */
function savingClient() {
  const calls = [];
  let tags = structuredClone(statusTags);
  let customized = true;
  const status = () => ({ scribe: { cadence: { minutes: 20, turns: 10 }, tags: structuredClone(tags), tags_customized: customized } });
  return {
    calls,
    async callTool(name, input) {
      calls.push({ name, input: structuredClone(input) });
      if (name === "office.config-set") {
        if (input.key === "scribe.tags" && input.value === null) {
          tags = structuredClone(statusTags.slice(0, 2));
          customized = false;
        } else if (input.value === null) {
          tags = tags.filter((tag) => tag.name !== input.key.slice("scribe.tags.".length));
        } else {
          tags = [...tags.filter((tag) => tag.name !== input.value.name), input.value];
        }
        return { key: input.key };
      }
      if (name === "office.status") return status();
      throw new Error(`unexpected ${name}`);
    },
    async pollTool(name, input) { return this.callTool(name, input); },
  };
}

test("a tag save writes through office.config-set and returns the tags read back", async (t) => {
  const client = savingClient();
  const url = await serve(t, client);
  const tag = { name: "incident", scope: "project", instruction: "Something broke and what settled it.", sink: "scr_only", mode: "append", capture: "markers" };
  const saved = await post(`${url}/api/scribe/tags`, { action: "upsert", tag });
  assert.equal(saved.status, 200, JSON.stringify(saved.body));
  assert.equal(saved.body.saved, true);
  assert.deepEqual(client.calls[0], { name: "office.config-set", input: { key: "scribe.tags.incident", value: tag } });
  assert.deepEqual(client.calls[1], { name: "office.status", input: {} }, "read back after the write");
  assert.ok(saved.body.tags.tags.some((row) => row.name === "incident" && row.sink === "scr_only"));

  const removed = await post(`${url}/api/scribe/tags`, { action: "delete", name: "preferences" });
  assert.deepEqual(client.calls.at(-2).input, { key: "scribe.tags.preferences", value: null }, "a default can be deleted");
  assert.ok(!removed.body.tags.tags.some((row) => row.name === "preferences"));

  const restored = await post(`${url}/api/scribe/tags`, { action: "restore" });
  assert.deepEqual(client.calls.at(-2).input, { key: "scribe.tags", value: null });
  assert.deepEqual(restored.body.tags.tags.map((row) => row.name), ["preferences", "decisions"]);
  assert.equal(restored.body.tags.tags_customized, false);
});

test("when Orbit refuses config-set, the save is not_permitted with the operator command and claims no success", async (t) => {
  const { url, calls } = await scribeServer(t);
  const save = { action: "delete", name: "lesson" };
  const { status, body } = await post(`${url}/api/scribe/tags`, save);
  assert.equal(status, 403);
  assert.equal(body.error, "not_permitted");
  assert.equal(body.command, `orbit tool run office.config-set --input '${JSON.stringify(configSetInput(save))}'`);
  assert.match(body.command, /scribe\.tags\.lesson/);
  assert.equal(body.saved, undefined, "nothing claims the save happened");
  assert.deepEqual(calls.map((call) => call.name), ["office.config-set"], "no read-back after a refused save");

  // The envelope form of the same refusal (ok: false) names it too.
  const envelope = {
    async callTool() { return { ok: false, error: { code: "policy_denied", message: "denied" } }; },
    async pollTool() { throw new Error("unexpected"); },
  };
  const second = await serve(t, envelope);
  const again = await post(`${second}/api/scribe/tags`, { action: "restore" });
  assert.equal(again.status, 403);
  assert.deepEqual([again.body.error, again.body.command], ["not_permitted", operatorCommand({ action: "restore" })]);
});

test("an Office refusal of a tag definition keeps its own code; bad bodies are refused before any tool call", async (t) => {
  const refusing = {
    async callTool() {
      throw new OrbitExitError("office.config-set", 1, null,
        JSON.stringify({ error: { code: "invalid_library_setting", message: "scribe.tags.x.mode: sink and mode do not pair" } }));
    },
    async pollTool() { throw new Error("unexpected"); },
  };
  const url = await serve(t, refusing);
  const tag = { name: "x", scope: "global", instruction: "One tag.", sink: "decisions_ledger", mode: "apply", capture: "markers" };
  assert.deepEqual(await post(`${url}/api/scribe/tags`, { action: "upsert", tag }),
    { status: 422, body: { error: "invalid_library_setting" } });

  for (const body of [
    { action: "upsert", tag: { ...tag, sink: "polaris.md" } },
    { action: "upsert", tag: { ...tag, name: "Bad Name" } },
    { action: "upsert", tag: { ...tag, instruction: "  " } },
    { action: "upsert", tag: { ...tag, path: "/tmp/x" } },
    { action: "delete", name: "x", extra: 1 },
    { action: "restore", name: "x" },
    { action: "everything" },
  ]) {
    assert.throws(() => tagSaveOf(body), (error) => error.code === "invalid_input", JSON.stringify(body));
  }
});

test("the ask box cites SCR file:line, drops a quote not on its cited line, and says not captured with the range searched", async (t) => {
  const { url, calls } = await scribeServer(t);
  const answered = await post(`${url}/api/scribe/ask`, { question: "How often does the screen poll?" });
  assert.equal(answered.status, 200);
  assert.deepEqual(calls.at(-1), { name: "office.scribe-ask", input: { question: "How often does the screen poll?" } });
  const decision = office.why.results.find((row) => row.why !== null);
  assert.equal(answered.body.status, "answered");
  assert.deepEqual(answered.body.claims.map((claim) => [claim.text, claim.quote, claim.file_line, claim.msg]),
    [["The screen polls every ten seconds.", "poll every ten seconds", decision.file_line, "m-0003"]]);
  assert.ok(decision.text.includes(answered.body.claims[0].quote), "the kept quote is on its cited line");
  assert.equal(answered.body.dropped, 2, "one dropped by Office, one mismatched quote dropped here");
  assert.equal(answered.body.model, "hosted haiku");

  const missing = await post(`${url}/api/scribe/ask`, { question: "What did we order for lunch?" });
  assert.deepEqual([missing.body.status, missing.body.claims, missing.body.since, missing.body.until, missing.body.topics, missing.body.files_searched],
    ["not_captured", [], "2026-10-31", "2026-11-01", ["recall", "scribe"], 2]);
  assert.deepEqual((await post(`${url}/api/scribe/ask`, { question: "x", limit: 3 })).body, { error: "invalid_input" });
  assert.deepEqual((await post(`${url}/api/scribe/ask`, { question: " " })).body, { error: "question_required" });
});

function agreementClient(readout, calls = []) {
  return {
    calls,
    async callTool() { throw new Error("unexpected callTool"); },
    async pollTool(name, input) {
      calls.push({ name, input: structuredClone(input) });
      if (readout instanceof Error) throw readout;
      return structuredClone(readout);
    },
  };
}

test("decision-agreement renders the pinned readout per category, aggregates only", async (t) => {
  const withPredictions = structuredClone(agreement);
  withPredictions.categories[0].predictions = [{ request: "ORB-1", predicted: "yes", probability: 0.9 }];
  withPredictions.requests = [{ id: "ORB-1", prediction: "yes" }];
  const client = agreementClient(withPredictions);
  const url = await serve(t, client);
  const { status, body } = await get(`${url}/api/scribe/decision-agreement`);
  assert.equal(status, 200);
  assert.deepEqual(client.calls, [{ name: "orbit.decision.readout", input: { contract: "precedent-decision-readout/1" } }]);
  assert.equal(body.agreement.contract, "precedent-decision-readout/1");
  assert.equal(body.agreement.consumer_mode, "shadow");
  assert.deepEqual([body.agreement.window_start, body.agreement.window_end, body.agreement.generated_at],
    ["2026-10-01T00:00:00Z", "2026-11-01T00:00:00Z", "2026-11-01T09:00:00Z"]);
  assert.deepEqual(body.agreement.categories.map((row) => [row.category, row.mode, row.scored_requests, row.accuracy, row.accuracy_wilson_low, row.accuracy_wilson_high, row.coverage, row.calibration_error, row.abstention_rate, row.conflict_rate, row.min_samples]), [
    ["ws_orbit-office:agent_request", "shadow", 42, 0.81, 0.68, 0.9, 0.74, 0.04, 0.12, 0.05, 30],
    ["ws_orbit-office:final_recovery", "enabled", 7, 1, 0.65, 1, 0.5, 0.05, 0, 0, 30],
  ], "one row per category, including the low-sample one (the screen marks it)");
  assert.ok(!JSON.stringify(body).includes("prediction"), "no per-request prediction anywhere");
  assert.ok(!JSON.stringify(body).includes("ORB-1"), "no per-request id anywhere");
});

test("decision-agreement is hidden when the consumer is off, there is no readout, or Orbit has no readout tool", async (t) => {
  for (const readout of [{ ...agreement, consumer_mode: "off" }, { available: false }, null, {}]) {
    const url = await serve(t, agreementClient(readout));
    assert.deepEqual(await get(`${url}/api/scribe/decision-agreement`), { status: 200, body: { agreement: null } }, JSON.stringify(readout));
  }
  const missingTool = new OrbitExitError("orbit.decision.readout", 1, null,
    JSON.stringify({ error: { code: "unknown_tool", message: "unknown tool" } }));
  const url = await serve(t, agreementClient(missingTool));
  assert.deepEqual(await get(`${url}/api/scribe/decision-agreement`), { status: 200, body: { agreement: null } });
});

test("decision-agreement with no categories is an empty readout, not hidden", async (t) => {
  const url = await serve(t, agreementClient({ ...agreement, categories: [] }));
  const { status, body } = await get(`${url}/api/scribe/decision-agreement`);
  assert.equal(status, 200);
  assert.deepEqual(body.agreement.categories, []);
});

test("decision-agreement read failure is a plain agreement_unavailable error", async (t) => {
  const failing = await serve(t, agreementClient(new Error("orbit down")));
  assert.deepEqual(await get(`${failing}/api/scribe/decision-agreement`), { status: 502, body: { error: "agreement_unavailable" } });
  const malformed = await serve(t, agreementClient({ contract: "precedent-decision-readout/1", consumer_mode: "shadow", categories: [{ category: "x" }] }));
  assert.deepEqual(await get(`${malformed}/api/scribe/decision-agreement`), { status: 502, body: { error: "agreement_unavailable" } });
});

test("decision-agreement reads Orbit at most once per 60 seconds", async (t) => {
  const calls = [];
  const url = await serve(t, agreementClient(agreement, calls));
  await get(`${url}/api/scribe/decision-agreement`);
  await get(`${url}/api/scribe/decision-agreement`);
  assert.equal(calls.length, 1, "the second read is served from the 60-second cache");
});

const listed = (output, kind) => output.sections.find((section) => section.kind === kind);

test("the legacy handoffs read office.session-brief view list with legacy true and keep its order, states and anchors", async (t) => {
  const { url, calls } = await scribeServer(t);
  const { status, body } = await get(`${url}/api/scribe/handoffs`);
  assert.equal(status, 200);
  assert.deepEqual(calls.map((call) => [call.name, call.input]), [["office.session-brief", { view: "list", legacy: true, include_closed: true, limit: 50 }]]);
  const output = handoffs.all.output;
  for (const kind of ["threads", "global", "history"]) {
    assert.deepEqual(body[kind].map((item) => item.id), listed(output, kind).items.map((item) => item.id), kind);
    assert.equal(body[`${kind}_total`], listed(output, kind).total, kind);
  }
  assert.equal(body.history_offset, 0);
  assert.equal(body.history_more, false);
  const [structured] = body.threads;
  assert.equal(structured.source_id, "src-20261031-office");
  assert.equal(structured.line, 5);
  assert.equal(structured.assignment, "ORB-21075");
  assert.equal(structured.resume, "claude --resume 7f3c2a10-5b1e-4c0d-9a8e-2f6b1c4d9e01");
  assert.deepEqual(Object.keys(structured.fields), ["objective", "next", "authorization", "observations", "evidence"]);
  assert.equal(structured.legacy, false);
  const orbit = body.threads.find((item) => item.project === "orbit");
  assert.equal(orbit.resume, null);
  assert.equal(orbit.legacy, true);
  const hidden = body.threads.find((item) => item.withheld);
  assert.deepEqual([hidden.text, hidden.fields, hidden.source_title, hidden.legacy], ["", null, "", false]);
  assert.deepEqual(body.global.map((item) => item.state), ["resolved", "expired", "open"]);
  assert.equal(body.global[0].resolved_by.id, "src-20261031-constellation#L1");
  assert.equal(body.global[1].until, "2026-10-31T12:00:00Z");
  assert.deepEqual(body.history.map((item) => item.superseded), [false, false, false, true, true, false]);
});

test("legacy handoff filters and the page offset pass through to the tool; unknown or malformed parameters are refused", async (t) => {
  const { url, calls } = await scribeServer(t);
  for (const [query, recorded] of [["project=constellation", "project"], ["orchestrator=codex", "orchestrator"], [`thread=${encodeURIComponent("task:ORB-21001")}`, "thread"], ["offset=4", "page"]]) {
    const { status, body } = await get(`${url}/api/scribe/handoffs?${query}`);
    assert.equal(status, 200, query);
    assert.deepEqual(calls.at(-1).input, handoffs[recorded].input, query);
    assert.deepEqual(body.history.map((item) => item.id), listed(handoffs[recorded].output, "history").items.map((item) => item.id), query);
  }
  const before = calls.length;
  for (const query of ["view=brief", "offset=-1", "offset=abc", "offset=100001", `project=${"x".repeat(257)}`, "thread=%0A", "project=%20"]) {
    assert.deepEqual(await get(`${url}/api/scribe/handoffs?${query}`), { status: 400, body: { error: "invalid_input" } }, query);
  }
  assert.equal(calls.length, before, "refused before any tool call");
});
