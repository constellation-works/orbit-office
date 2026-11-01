/**
 * Scribe fixture: a Library holding SCR files for two sessions, as Office
 * itself answers for it. `scribe-fixture.json` is the verbatim output of the
 * `office` binary (`office orbit-tool`) built from this revision, over a
 * fresh Library where `office.scribe-pending` and `office.scribe-append`
 * scribed two transcripts (an older "constellation" session and the most
 * recent "Scribe screen" session in "orbit-office"):
 *
 * - `feed`: `office.scribe-query` with no mode (every current line);
 * - `pending`, `pref`, `landed`, `why`: the canned modes (landed defaults to today);
 * - `note`: the `office.scribe-append` a web note sends, with `through` set to
 *   the session's own cursor; `after_note`: that session's lines afterwards.
 *
 * The client below answers the server's `orbit tool run` calls from those
 * outputs, records every call, and adds the appended note to later reads.
 * Office's `office.scribe-ask` needs a model, so its two answers are written
 * here by hand in the tool's response schema, citing fixture lines.
 *
 * `handoffs-fixture.json` is the verbatim `office.session-brief` output
 * (`view: list`) of the `office` binary built from this revision over a
 * Library of four conversation sources: "Scribe screen" (orbit-office,
 * claude) with a superseded legacy handoff, an open, an expired and a
 * resolved global and a structured handoff for ORB-21075; "Plan the
 * constellation rollout" (constellation, claude) resolving that global and
 * two handoffs for ORB-21001; "Orbit release" (orbit, codex); and a private
 * source. Each case records the input it answers (`all`, a `project`,
 * `orchestrator` and `thread` filter, and the `page` at offset 4).
 */
import { readFileSync } from "node:fs";
import { createServer } from "node:http";

import { createScribeFeature } from "../dist/features/scribe/index.js";
import { createHandler } from "../dist/http/router.js";

export const office = JSON.parse(readFileSync(new URL("./scribe-fixture.json", import.meta.url), "utf8"));
export const handoffs = JSON.parse(readFileSync(new URL("./handoffs-fixture.json", import.meta.url), "utf8"));
const sameInput = (a, b) => JSON.stringify(Object.entries(a).sort()) === JSON.stringify(Object.entries(b).sort());
export const NOW = new Date(2026, 10, 1, 9, 0, 0);
export const RECENT = office.after_note.results[0].session_id;
export const OLDER = office.feed.results[0].session_id;
const noteLine = office.after_note.results.find((row) => row.by === "user");
const cite = (row) => row.citation;
const decision = office.why.results.find((row) => row.why !== null);

/** `office.scribe-ask` answering from the fixture: one good claim and one whose quote is not on its line. */
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

/** A fake Orbit client over the fixture outputs. */
export function scribeClient() {
  const calls = [];
  let noted = false;
  const lines = () => noted ? [...office.feed.results, noteLine] : office.feed.results;
  const answer = (name, input) => {
    calls.push({ name, input: structuredClone(input) });
    if (name === "office.scribe-query") {
      const mode = input.mode ?? null;
      if (mode !== null) return structuredClone(office[mode]);
      const rows = lines().filter((row) => input.session_id === undefined || row.session_id === input.session_id);
      // The general query keeps the newest `limit` rows (canned modes above
      // return their canned outputs unchanged).
      const limited = rows.slice(-(input.limit ?? 100));
      return { ...structuredClone(office.feed), results: structuredClone(limited), count: rows.length, since: input.since ?? null };
    }
    if (name === "office.scribe-append") {
      const through = office.feed.results.find((row) => row.session_id === input.session_id)?.scribed_through;
      if (input.through !== through) throw new Error(`append moved the cursor: ${input.through}`);
      noted = true;
      return structuredClone(office.note);
    }
    if (name === "office.session-brief") {
      const recorded = Object.values(handoffs).find((entry) => sameInput(entry.input, input));
      if (recorded === undefined) throw new Error(`no recorded handoff list for ${JSON.stringify(input)}`);
      return structuredClone(recorded.output);
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
  const feature = createScribeFeature(client, () => NOW);
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
