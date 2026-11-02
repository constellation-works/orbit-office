import assert from "node:assert/strict";
import { test } from "node:test";

import { accuracyLabel, blankTag, categoryLabel, defaultModeForSink, hasEnoughSamples, messageFor, modesForSink, notCapturedText, percentLabel, timeLabel } from "../dist/screens/scribe/model.js";

test("a line's time reads as written, without seconds or offset", () => {
  assert.equal(timeLabel("2026-11-01T06:05+00:00"), "2026-11-01 06:05");
  assert.equal(timeLabel("2026-10-31T22:24-07:00"), "2026-10-31 22:24");
  assert.equal(timeLabel("garbled"), "garbled");
});

test("refusals read in plain words and never show the code", () => {
  for (const code of ["scribe_scr_not_found", "invalid_library_setting", "something_new", "network_error"]) {
    assert.ok(!messageFor(code).includes(code), code);
    assert.ok(messageFor(code).endsWith("."), code);
  }
  assert.match(messageFor("not_permitted"), /nothing was saved/);
});

test("not captured states the searched range, files and topics", () => {
  const base = { status: "not_captured", claims: [], references: [], dropped: 0, model: null };
  assert.equal(notCapturedText({ ...base, since: "2026-10-31", until: "2026-11-01", topics: ["recall", "scribe"], files_searched: 2 }),
    "Not captured. Searched 2026-10-31 to 2026-11-01 across 2 files; topics recall, scribe.");
  assert.equal(notCapturedText({ ...base, since: null, until: null, topics: [], files_searched: 1 }), "Not captured. Searched every date across 1 file.");
});

test("a sink takes only its paired modes, and a new tag starts as a markers-only, cadence-trigger session record", () => {
  assert.deepEqual(modesForSink("preference_page"), ["propose", "apply"]);
  assert.deepEqual(modesForSink("orbit_task"), ["propose"]);
  assert.deepEqual(modesForSink("decisions_ledger"), ["append"]);
  assert.deepEqual(modesForSink("scr_only"), ["append"]);
  assert.equal(defaultModeForSink("preference_page"), "propose");
  assert.equal(defaultModeForSink("decisions_ledger"), "append");
  assert.deepEqual(blankTag(), { name: "", scope: "session", instruction: "", sink: "scr_only", mode: "append", capture: "markers", trigger: "cadence" });
});

test("agreement rows: percents, Wilson interval, low-sample gate and plain category words", () => {
  const category = {
    category: "ws_orbit-office:agent_request", workspace: "ws_orbit-office", kind: "agent_request",
    mode: "shadow", scored_requests: 42, accuracy: 0.81, accuracy_wilson_low: 0.68, accuracy_wilson_high: 0.9,
    coverage: 0.74, calibration_error: 0.04, abstention_rate: 0.12, conflict_rate: 0.05, min_samples: 30,
  };
  assert.equal(percentLabel(0.74), "74%");
  assert.equal(percentLabel(null), "—");
  assert.equal(accuracyLabel(category), "81% (68%–90%)");
  assert.equal(accuracyLabel({ ...category, accuracy_wilson_low: null }), "81%");
  assert.equal(categoryLabel(category), "ws_orbit-office · agent request");
  assert.equal(hasEnoughSamples(category), true);
  assert.equal(hasEnoughSamples({ ...category, scored_requests: 7 }), false, "below min_samples shows Not enough decisions yet");
  assert.equal(messageFor("agreement_unavailable"), "Could not read Orbit's measurement.");
});

test("card helpers: age, scope, summary and tag counts read in plain words", async () => {
  const { ageLabel, cardSummary, cardTitle, scopeLabel, tagCountsLabel, taskHref, SCRIBE_FEED_HREF } = await import("../dist/ui/handoff.js");
  const now = new Date("2026-11-01T09:00:00Z");
  assert.equal(ageLabel("2026-11-01T08:59:40Z", now), "just now");
  assert.equal(ageLabel("2026-11-01T08:30:00Z", now), "30 min ago");
  assert.equal(ageLabel("2026-10-31T10:00:00Z", now), "23 h ago");
  assert.equal(ageLabel("2026-10-28T09:00:00Z", now), "4 d ago");
  assert.equal(taskHref("ORB-21120"), "orbit://task/ORB-21120");
  assert.equal(SCRIBE_FEED_HREF, "#scribe");
  const base = {
    id: "SCR-20261031-0900-aaaa02", title: "Ship the feed", scope: "project:orbit-office", project: "orbit-office",
    summary: "(scribe) Rework the tab.", withheld: false, tag_counts: { Decisions: 2, Preferences: 1 },
  };
  assert.equal(scopeLabel(base), "orbit-office");
  assert.equal(scopeLabel({ ...base, scope: "global", project: null }), "Global");
  assert.equal(cardTitle(base), "Ship the feed");
  assert.equal(cardTitle({ ...base, title: null }), "SCR-20261031-0900-aaaa02");
  assert.equal(cardSummary(base), "(scribe) Rework the tab.");
  assert.equal(cardSummary({ ...base, withheld: true, summary: null }), "Private session: content withheld.");
  assert.equal(tagCountsLabel(base), "2 decisions · 1 preference");
  assert.equal(tagCountsLabel({ ...base, tag_counts: { lesson: 1 } }), "1 lesson");
  assert.equal(tagCountsLabel({ ...base, tag_counts: {} }), "No tagged entries");
});
