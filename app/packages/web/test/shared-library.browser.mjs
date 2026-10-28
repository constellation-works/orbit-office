/**
 * The shared library across screens: Settings › Shared library, the Library's
 * labels and **Share with the team** switch, and the Needs-you card. Run after
 * the app build with the verified host browser kit. Every Orbit call is
 * answered in memory: no real HOME, workspace, git or network.
 */
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";

import { createApprovalsFeature } from "../../server/dist/features/approvals/index.js";
import { createLibraryFeature } from "../../server/dist/features/library/index.js";
import { createSettingsFeature } from "../../server/dist/features/settings/index.js";
import { createTodayFeature } from "../../server/dist/features/today/index.js";
import { checkLoopbackRequest } from "../../server/dist/http/guard.js";
import { createHandler } from "../../server/dist/http/router.js";
import { fakeTodayClient, now, sharedTasks } from "../../server/test/today-fixture.mjs";

const history = { revisions: [], total: 0, truncated: false };
const UPDATE = JSON.stringify({ kind: "knowledge_update", page: "rb-lantern-spool", markdown: "# When the Lantern spool stalls\n\nRestart the spooler, then drain the queue.", shareable: true });
const CONFLICT = JSON.stringify({ kind: "shared_conflict", page: "dom-client-acme", shared_path: "knowledge/clients/dom-client-acme.md", shared_blob: "b".repeat(40), markdown: "# Acme\n\nBob's version of the account notes.", own_sha256: "c".repeat(64) });
const sha = (text) => createHash("sha256").update(text).digest("hex");

