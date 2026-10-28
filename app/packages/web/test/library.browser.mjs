import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";
import { createLibraryFeature } from "../../server/dist/features/library/index.js";
import { createHandler } from "../../server/dist/http/router.js";

test("Library renders fixture search, pages, outputs and setup on desktop and phone", { timeout: 60000 }, async (t) => {
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true }); t.after(() => browser.close());
  const history = { revisions: [{ commit: "abc", time: "2026-10-01T00:00:00Z", summary: "Updated" }], total: 1, truncated: false };
  const pageData = { id: "dom-person-alex", type: "domain", path: "knowledge/alex.md", template: "person", title: "Alex", aliases: [], summary: "A colleague", tags: [], cites: ["src-20261001-meeting#L1"], refs: [{ label: "Profile", target: "url", value: "https://example.org/", authoritative: false, as_of: null }], related: [], updated_at: null, last_verified: null, status: "current", sensitivity: "normal", as_of: null, performable: null, submission: null, body: "A colleague [src-20261001-meeting#L1]" };
  const client = { async callTool(name, input) {
    if (name === "office.knowledge-list") return { pages: [{ id: pageData.id, template: "person", title: "Alex", summary: "A colleague", status: "current", sensitivity: "normal" }], missing_templates: [{ template: "user", id: "dom-user", title: "You" }] };
    if (name === "office.artifact-list") return { artifacts: [{ id: "art-20261001-report", kind: "report", series: "weekly", title: "Weekly report", approved_at: "2026-10-01T00:00:00Z", sensitivity: "normal" }], total: 101, truncated: true };
    if (name === "office.bootstrap-status") return { phases: ["docs", "kt", "team", "process"].map((phase) => ({ phase, captured: 2, digested: 1, consolidating: 0, proposed: 0, accepted: 0, dismissed: 0, overflow_waiting: 0, gaps: phase === "docs" ? [{ kind: "subject_without_page", subject: "Planning", sources: ["src-20261001-meeting"] }] : [] })), pending_batches: [] };
    if (name === "office.search") return { count: 1, hits: [{ id: pageData.id, kind: "domain", title: "Alex", path: "knowledge/alex.md", snippet: "A colleague" }] };
    if (name === "office.knowledge-show") return { page: pageData, backlinks: [{ id: "src-20261001-meeting", kind: "source", title: "Planning meeting", path: "sources/meeting.md", via: ["cites"] }], history };
    if (name === "office.artifact-show") return { artifact: { id: input.id, kind: "report", title: "Weekly report", summary: "A report", files: [], task: "ORB-1", produced_by: { crew: null, runbook: null, at: null }, approved: { by: "user", at: "2026-10-01T00:00:00Z", edited: false }, cites: [], series: "weekly", version: 2, supersedes: "art-20260901-report", sensitivity: "normal", tags: [] }, path: "artifacts/report.md", files: [], series_history: [{ id: "art-20260901-report", version: 1, supersedes: null, path: "artifacts/old.md" }], backlinks: [], body: "Report body", truncated: false, history };
    throw new Error(`unexpected ${name}`);
  } };
  const route = createLibraryFeature(client).routes[0]; const fallback = createHandler();
  // Screens other than Today render only once the Library is ready; never ask the real Orbit.
  const server = createServer((request, response) => { if (request.url === "/api/setup") { response.writeHead(200, { "content-type": "application/json" }); response.end(JSON.stringify({ state: "ready" })); } else if (request.url.startsWith("/api/library")) void route.handle(request, response); else fallback(request, response); });
  t.after(() => new Promise((done) => server.close(done))); await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const page = await browser.newPage(); const errors = []; page.on("pageerror", (error) => errors.push(error.message));
  const dir = process.env.LIBRARY_SCREENSHOTS_DIR; if (dir) await mkdir(dir, { recursive: true });
  async function shot(name) { for (const width of [1440, 375]) { await page.setViewportSize({ width, height: 1000 }); assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)); if (dir) await page.screenshot({ path: resolve(dir, `${name}-light-${width}.png`), fullPage: true }); } }
  await page.goto(`http://127.0.0.1:${server.address().port}/#library`);
  await page.getByText("Showing the latest 100").waitFor(); assert.equal(await page.getByText("You — Not set up yet").count(), 1); assert.equal(await page.getByText("Planning — Needs a page.").count(), 1); await shot("library");
  await page.getByRole("button", { name: "Alex" }).click(); await page.getByText("Planning meeting").waitFor(); assert.equal(await page.getByText("Profile").count(), 1); assert.equal(await page.getByText("Task links are not available yet.").count(), 1); assert.equal(await page.getByText("1 changes").count(), 1); await shot("page");
  await page.getByRole("button", { name: "Library", exact: true }).click(); await page.getByRole("button", { name: "Weekly report" }).click(); await page.getByText("Version 1").waitFor(); await shot("artifact");
  await page.getByRole("button", { name: "Library", exact: true }).click(); await page.getByRole("searchbox").fill("Alex"); await page.getByRole("button", { name: "Search", exact: true }).click(); await page.getByRole("heading", { name: "Search results" }).waitFor(); await page.getByText("A colleague").waitFor(); await shot("search"); assert.deepEqual(errors, []);
});
