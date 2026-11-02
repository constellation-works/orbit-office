/**
 * Scribe fixture: the tool outputs the scribe feature shapes, in the
 * tools' own response schemas.
 *
 * `scribe-fixture.json` is the verbatim output of the `office` binary
 * (`office orbit-tool`) over a two-session v1 Library; only the ask
 * answers below still cite its lines (`office.scribe-ask` needs a model,
 * so its two answers are written here by hand in the tool's schema).
 *
 * The v2 outputs are written here by hand in the tools' schemas, as the
 * Rust tests pin them (`session_brief_modules/list.rs` for the cards,
 * `scribe_query`'s v2 modes for the panels): `feed` is
 * `office.session-brief` with `view: list` (three cards — the latest
 * global SCR, the latest orbit-office SCR chained to an older one, and a
 * private project SCR), `byId` its answers with `id`, `preferences` and
 * `decisions` the `office.scribe-query` modes, and `status` the
 * `office.status` of a Library with one custom tag. `office.config-set`
 * refuses the app's authority (`capability_denied`), as Orbit does until
 * ORB-20363 grants it the `scribe.*` keys.
 *
 * `handoffs-fixture.json` is the verbatim legacy `office.session-brief`
 * output (`view: list` with `legacy: true`) over four conversation
 * sources, as before.
 */
import { readFileSync } from "node:fs";
import { createServer } from "node:http";

import { createScribeFeature } from "../dist/features/scribe/index.js";
import { createHandler } from "../dist/http/router.js";
import { OrbitExitError } from "../dist/orbit/client.js";

export const office = JSON.parse(readFileSync(new URL("./scribe-fixture.json", import.meta.url), "utf8"));
export const handoffs = JSON.parse(readFileSync(new URL("./handoffs-fixture.json", import.meta.url), "utf8"));
const sameInput = (a, b) => JSON.stringify(Object.entries(a).sort()) === JSON.stringify(Object.entries(b).sort());
export const NOW = new Date(2026, 10, 1, 9, 0, 0);
const decision = office.why.results.find((row) => row.why !== null);
const cite = (row) => row.citation;

function card(overrides) {
  return {
    id: "SCR-20261031-0900-aaaa02", path: "sources/conversation/2026/11/SCR-20261031-0900-aaaa02.md",
    title: "Ship the scribe feed", title_by: "scribe", scope: "project:orbit-office", project: "orbit-office",
    session: "session-aaaa02", started: "2026-10-31T09:00:00Z", updated: "2026-10-31T09:00:00Z",
    previous: "SCR-20261030-0900-aaaa01", scribed_through: "m-0003",
    summary: "(scribe) Rework the Scribe tab into the one-card feed.",
    goals: ["Ship the feed for ORB-21120."], outcome: ["The cards feed reads view list."],
    next: ["ORB-21120 — finish the Tags editor. owner: agent."],
    tag_entries: 3, tag_counts: { Decisions: 2, Preferences: 1 },
    tags: [
      { id: "T1", citation: "SCR-20261031-0900-aaaa02#T1", group: "Preferences", msg: "m-0001", time: "2026-10-31T09:01-07:00", by: "user", scope: "global", quote: "#preferences always draft emails, never send", fields: { matter: "email", proposal: "prop-1" } },
      { id: "T2", citation: "SCR-20261031-0900-aaaa02#T2", group: "Decisions", msg: "m-0002", time: "2026-10-31T09:05-07:00", by: "user", scope: "project:orbit-office", quote: "#decision ship v2 first because: the feed is unusable", fields: { ledger: "dec-orbit-office-7" } },
      { id: "T3", citation: "SCR-20261031-0900-aaaa02#T3", group: "Decisions", msg: "m-0003", time: "2026-10-31T09:06-07:00", by: "agent", scope: "project:orbit-office", quote: "#decision keep the legacy list behind legacy:true", fields: { ledger: "dec-orbit-office-8" } },
    ],
    sensitivity: "normal", content_withheld: false,
    ...overrides,
  };
}

