import assert from "node:assert/strict";
import { test } from "node:test";
import { createLibraryFeature } from "../dist/features/library/index.js";

const history = { revisions: [], total: 0, truncated: false };
function fixture() {
  const calls = [];
  const data = {
    "office.knowledge-list": { pages: [], missing_templates: [{ template: "user", id: "dom-user", title: "You" }] },
    "office.artifact-list": { artifacts: [], total: 101, truncated: true },
    "office.bootstrap-status": { phases: [], pending_batches: [] },
    "office.search": { count: 1, hits: [{ id: "dom-person-alex", kind: "domain", title: "Alex", path: "knowledge/alex.md", snippet: "A person" }] },
    "office.knowledge-show": { page: { id: "dom-person-alex", title: "Alex" }, backlinks: [{ id: "src-20261001-meeting", kind: "source", title: "Meeting", path: "sources/meeting.md", via: ["cites"] }], history },
    "office.artifact-show": { artifact: { id: "art-20261001-report", title: "Report" }, series_history: [{ id: "art-20260901-report", version: 1, supersedes: null, path: "artifacts/report.md" }], backlinks: [], truncated: false, history },
  };
  return { calls, async callTool(name, input) { calls.push({ name, input }); if (!(name in data)) throw new Error("unexpected call"); return { ok: true, output: data[name] }; } };
}
async function get(client, query = "") {
  const route = createLibraryFeature(client).routes[0];
  const response = { writeHead(status) { this.status = status; }, end(body) { this.body = JSON.parse(body); } };
  await route.handle({ url: `/api/library${query}` }, response); return response;
}
test("Library routes read listings, filter outputs, search and fetch document histories", async () => {
  const client = fixture(); const overview = await get(client); assert.equal(overview.status, 200); assert.equal(overview.body.artifacts.truncated, true); assert.equal(overview.body.pages.missing_templates[0].title, "You");
  assert.deepEqual(client.calls.find((c) => c.name === "office.artifact-list").input, { limit: 100 });
  await get(client, "?view=outputs&kind=report&series=weekly"); assert.deepEqual(client.calls.at(-1).input, { limit: 100, kind: "report", series: "weekly" });
  const search = await get(client, "?view=search&query=Alex"); assert.equal(search.body.hits[0].title, "Alex");
  const page = await get(client, "?view=document&id=dom-person-alex"); assert.equal(page.body.backlinks[0].title, "Meeting"); assert.deepEqual(client.calls.at(-1).input, { id: "dom-person-alex", history: true });
  const artifact = await get(client, "?view=document&id=art-20261001-report"); assert.equal(artifact.body.series_history[0].version, 1); assert.deepEqual(client.calls.at(-1).input, { id: "art-20261001-report", history: true });
  assert.equal((await get(client, "?view=document&id=bad")).status, 502);
  client.callTool = async () => ({ ok: false }); assert.equal((await get(client)).status, 502);
});
