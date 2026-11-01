import assert from "node:assert/strict";
import { test } from "node:test";

import { emptyView, leadingTag, messageFor, notCapturedText, sessionMeta, sessionTitle, suggestedTag, timeLabel } from "../dist/screens/scribe/model.js";

test("a line's time reads as written, without seconds or offset", () => {
  assert.equal(timeLabel("2026-11-01T06:05+00:00"), "2026-11-01 06:05");
  assert.equal(timeLabel("2026-10-31T22:24-07:00"), "2026-10-31 22:24");
  assert.equal(timeLabel("garbled"), "garbled");
});

test("a session reads by title and project, untitled ones plainly", () => {
  const session = { session_id: "s", title: null, project: "constellation", counterpart_name: null, scribed_through: "m-0004", latest: "", entries: [{}, {}] };
  assert.equal(sessionTitle(session), "Untitled session");
  assert.equal(sessionMeta(session), "constellation · 2 lines");
  assert.equal(sessionTitle({ ...session, title: "Scribe screen" }), "Scribe screen");
  assert.equal(sessionMeta({ ...session, project: null, entries: [{}] }), "1 line");
});

test("a note's tag: a leading #tag wins, then plain cues, else fact", () => {
  assert.equal(leadingTag("#todo call the venue"), "todo");
  assert.equal(leadingTag("call #todo"), null);
  assert.equal(suggestedTag("#reminder at 3pm"), "reminder");
  assert.equal(suggestedTag("Decision: ship parse first"), "decision");
  assert.equal(suggestedTag("I need to call the venue"), "todo");
  assert.equal(suggestedTag("remind me to stretch"), "reminder");
  assert.equal(suggestedTag("Which host runs the gateway?"), "question");
  assert.equal(suggestedTag("The NAS is on 10.0.0.5"), "fact");
});

test("refusals read in plain words and never show the code", () => {
  for (const code of ["no_cursor", "scribe_not_parsed", "tag_not_permitted", "something_new", "network_error"]) {
    assert.ok(!messageFor(code).includes(code), code);
    assert.ok(messageFor(code).endsWith("."));
  }
});

test("not captured states the searched range, files and topics; empty views say what was searched", () => {
  const base = { status: "not_captured", claims: [], references: [], dropped: 0, model: null };
  assert.equal(notCapturedText({ ...base, since: "2026-10-31", until: "2026-11-01", topics: ["recall", "scribe"], files_searched: 2 }),
    "Not captured. Searched 2026-10-31 to 2026-11-01 across 2 files; topics recall, scribe.");
  assert.equal(notCapturedText({ ...base, since: null, until: null, topics: [], files_searched: 1 }), "Not captured. Searched every date across 1 file.");
  assert.equal(emptyView("landed", null, "America/Los_Angeles", "2026-11-01"), "Nothing landed today (2026-11-01, America/Los_Angeles).");
  assert.equal(emptyView("pref", "review-cadence", "UTC", "2026-11-01"), "No preference recorded on review-cadence.");
});
