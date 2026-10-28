import assert from "node:assert/strict";
import { Readable } from "node:stream";
import { test } from "node:test";
import { createLibraryFeature } from "../dist/features/library/index.js";
import { OrbitExitError } from "../dist/orbit/client.js";

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

async function share(client, body) {
  const route = createLibraryFeature(client).routes.find((item) => item.path === "/api/library/share");
  const request = Object.assign(Readable.from([Buffer.from(JSON.stringify(body))]), { url: "/api/library/share", method: "POST" });
  const response = { writeHead(status) { this.status = status; }, end(text) { this.body = JSON.parse(text); } };
  await route.handle(request, response); return response;
}

/** `office.share-set` that refuses the page until its cited documents are shareable. */
function sharing() {
  const calls = [];
  const shared = new Set();
  const titles = { "src-20261028-lantern-kt": "Lantern KT", "art-20261001-report": "Weekly report", "rb-lantern-spool": "When the Lantern spool stalls" };
  return { calls, shared, async callTool(name, input) {
    calls.push({ name, input });
    if (name === "office.share-set") {
      if (input.id === "src-20261028-private") return { ok: false, error: { code: "share_private_refused", message: "sensitivity is private" } };
      if (input.id === "dom-missing") throw new OrbitExitError("office.share-set", 1, null, `${JSON.stringify({ error: { code: "document_not_found", message: "dom-missing" } })}\n`);
      const missing = input.id === "rb-lantern-spool" && input.shareable
        ? ["art-20261001-report", "src-20261028-lantern-kt"].filter((id) => !shared.has(id)) : [];
      if (missing.length > 0) return { ok: false, error: { code: "share_cites_unshared", message: `cites unshared own documents: ${missing.join(", ")}, ../../etc/passwd` } };
      if (input.shareable) shared.add(input.id); else shared.delete(input.id);
      return { ok: true, output: { id: input.id, path: `knowledge/${input.id}.md`, shareable: input.shareable } };
    }
    if (name === "office.source-show") return { ok: true, output: { id: input.id, title: titles[input.id] } };
    if (name === "office.artifact-show") return { ok: true, output: { artifact: { id: input.id, title: titles[input.id] } } };
    if (name === "office.knowledge-show") return { ok: true, output: { page: { id: input.id, title: titles[input.id] } } };
    throw new Error("unexpected call");
  } };
}

test("Share with the team calls office.share-set; a citation refusal lists the documents to share first, sources first", async () => {
  const client = sharing();
  const refused = await share(client, { id: "rb-lantern-spool", shareable: true });
  assert.equal(refused.status, 409);
  assert.deepEqual(refused.body, { error: "share_refused", reason: "share_cites_unshared", documents: [
    { id: "src-20261028-lantern-kt", title: "Lantern KT" },
    { id: "art-20261001-report", title: "Weekly report" },
  ] });
  assert.ok(!JSON.stringify(refused.body).includes("passwd"));

  client.calls.length = 0;
  const shared = await share(client, { id: "rb-lantern-spool", shareable: true, also: ["art-20261001-report", "src-20261028-lantern-kt"] });
  assert.equal(shared.status, 200);
  assert.deepEqual(shared.body, { shareable: true });
  assert.deepEqual(client.calls.map((call) => call.input), [
    { id: "src-20261028-lantern-kt", shareable: true },
    { id: "art-20261001-report", shareable: true },
    { id: "rb-lantern-spool", shareable: true },
  ]);

  const stopped = await share(client, { id: "rb-lantern-spool", shareable: false });
  assert.equal(stopped.status, 200);
  assert.deepEqual(client.calls.at(-1).input, { id: "rb-lantern-spool", shareable: false });
});

test("Share with the team passes refusals on as their reason and refuses bad input before any call", async () => {
  const client = sharing();
  const privateSource = await share(client, { id: "src-20261028-private", shareable: true });
  assert.equal(privateSource.status, 409);
  assert.deepEqual(privateSource.body, { error: "share_refused", reason: "share_private_refused" });
  const missing = await share(client, { id: "dom-missing", shareable: true });
  assert.equal(missing.status, 404);
  assert.deepEqual(missing.body, { error: "share_refused", reason: "document_not_found" });
  client.calls.length = 0;
  for (const body of [
    { id: "alice/rb-lantern-spool", shareable: true },
    { id: "rb-lantern-spool", shareable: "yes" },
    { id: "rb-lantern-spool", shareable: false, also: ["src-20261028-lantern-kt"] },
    { id: "rb-lantern-spool", shareable: true, also: ["rb-lantern-spool"] },
    { id: "rb-lantern-spool", shareable: true, extra: 1 },
  ]) {
    const response = await share(client, body);
    assert.equal(response.status, 400, JSON.stringify(body));
    assert.deepEqual(response.body, { error: "invalid_input" });
  }
  assert.deepEqual(client.calls, []);
});

test("the Library overview says whether a shared library is connected, and a colleague's document opens by its qualified id", async () => {
  const client = fixture();
  let shared = null;
  const base = client.callTool;
  client.callTool = async (name, input) => {
    if (name === "office.status") return { ok: true, output: { initialized: true, shared } };
    if (name === "office.knowledge-show" && input.id === "alice/rb-lantern-spool") return { ok: true, output: { page: { id: input.id, title: "Spool", layer: "shared", shared_by: "alice" }, backlinks: [], history } };
    return base(name, input);
  };
  assert.deepEqual((await get(client)).body.shared, { configured: false });
  shared = { remote: "/srv/team.git", branch: "main", member: "daniel" };
  const configured = await get(client);
  assert.deepEqual(configured.body.shared, { configured: true });
  assert.ok(!JSON.stringify(configured.body).includes("/srv/team.git"));
  const page = await get(client, "?view=document&id=alice%2Frb-lantern-spool");
  assert.equal(page.status, 200);
  assert.equal(page.body.page.shared_by, "alice");
  client.callTool = async (name, input) => name === "office.status" ? { ok: false, error: { code: "refused" } } : base(name, input);
  assert.equal((await get(client)).body.shared, null);
});