/** One in-memory Orbit for every feature, recording each call. */
function orbit(todayTasks) {
  const state = { shared: null, routineOn: false, shareable: new Set(["dom-client-acme"]), calls: [], syncs: 0 };
  const pages = () => [
    { id: "dom-client-acme", template: "client", title: "Acme", layer: "own", shared_by: null, shareable: state.shareable.has("dom-client-acme") },
    { id: "alice/dom-client-globex", template: "client", title: "Globex", layer: "shared", shared_by: "alice", shareable: false },
    { id: "rb-lantern-spool", template: "runbook", title: "When the Lantern spool stalls", layer: "own", shared_by: null, shareable: state.shareable.has("rb-lantern-spool") },
  ];
  const tasks = { "shared-update": UPDATE, "shared-conflict": CONFLICT };
  async function callTool(name, input) {
    state.calls.push({ name, input: structuredClone(input) });
    switch (name) {
      case "office.status": return { ok: true, output: { initialized: true, shared: state.shared } };
      case "office.config-set":
        if (/:[^@/]+@/.test(input.value.remote)) return { ok: false, error: { code: "invalid_library_setting", message: `shared.remote carries credentials: ${input.value.remote}` } };
        state.shared = input.value;
        return { ok: true, output: { path: ".office/config.yaml", key: "shared" } };
      case "orbit.routine.control":
        if (input.action === "toggle") { state.routineOn = input.enabled; return { name: input.name, enabled: input.enabled }; }
        return { items: [{ name: "office-shared-sync", description: "Sync the shared library", enabled: state.routineOn, effective: state.routineOn, target: "plugin:office", schedule: { every_minutes: 60 } }], pagination: { next_offset: null }, notes: [] };
      case "office.library-sync":
        state.syncs += 1;
        if (state.syncs === 1) return { ok: false, error: { code: "shared_remote_unreachable", message: "could not reach git.example" } };
        return { ok: true, output: {
          direction: "both", dry_run: false,
          pulled: ["knowledge/runbooks/rb-lantern-spool.md", "knowledge/clients/dom-client-acme.md", "knowledge/clients/dom-client-globex.md"],
          pushed: ["knowledge/clients/dom-client-acme.md", "members/daniel/sources/2026/10/src-20261028-lantern-kt/source.md", "members/daniel/sources/2026/10/src-20261028-lantern-kt/v1.md"],
          removed: [],
          incoming: [{ page: "rb-lantern-spool", proposal: "shared-update", created: true }],
          conflicts: [{ page: "dom-client-acme", proposal: "shared-conflict", created: true }],
          skipped: [{ phase: "push", path: "knowledge/topics/dom-scratch-notes.md", id: "dom-scratch-notes", reason: "share_private_refused" }],
          commit: "0123456789abcdef0123456789abcdef01234567", synced_at: now.toISOString(),
        } };
      case "office.knowledge-list": return { ok: true, output: { pages: pages(), missing_templates: [] } };
      case "office.artifact-list": return { ok: true, output: { artifacts: [{ id: "art-20261001-report", kind: "report", series: "weekly", title: "Weekly report", approved_at: "2026-10-01T00:00:00Z", sensitivity: "normal", shareable: false }], total: 1, truncated: false } };
      case "office.bootstrap-status": return { ok: true, output: { phases: [], pending_batches: [] } };
      case "office.knowledge-show": {
        const row = pages().find((item) => item.id === input.id);
        return { ok: true, output: { page: { ...row, summary: "What to do when the spooler stops draining.", body: "Restart the spooler [src-20261028-lantern-kt].", cites: ["src-20261028-lantern-kt"], sensitivity: "normal", shareable: row.shareable || undefined }, backlinks: [], history } };
      }
      case "office.source-show": return { ok: true, output: { id: input.id, title: "Lantern KT walkthrough" } };
      case "office.share-set":
        if (input.id === "rb-lantern-spool" && input.shareable && !state.shareable.has("src-20261028-lantern-kt")) {
          return { ok: false, error: { code: "share_cites_unshared", message: "cites unshared own documents: src-20261028-lantern-kt" } };
        }
        if (input.shareable) state.shareable.add(input.id); else state.shareable.delete(input.id);
        return { ok: true, output: { id: input.id, path: `knowledge/${input.id}.md`, shareable: input.shareable } };
      case "orbit.task.show": {
        const task = todayTasks.find((row) => row.id === input.id);
        return { ...structuredClone(task), artifacts: [{ path: "proposal.json" }] };
      }
      case "orbit.task.artifact.get": return { id: input.id, path: input.path, encoding: "utf8", content: tasks[input.id] };
      case "office.proposal-accept": {
        todayTasks.find((row) => row.id === input.id).status = "done";
        return { ok: true, output: { id: input.id, accepted: true, change: { page: "dom-client-acme", path: "knowledge/clients/dom-client-acme.md" } } };
      }
      default: throw new Error(`unexpected ${name}`);
    }
  }
  return { state, client: { callTool } };
}

