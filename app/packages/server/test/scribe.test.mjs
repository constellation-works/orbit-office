/**
 * The Scribe feature through its HTTP routes, on the two-session fixture
 * Library (scribe-fixture.mjs): the feed, the canned views, a note and the
 * ask box, each checked against what Office returned for the same call.
 */
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";

import { createScribeFeature } from "../dist/features/scribe/index.js";
import { noteEntry } from "../dist/features/scribe/model.js";
import { OrbitExitError } from "../dist/orbit/client.js";
import { NOW, OLDER, RECENT, handoffs, office, scribeServer } from "./scribe-fixture.mjs";

const get = async (url) => { const response = await fetch(url); return { status: response.status, body: await response.json() }; };
const post = async (url, body) => {
  const response = await fetch(url, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  return { status: response.status, body: await response.json() };
};
const instant = (time) => Date.parse(time);

/** A feature on a loopback server over any fake client. */
async function serve(t, client) {
  const feature = createScribeFeature(client, () => NOW);
  const server = createServer((req, res) => {
    const route = feature.routes.find((r) => r.method === req.method && r.path === req.url.split("?")[0]);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end("{}"); }
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise((done) => server.close(done)));
  return `http://127.0.0.1:${server.address().port}`;
}

test("the feed lists every line newest first, the most recent session first, grouped by title and project", async (t) => {
  const { url, calls } = await scribeServer(t);
  const { status, body } = await get(`${url}/api/scribe/feed`);
  assert.equal(status, 200);
  assert.deepEqual(calls.map((call) => call.input), [{ since: "2026-10-03", limit: 1000 }], "one 30-day window, read once");
  assert.deepEqual(body.sessions.map((session) => [session.session_id, session.title, session.project]),
    [[RECENT, "Scribe screen", "orbit-office"], [OLDER, null, "constellation"]]);
  for (const session of body.sessions) {
    const times = session.entries.map((entry) => instant(entry.time));
    assert.deepEqual(times, [...times].sort((a, b) => b - a), `${session.session_id} is newest first`);
  }
  // Every line Office returned is shown once, with its tags, quote, time and anchors.
  const shown = body.sessions.flatMap((session) => session.entries);
  assert.equal(shown.length, office.feed.results.length);
  assert.equal(body.shown, office.feed.results.length);
  for (const row of office.feed.results) {
    const entry = shown.find((item) => item.citation === row.citation);
    assert.ok(entry, `${row.citation} is in the feed`);
    assert.deepEqual([entry.file_line, entry.tag, entry.topics, entry.quote, entry.time, entry.msg, entry.resume, entry.why, entry.matter, entry.refs, entry.by],
      [row.file_line, row.tag, row.topics, row.quote, row.time, row.msg, row.resume, row.why, row.matter, row.refs, row.by]);
  }
  assert.equal(body.sessions[0].scribed_through, "m-0005");
  assert.equal(body.time_zone, office.feed.time_zone);
});

test("a window holding more lines than the limit narrows until the newest fit", async (t) => {
  const inputs = [];
  const client = {
    async pollTool(_name, input) {
      inputs.push(input);
      return { ...office.feed, results: office.feed.results, count: input.since === "2026-11-01" ? office.feed.results.length : 5000 };
    },
    async callTool() { throw new Error("unexpected"); },
  };
  const url = await serve(t, client);
  const { body } = await get(`${url}/api/scribe/feed`);
  assert.deepEqual(inputs.map((input) => input.since), ["2026-10-03", "2026-10-26", "2026-11-01"]);
  assert.equal(body.since, "2026-11-01");
});

test("an exact-cap day is shown whole and an over-cap day keeps its newest lines", async (t) => {
  // Rows as the real general scribe-query returns them: ascending
  // (time, path, line), bounded to the newest `limit`, count = full total.
  // All rows share one timestamp, so line order is the tie-break.
  const template = office.feed.results[0];
  const denseClient = (count) => {
    const rows = Array.from({ length: count }, (_, i) => {
      const line = 21 + i;
      const label = `event ${String(i).padStart(4, "0")}`;
      return {
        ...template, citation: `${template.id}#L${line}`, file_line: `${template.path}:${line}`, line,
        msg: `m-${String(i).padStart(4, "0")}`, time: "2026-11-01T09:00:00-07:00", quote: label,
        text: `- note 2026-11-01T09:00:00-07:00 #fact "${label}"`,
      };
    });
    return {
      async pollTool(_name, input) {
        const matched = rows.filter((row) => row.time.slice(0, 10) >= input.since)
          .sort((a, b) => a.time.localeCompare(b.time) || a.path.localeCompare(b.path) || a.line - b.line);
        return {
          ...structuredClone(office.feed), results: structuredClone(matched.slice(-input.limit)),
          count: matched.length, since: input.since,
        };
      },
      async callTool() { throw new Error("unexpected"); },
    };
  };

  const exactUrl = await serve(t, denseClient(1000));
  const exact = (await get(`${exactUrl}/api/scribe/feed`)).body;
  assert.deepEqual([exact.total, exact.shown, exact.since], [1000, 1000, "2026-10-03"]);
  assert.equal(exact.sessions[0].entries[0].quote, "event 0999");
  assert.equal(exact.sessions[0].entries.at(-1).quote, "event 0000");

  const overUrl = await serve(t, denseClient(1002));
  const over = (await get(`${overUrl}/api/scribe/feed`)).body;
  assert.deepEqual([over.total, over.shown, over.since], [1002, 1000, "2026-11-01"]);
  const quotes = over.sessions[0].entries.map((entry) => entry.quote);
  assert.equal(quotes.length, 1000);
  assert.deepEqual([quotes[0], quotes[1]], ["event 1001", "event 1000"], "the newest lines are kept, newest first");
  assert.ok(!quotes.includes("event 0000") && !quotes.includes("event 0001"), "the oldest lines are the truncated ones");
  assert.equal(quotes.at(-1), "event 0002");
});

test("Pending, Preferences, Landed and Why return exactly the lines office.scribe-query returns for the same query", async (t) => {
  const { url, calls } = await scribeServer(t);
  for (const view of ["pending", "pref", "landed", "why"]) {
    const { status, body } = await get(`${url}/api/scribe/query?view=${view}`);
    assert.equal(status, 200);
    assert.deepEqual(calls.at(-1), { name: "office.scribe-query", input: { mode: view } });
    assert.deepEqual(body.entries.map((entry) => [entry.citation, entry.quote]), office[view].results.map((row) => [row.citation, row.quote]), view);
    assert.equal(body.count, office[view].count);
  }
  const pending = (await get(`${url}/api/scribe/query?view=pending`)).body;
  assert.ok(!pending.entries.some((entry) => entry.refs.includes("ORB-20780")), "the resolved pending is not listed");
  assert.deepEqual(pending.entries.map((entry) => instant(entry.time)), pending.entries.map((entry) => instant(entry.time)).sort((a, b) => a - b), "oldest first");
  const pref = (await get(`${url}/api/scribe/query?view=pref`)).body;
  assert.deepEqual(pref.entries.map((entry) => [entry.quote, entry.stale, entry.review_by]), [["hold releases with open defects for review", true, "2026-10-31"]]);
  assert.ok(!pref.entries.some((entry) => entry.quote === "ship early"), "the superseded preference is hidden");
  const landed = (await get(`${url}/api/scribe/query?view=landed`)).body;
  assert.equal(landed.since, landed.today); assert.equal(landed.until, landed.today); assert.equal(landed.time_zone, office.landed.time_zone);
  const why = (await get(`${url}/api/scribe/query?view=why`)).body;
  assert.deepEqual(why.entries.map((entry) => [entry.quote, entry.why, entry.no_why]),
    [["lets go one step at a time", null, true], ["poll every ten seconds", "a socket is overkill for one window", false]]);
});

test("Why on a word tries it as a matter, then as a topic; Preferences pass the matter", async (t) => {
  const inputs = [];
  const client = {
    async pollTool(_name, input) { inputs.push(input); return input.topic ? office.why : { ...office.why, results: [], count: 0 }; },
    async callTool() { throw new Error("unexpected"); },
  };
  const url = await serve(t, client);
  const { body } = await get(`${url}/api/scribe/query?view=why&about=%23scribe`);
  assert.deepEqual(inputs, [{ mode: "why", matter: "scribe" }, { mode: "why", topic: "scribe" }]);
  assert.equal(body.entries.length, office.why.results.length);
  await get(`${url}/api/scribe/query?view=pref&about=release-with-open-defects`);
  assert.deepEqual(inputs.at(-1), { mode: "pref", matter: "release-with-open-defects" });
  assert.equal((await get(`${url}/api/scribe/query?view=everything`)).status, 400);
  assert.equal((await get(`${url}/api/scribe/query?view=why&about=a%20b`)).status, 400);
  assert.equal((await get(`${url}/api/scribe/query?view=why&limit=5`)).status, 400, "an unknown parameter is refused");
});

test("a note appends one by: user line at the session's own cursor, and the next poll shows it", async (t) => {
  const { url, calls } = await scribeServer(t);
  const { status, body } = await post(`${url}/api/scribe/note`, { session_id: RECENT, text: "#todo call the venue at 3pm" });
  assert.equal(status, 200, JSON.stringify(body));
  assert.deepEqual(calls.map((call) => call.name), ["office.scribe-query", "office.scribe-append"]);
  assert.deepEqual(calls[0].input, { session_id: RECENT, limit: 1 });
  assert.deepEqual(calls[1].input, { session_id: RECENT, through: "m-0005", entries: [{ tag: "todo", by: "user", text: "call the venue at 3pm" }] });
  assert.deepEqual(body.added, [{ file_line: `${office.note.path}:25`, text: office.note.added[0].text }]);
  assert.match(body.added[0].text, / #todo "call the venue at 3pm" by: user$/);
  const feed = (await get(`${url}/api/scribe/feed`)).body;
  const first = feed.sessions[0].entries[0];
  assert.deepEqual([first.tag, first.by, first.msg, first.quote], ["todo", "user", "note", "call the venue at 3pm"]);
  assert.equal(feed.sessions[0].entries.length, office.after_note.results.length);
});

test("a note without a leading tag takes the chosen tag; bad bodies are refused before any tool call", async (t) => {
  const { url, calls } = await scribeServer(t);
  assert.equal((await post(`${url}/api/scribe/note`, { session_id: RECENT, text: "call the venue", tag: "todo" })).status, 200);
  assert.deepEqual(calls[1].input.entries, [{ tag: "todo", by: "user", text: "call the venue" }]);
  calls.length = 0;
  for (const [body, code] of [
    [{ session_id: RECENT, text: "untagged" }, "tag_required"],
    [{ session_id: RECENT, text: "#todo x", extra: 1 }, "invalid_input"],
    [{ session_id: "a b", text: "#todo x" }, "invalid_input"],
    [{ session_id: RECENT, text: "   " }, "note_text_required"],
  ]) {
    const response = await post(`${url}/api/scribe/note`, body);
    assert.deepEqual([response.status, response.body], [400, { error: code }]);
  }
  assert.deepEqual(calls, []);
});

test("a note is parsed exactly as /scribe note parses it", () => {
  assert.deepEqual(noteEntry('#preference matter: release-with-open-defects review_by: 2027-01-01 "hold for review"', undefined),
    { tag: "preference", by: "user", matter: "release-with-open-defects", review_by: "2027-01-01", text: "hold for review" });
  assert.deepEqual(noteEntry('#why "because grep" resolves: SCR-01-0600-scribe-screen#L22', undefined),
    { tag: "why", by: "user", resolves: "SCR-01-0600-scribe-screen#L22", text: "because grep" });
  assert.deepEqual(noteEntry("#todo refs: ORB-1,ORB-2 ship it", undefined), { tag: "todo", by: "user", refs: ["ORB-1", "ORB-2"], text: "ship it" });
  // Without a leading tag the text stays verbatim, keys and all.
  assert.deepEqual(noteEntry("matter: x stays as written", "fact"), { tag: "fact", by: "user", text: "matter: x stays as written" });
});

test("a refused note and a tool refusal come back as short codes", async (t) => {
  const refusing = {
    async callTool(name) {
      if (name === "office.scribe-query") return office.after_note;
      return { ...office.note, added: [], refused: [{ index: 0, msg: "note", reason: "tag_not_permitted", message: "x" }] };
    },
    async pollTool() { throw new Error("unexpected"); },
  };
  let url = await serve(t, refusing);
  assert.deepEqual((await post(`${url}/api/scribe/note`, { session_id: RECENT, text: "#pending x" })).body, { error: "tag_not_permitted" });
  const unparsed = {
    async callTool(name) {
      if (name === "office.scribe-query") return office.after_note;
      throw new OrbitExitError("office.scribe-append", 1, null, JSON.stringify({ error: { code: "scribe_not_parsed", message: "run office.scribe-pending first" } }));
    },
    async pollTool() { throw new Error("unexpected"); },
  };
  url = await serve(t, unparsed);
  assert.deepEqual(await post(`${url}/api/scribe/note`, { session_id: RECENT, text: "#todo x" }), { status: 422, body: { error: "scribe_not_parsed" } });
  const empty = { async callTool() { return { ...office.after_note, results: [] }; }, async pollTool() { throw new Error("unexpected"); } };
  url = await serve(t, empty);
  assert.deepEqual(await post(`${url}/api/scribe/note`, { session_id: "unknown", text: "#todo x" }), { status: 409, body: { error: "no_cursor" } });
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

const listed = (output, kind) => output.sections.find((section) => section.kind === kind);

test("handoffs read office.session-brief view list once and keep its order, states and anchors", async (t) => {
  const { url, calls } = await scribeServer(t);
  const { status, body } = await get(`${url}/api/scribe/handoffs`);
  assert.equal(status, 200);
  assert.deepEqual(calls.map((call) => [call.name, call.input]), [["office.session-brief", { view: "list", include_closed: true, limit: 50 }]]);
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
  // Codex sessions do not resume with claude; private sources stay withheld.
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

test("handoff filters and the page offset pass through to the tool; unknown or malformed parameters are refused", async (t) => {
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