const older = card({
  id: "SCR-20261030-0900-aaaa01", path: "sources/conversation/2026/11/SCR-20261030-0900-aaaa01.md",
  title: "Design the scribe feed", session: "session-aaaa01", started: "2026-10-30T09:00:00Z",
  updated: "2026-10-30T09:00:00Z", previous: null, summary: "(scribe) Design one card per project.",
});
const latest = card({});
const globalCard = card({
  id: "SCR-20261029-0900-cccc01", path: "sources/conversation/2026/11/SCR-20261029-0900-cccc01.md",
  title: "Plan the week", scope: "global", project: null, session: "session-cccc01",
  started: "2026-10-29T09:00:00Z", updated: "2026-10-29T09:00:00Z", previous: null,
  summary: "(scribe) Plan the week across projects.",
});
const privateCard = card({
  id: "SCR-20261028-0900-dddd01", path: "sources/conversation/2026/11/SCR-20261028-0900-dddd01.md",
  title: null, scope: "project:secret-project", project: "secret-project", session: "session-dddd01",
  started: "2026-10-28T09:00:00Z", updated: "2026-10-28T09:00:00Z", previous: null,
  summary: null, goals: [], outcome: [], next: [], tag_entries: 3, tag_counts: {}, tags: [],
  sensitivity: "private", content_withheld: true,
});

/** `office.session-brief` `view: list` outputs: the feed and each chain step by id. */
export const cards = {
  feed: { cards: [globalCard, latest, privateCard], count: 3, files_searched: 4 },
  byId: {
    [latest.id]: { scr: latest, previous: older },
    [older.id]: { scr: older, previous: null },
    [globalCard.id]: { scr: globalCard, previous: null },
    [privateCard.id]: { scr: privateCard, previous: null },
  },
};

/** `office.scribe-query` mode `preferences`. */
export const preferences = {
  mode: "preferences", count: 2, today: "2026-11-01",
  preferences: [
    {
      page: "dom-preferences-global", scope: "global", matter: "email", citation: "dom-preferences-global#email",
      statement: "Always draft emails, never send.", cites: ["SCR-20261031-0900-aaaa02#T1"],
      stated_at: "2026-10-31", review_by: "2027-01-29", stale: false,
      proposed: [{ entry: "SCR-20261031-0900-aaaa02#T1", proposal: "prop-1" }],
    },
    {
      page: "dom-preferences-orbit-office", scope: "project:orbit-office", matter: "release-with-open-defects",
      citation: "dom-preferences-orbit-office#release-with-open-defects",
      statement: "Hold releases with open defects for review.", cites: ["SCR-20261030-0900-aaaa01#T1"],
      stated_at: "2026-08-01", review_by: "2026-10-31", stale: true, proposed: [],
    },
  ],
};

/** `office.scribe-query` mode `decisions` (ledger order; the panel shows newest first). */
export const decisions = {
  mode: "decisions", count: 2, ledgers: ["decisions-orbit-office"], entries_searched: 2,
  decisions: [
    {
      id: "dec-orbit-office-7", question: "Ship v2 or fix the v1 feed?", question_by: "scribe",
      options: "unrecoverable", chosen: "ship v2 first", quote: "#decision ship v2 first because: the feed is unusable",
      ref: "SCR-20261031-0900-aaaa02#T2 m-0002", because: { text: "the feed is unusable", ref: "SCR-20261031-0900-aaaa02#T2 m-0002" },
      scope: "project:orbit-office", context: { task_type: "feature", workspace: "ws_orbit-office", task: "ORB-21120", risk_tier: null },
      decided_by: "human", decided_at: "2026-10-31T09:05:00-07:00", reverses: null, source: "SCR-20261031-0900-aaaa02",
      reversed_by: null, ledger: "decisions-orbit-office", line: 7, current: true,
    },
    {
      id: "dec-orbit-office-8", question: "Keep the legacy list during the window?", question_by: "scribe",
      options: "unrecoverable", chosen: "keep it behind legacy:true", quote: "#decision keep the legacy list behind legacy:true",
      ref: "SCR-20261031-0900-aaaa02#T3 m-0003",
      scope: "project:orbit-office", context: { task_type: null, workspace: null, task: null, risk_tier: null },
      decided_by: "agent", decided_at: "2026-10-31T09:06:00-07:00", reverses: null, source: "SCR-20261031-0900-aaaa02",
      reversed_by: null, ledger: "decisions-orbit-office", line: 8, current: true,
    },
  ],
};

/** The two shipped defaults plus one custom tag, as `office.status` reports them. */
export const statusTags = [
  { name: "preferences", scope: "global", instruction: "A standing answer to a class of question, in the user's words.", sink: "preference_page", mode: "propose", capture: "markers" },
  { name: "decisions", scope: "session", instruction: "A choice that was made, with the question and options from the turns before it.", sink: "decisions_ledger", mode: "append", capture: "markers" },
  { name: "lesson", scope: "session", instruction: "A lesson worth proposing to polaris.", sink: "orbit_task", mode: "propose", capture: "markers", workspace: "ws_polaris" },
];

