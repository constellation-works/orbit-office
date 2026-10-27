import assert from "node:assert/strict";
import { test } from "node:test";
import { createInsightsFeature } from "../dist/features/insights/index.js";

const metrics = {
  automation: { count_ratio: 0.75, minutes_ratio: 0.78, automated_count: 3, total_count: 4 }, hours_returned: 1.75,
  first_pass_acceptance: { ratio: 2 / 3 }, knowledge_coverage: { ratio: 0.5 },
  proposal_acceptance: { ratio: 2 / 3 }, open_load: [],
  conversions: [{ cluster: "weekly-reports", proposal: "proposal-automation", manual_tasks: ["manual-period"] }],
};
export const fixture = { since: "2026-10-01", until: "2026-10-22", totals: metrics, weeks: [
  { since: "2026-10-01", metrics },
  { since: "2026-10-05", metrics: { ...metrics, automation: { ...metrics.automation, count_ratio: null } } },
  { since: "2026-10-12", metrics },
], simulated_until: "2026-10-05" };

function capture() { return { writeHead(status) { this.status = status; }, end(body) { this.body = JSON.parse(body); } }; }

test("Insights route resolves proposal titles and preserves metrics and optional simulation date", async () => {
  const calls = [];
  const client = { async pollTool(name, input) { calls.push({ name, input }); return name === "office.metrics" ? { ok: true, output: fixture } : { title: "Prepare weekly reports" }; } };
  const route = createInsightsFeature(client, () => new Date("2026-10-21T12:00:00Z")).routes[0];
  const response = capture(); await route.handle({}, response);
  assert.equal(response.status, 200);
  assert.deepEqual(calls, [{ name: "office.metrics", input: { since: "2026-07-30", until: "2026-10-22" } }, { name: "orbit.task.show", input: { id: "proposal-automation", fields: ["title"] } }]);
  assert.deepEqual(response.body.totals.conversions, [{ name: "weekly reports", proposal_title: "Prepare weekly reports" }]);
  assert.equal(response.body.simulated_until, "2026-10-05");
  assert.deepEqual(response.body.weeks, fixture.weeks);
  const absent = { ...fixture }; delete absent.simulated_until;
  const noDate = capture(); await createInsightsFeature({ pollTool: async (name) => name === "office.metrics" ? absent : { title: "Prepare weekly reports" } }).routes[0].handle({}, noDate);
  assert.equal("simulated_until" in noDate.body, false);
  const failed = capture(); await createInsightsFeature({ pollTool: async () => ({ ok: false }) }).routes[0].handle({}, failed);
  assert.equal(failed.status, 502);
});