test("Shared library browser: connect in Settings, share a page with its source, and settle the sync in Needs you", { timeout: 60_000 }, async (t) => {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source ORBIT_BROWSER_KIT_ENV first");
  const today = fakeTodayClient(sharedTasks());
  const fake = orbit(today.state.tasks);
  const features = [
    createSettingsFeature(fake.client, {}, { syncClient: fake.client }),
    createLibraryFeature(fake.client),
    createTodayFeature({ client: today.client, now: () => now }),
    createApprovalsFeature({ client: fake.client, approver: () => "Dana" }),
  ];
  const fallback = createHandler();
  const json = (response, body) => { response.writeHead(200, { "content-type": "application/json" }); response.end(JSON.stringify(body)); };
  const server = createServer((request, response) => {
    if (!checkLoopbackRequest(request).ok) { response.writeHead(403); response.end(); return; }
    const path = (request.url ?? "/").split("?")[0];
    if (path === "/api/setup") { json(response, { state: "ready" }); return; }
    if (path === "/api/settings") { json(response, { agentCli: { cli: "Muse Code", connected: true } }); return; }
    if (path === "/api/calendar") {
      json(response, { from: "", until: "", synced_at: now.toISOString(), sync_failed: false, prep_lead_minutes: 120, meetings: [], routines: [], routines_unavailable: false });
      return;
    }
    const route = features.flatMap((feature) => feature.routes).find((item) => item.method === request.method && item.path === path);
    if (route) void route.handle(request, response); else fallback(request, response);
  });
  t.after(() => new Promise((done) => { server.closeAllConnections(); server.close(done); }));
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const page = await browser.newPage();
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const origin = `http://127.0.0.1:${server.address().port}`;
  const dir = process.env.SHARED_LIBRARY_SCREENSHOTS_DIR;
  if (dir) await mkdir(dir, { recursive: true });
  async function shot(name) {
    for (const width of [1440, 375]) {
      await page.setViewportSize({ width, height: 1000 });
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `no overflow at ${width}px (${name})`);
      // The app has one dark theme (docs/design/office/specs/web-app.md, "Look").
      if (dir) await page.screenshot({ path: resolve(dir, `${name}-dark-${width}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 1440, height: 1000 });
  }
  const visibleText = () => page.locator("main").innerText();
  const noInternals = async () => {
    const text = await visibleText();
    for (const internal of ["src-2026", "rb-lantern", "dom-client", "shared_remote_unreachable", "share_cites_unshared", "proposal:", "shared-library", "office."]) {
      assert.ok(!text.includes(internal), `no internal value ${internal} is shown`);
    }
  };

  // A Library with no shared library connected: the first setup step connects one.
  await page.goto(`${origin}/#library`);
  const connect = page.getByRole("link", { name: "Connect a shared library" });
  await connect.waitFor();
  assert.equal(await page.getByText("Shared · from Alice", { exact: true }).count(), 1);
  assert.equal(await page.locator(".library-entry", { hasText: "Acme" }).getByText("Shared", { exact: true }).count(), 1);
  assert.equal(await page.locator(".library-entry", { hasText: "When the Lantern spool stalls" }).locator(".pill").count(), 0);
  await connect.click();

  // Settings › Shared library, opened by the deep link.
  const section = page.getByRole("region", { name: "Shared library", exact: true });
  await section.getByRole("heading", { name: "Shared library", exact: true }).waitFor();
  assert.equal(await page.getByRole("button", { name: "Shared library", exact: true }).getAttribute("aria-pressed"), "true");
  assert.equal(new URL(page.url()).search, "", "the deep link's query is dropped once read");
  assert.equal(await section.getByRole("button", { name: "Sync now" }).isDisabled(), true);
  const secret = "https://daniel:hunter2@git.example/team-library.git";
  await section.getByLabel("Shared library address").fill(secret);
  await section.getByLabel("Your name in the shared library").fill("daniel");
  await section.getByRole("button", { name: "Save shared library" }).click();
  const feedback = section.locator(".settings-feedback");
  await feedback.filter({ hasText: "Check the shared library address" }).waitFor();
  assert.ok(!(await feedback.innerText()).includes("hunter2"));
  assert.equal(await section.getByLabel("Shared library address").getAttribute("aria-invalid"), "true");

  await section.getByLabel("Shared library address").fill("git@github.com:acme-team/team-library.git");
  await section.getByRole("button", { name: "Save shared library" }).click();
  await feedback.filter({ hasText: "Shared library saved." }).waitFor();
  assert.deepEqual(fake.state.shared, { remote: "git@github.com:acme-team/team-library.git", branch: "main", member: "daniel" });
  await section.getByRole("button", { name: "Turn on hourly sync" }).click();
  await section.getByText("Hourly sync is on.").waitFor();
  assert.equal(fake.state.routineOn, true);

  const status = section.locator(".settings-sync-status");
  await section.getByRole("button", { name: "Sync now" }).click();
  await status.filter({ hasText: "The shared library couldn't be reached." }).waitFor();
  await section.getByRole("button", { name: "Sync now" }).click();
  await status.filter({ hasText: "Shared 2, received 3, 3 need you." }).waitFor();
  await noInternals();
  await shot("settings-shared-library");

  // The Library: the connect step is gone; sharing a page offers its source first.
  await page.goto(`${origin}/#library`);
  await page.getByRole("heading", { name: "How-to" }).waitFor();
  assert.equal(await page.getByRole("link", { name: "Connect a shared library" }).count(), 0);
  await page.getByRole("button", { name: "When the Lantern spool stalls" }).click();
  const share = page.getByRole("switch", { name: "Share with the team" });
  await share.waitFor();
  assert.equal(await share.isChecked(), false);
  await share.click();
  await page.getByText("Lantern KT walkthrough", { exact: true }).waitFor();
  assert.equal(await share.isChecked(), false, "nothing is shared until the cited source is");
  await page.getByRole("button", { name: "Share these too" }).click();
  await page.getByText("Shared, with the document it cites.", { exact: false }).waitFor();
  assert.equal(await share.isChecked(), true);
  assert.equal(await page.locator(".library-sharing").getByText("Shared", { exact: true }).count(), 1);
  assert.deepEqual(fake.state.calls.filter(({ name }) => name === "office.share-set").map(({ input }) => input), [
    { id: "rb-lantern-spool", shareable: true },
    { id: "src-20261028-lantern-kt", shareable: true },
    { id: "rb-lantern-spool", shareable: true },
  ]);
  await noInternals();
  await shot("library-shared-page");

  // Stopping is confirmed first, with what colleagues keep.
  const before = fake.state.calls.length;
  await share.click();
  await page.getByText("Colleagues who already synced keep their copy", { exact: false }).waitFor();
  assert.equal(await share.isChecked(), true);
  await page.getByRole("button", { name: "Keep sharing" }).click();
  assert.equal(fake.state.calls.length, before, "keeping it shared makes no call");
  await share.click();
  await page.getByRole("button", { name: "Stop sharing" }).click();
  await page.getByText("Stopped sharing.", { exact: false }).waitFor();
  assert.equal(await share.isChecked(), false);
  assert.deepEqual(fake.state.calls.at(-1), { name: "office.share-set", input: { id: "rb-lantern-spool", shareable: false } });

  // Needs you: one card for the sync's update, conflict and skipped document.
  await page.goto(`${origin}/#today`);
  const card = page.locator('[data-group="shared-library"]');
  await card.getByRole("heading", { name: "Changes from the shared library" }).waitFor();
  assert.equal(await page.locator('[data-group="shared-library"]').count(), 1);
  await card.getByText("Scratch notes", { exact: true }).waitFor();
  assert.match(await card.innerText(), /wasn’t shared\. It’s marked private/);
  await card.getByText("Review the update").click();
  await card.getByText("Will be shared with the team", { exact: true }).waitFor();
  await noInternals();
  await shot("today-shared-library");

  await card.getByRole("button", { name: "Keep mine" }).click();
  await page.locator(".today-outcome").filter({ hasText: "Kept your version of “Acme”." }).waitFor();
  const accepted = fake.state.calls.filter(({ name }) => name === "office.proposal-accept");
  assert.deepEqual(accepted.map(({ input }) => input), [{ id: "shared-conflict", sha256: sha(CONFLICT), choice: "mine" }]);
  await card.getByRole("button", { name: "Got it" }).click();
  await page.locator(".today-outcome").filter({ hasText: "Cleared the documents the last sync left out." }).waitFor();
  await card.getByRole("button", { name: "Accept" }).waitFor();
  assert.equal(await card.getByText("Scratch notes", { exact: true }).count(), 0);
  await card.getByRole("button", { name: "Accept" }).click();
  await page.locator(".today-outcome").filter({ hasText: "Accepted the update to" }).waitFor();
  assert.deepEqual(fake.state.calls.filter(({ name }) => name === "office.proposal-accept").at(-1).input, { id: "shared-update", sha256: sha(UPDATE) });
  await page.locator('[data-group="shared-library"]').waitFor({ state: "detached", timeout: 15_000 });
  assert.deepEqual(errors, []);
});