/** `office.scribe-ask` answering from the v1 fixture: one good claim and one whose quote is not on its line. */
export const askAnswered = {
  question: "How often does the screen poll?",
  not_captured: false, status: "answered", dropped_claims: 1,
  claims: [
    claimOf(decision, "The screen polls every ten seconds.", "poll every ten seconds"),
    claimOf(decision, "It uses a socket.", "a websocket for every window"),
  ],
  derived_by: { route: "hosted", model: "haiku" },
  shortlist: [shortOf(decision)], evidence: [shortOf(decision)], private_references: [],
  searched: {}, since: null, until: null, date_range: { since: null, until: null },
  topics: [], sessions: [], sessions_not_scribed_past_last_message: [], sessions_not_scribed: [],
  files_searched: 2, file_count: 2, matched: 1,
};

/** `office.scribe-ask` when no line matches the question. */
export const askNotCaptured = {
  question: "What did we order for lunch?",
  not_captured: true, status: "not_captured", dropped_claims: 0, claims: [], derived_by: null,
  shortlist: [], evidence: [], private_references: [], searched: {},
  since: "2026-10-31", until: "2026-11-01", date_range: { since: "2026-10-31", until: "2026-11-01" },
  topics: ["recall", "scribe"], sessions: [], sessions_not_scribed_past_last_message: [], sessions_not_scribed: [],
  files_searched: 2, file_count: 2, matched: 0,
};

function shortOf(row) {
  return { cite: cite(row), citation: cite(row), path: row.path, line: row.line, file_line: row.file_line, text: row.text,
    line_text: row.text, quote: row.quote, msg: row.msg, session_id: row.session_id, content_withheld: false };
}
function claimOf(row, text, quote) {
  return { text, quote, cites: [cite(row)], cite: cite(row), citation: cite(row), path: row.path, line: row.line,
    file_line: row.file_line, "file:line": row.file_line, scr_id: row.scr_id, id: row.scr_id, msg: row.msg,
    message_id: row.msg, session_id: row.session_id, content_withheld: false };
}

/** A fake Orbit client over the fixture outputs; `office.config-set` refuses the app's authority. */
export function scribeClient() {
  const calls = [];
  const answer = (name, input) => {
    calls.push({ name, input: structuredClone(input) });
    if (name === "office.session-brief") {
      if (input.legacy === true) {
        const recorded = Object.values(handoffs).find((entry) => sameInput(entry.input, input));
        if (recorded === undefined) throw new Error(`no recorded handoff list for ${JSON.stringify(input)}`);
        return structuredClone(recorded.output);
      }
      if (input.id !== undefined) {
        const detail = cards.byId[input.id];
        if (detail === undefined) throw new Error(`no recorded SCR for ${input.id}`);
        return structuredClone(detail);
      }
      return structuredClone(cards.feed);
    }
    if (name === "office.scribe-query") {
      if (input.mode === "preferences") return structuredClone(preferences);
      if (input.mode === "decisions") return structuredClone(decisions);
      throw new Error(`unexpected scribe-query mode ${input.mode}`);
    }
    if (name === "office.status") {
      return { scribe: { cadence: { minutes: 20, turns: 10 }, tags: structuredClone(statusTags), tags_customized: true } };
    }
    if (name === "office.config-set") {
      throw new OrbitExitError("office.config-set", 1, null,
        JSON.stringify({ error: { code: "capability_denied", message: "the app does not hold office.config-set" } }));
    }
    if (name === "office.scribe-ask") return structuredClone(/lunch/.test(input.question) ? askNotCaptured : askAnswered);
    throw new Error(`unexpected fixture call ${name}`);
  };
  return {
    calls,
    async callTool(name, input) { return answer(name, input); },
    async pollTool(name, input) { return answer(name, input); },
  };
}

/** The production scribe feature on a loopback server, with the app shell for the browser. */
export async function scribeServer(t) {
  const client = scribeClient();
  const feature = createScribeFeature(client);
  const fallback = createHandler();
  const server = createServer((req, res) => {
    const route = feature.routes.find((r) => r.method === req.method && r.path === req.url.split("?")[0]);
    if (route) void route.handle(req, res);
    else if (req.url.startsWith("/api/") && !req.url.endsWith(".js")) {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(JSON.stringify(req.url === "/api/today" ? { needs_you: [], agents_working_on: [], done_today: [], truncated: false, completion_dates_unavailable: 0 }
        : req.url === "/api/setup" ? { state: "ready" } : { route: null, token: "unset" }));
    } else fallback(req, res);
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise((done) => server.close(done)));
  return { url: `http://127.0.0.1:${server.address().port}`, calls: client.calls };
}
