/** Run with the verified host browser kit after `npm --prefix app run check`. */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";

import { createTodayFeature } from "../../server/dist/features/today/index.js";
import { createHandler } from "../../server/dist/http/router.js";
import { OrbitExitError } from "../../server/dist/orbit/client.js";
import { fakeTodayClient, now } from "../../server/test/today-fixture.mjs";

test("Today renders fake Orbit sections, results, empty states, failures and live updates at desktop and phone widths", { timeout: 30_000 }, async (t) => {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source the verified ORBIT_BROWSER_KIT_ENV first");
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const fake = fakeTodayClient();
  const today = createTodayFeature({ client: fake.client, now: () => now });
  const route = today.routes[0];
  const fallback = createHandler();
  const server = createServer((request, response) => {
    if (request.url === "/api/setup") { response.writeHead(200, { "content-type": "application/json" }); response.end(JSON.stringify({ state: "ready" })); }
    else if (request.url === "/api/today") void route.handle(request, response);
    // A calendar with nothing left today: the Next meeting card stays hidden.
    else if (request.url.startsWith("/api/calendar?")) {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ from: "", until: "", synced_at: now.toISOString(), sync_failed: false, prep_lead_minutes: 120, meetings: [], routines: [], routines_unavailable: false }));
    }
    else fallback(request, response);
  });
  t.after(() => new Promise((resolve) => server.close(resolve)));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.clock.install();
  const origin = `http://127.0.0.1:${server.address().port}`;
  await page.goto(origin);
  await page.getByRole("heading", { name: "Needs you", exact: true }).waitFor();
  const needs = page.getByRole("region", { name: "Needs you", exact: true });
  const working = page.getByRole("region", { name: "Agents working on", exact: true });
  const done = page.getByRole("region", { name: "Done today", exact: true });
  assert.equal(await needs.locator("article").count(), 3);
  assert.equal(await working.locator("article").count(), 1);
  assert.equal(await done.locator("article").count(), 1);
  for (const title of ["Turn meeting notes into a weekly brief", "The project brief is ready to read", "Send your feedback on the plan"]) {
    assert.equal(await needs.getByRole("heading", { name: title }).count(), 1);
  }
  assert.match(await working.innerText(), /Preparing the research notes/);
  assert.match(await done.innerText(), /A clearer plan for the week/);
  assert.equal(await page.locator('[data-slot="drop-zone"]').count(), 1);
  await page.locator('.today-next-slot[aria-busy="false"]').waitFor({ state: "attached" });
  assert.equal(await page.locator(".today-next").count(), 0);
  assert.equal(await needs.locator('[data-slot="approval-actions"]').count(), 2);
  await done.getByText("Open result", { exact: true }).click();
  await done.getByRole("link", { name: "About this work" }).click();
  assert.match(await done.innerText(), /The plan brings together the priorities/);
  assert.match(await done.innerText(), /From the planning meeting notes/);
  const visible = await page.locator("main").innerText();
  for (const internal of ["owner:agent", "owner:you", "in-progress", "suggestion-key", "completed-key"]) {
    assert.ok(!visible.includes(internal), `internal value ${internal} is not a display label`);
  }

  const screenshotDir = process.env.TODAY_SCREENSHOTS_DIR;
  if (screenshotDir) await mkdir(screenshotDir, { recursive: true });
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 1100 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `no horizontal overflow at ${width}px`);
    if (screenshotDir) await page.screenshot({ path: resolve(screenshotDir, `today-light-${width}.png`), fullPage: true });
  }

  // A changed snapshot keeps open results, background and keyboard focus.
  fake.state.tasks[3].title = "Finishing the research notes";
  await done.getByText("Open result", { exact: true }).focus();
  await page.clock.fastForward(5_000);
  await working.getByRole("heading", { name: "Finishing the research notes" }).waitFor();
  assert.equal(await done.locator("details").evaluate((element) => element.open), true);
  assert.equal(await done.getByRole("link").getAttribute("aria-expanded"), "true");
  assert.equal(await page.evaluate(() => document.activeElement.textContent), "Open result");

  fake.state.failure = new Error("private diagnostic");
  await page.clock.fastForward(5_000);
  await page.getByRole("alert").waitFor();
  assert.equal(await working.locator("article").count(), 1);
  assert.ok(!(await page.locator("main").innerText()).includes("private diagnostic"));
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 1100 });
    if (screenshotDir) await page.screenshot({ path: resolve(screenshotDir, `today-refresh-error-light-${width}.png`), fullPage: true });
  }
  fake.state.failure = null;
  fake.state.tasks = [];
  await page.clock.fastForward(5_000);
  await page.getByRole("heading", { name: "Nothing needs your attention.", exact: true }).waitFor();
  assert.equal(await page.getByRole("alert").count(), 0);
  assert.match(await working.innerText(), /No agent work is underway/);
  assert.match(await done.innerText(), /No completed work yet today/);
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 1100 });
    if (screenshotDir) await page.screenshot({ path: resolve(screenshotDir, `today-empty-light-${width}.png`), fullPage: true });
  }

  // A failed initial read must not look like an empty workspace, and recovers.
  fake.state.failure = new Error("unavailable");
  await page.reload();
  await page.getByRole("alert").waitFor();
  assert.equal(await page.getByRole("heading", { name: "Nothing needs your attention." }).count(), 0);
  fake.state.failure = null;
  await page.clock.fastForward(5_000);
  await page.getByRole("heading", { name: "Nothing needs your attention." }).waitFor();

  // Navigation between polls must prevent even one more request.
  await page.evaluate(async () => {
    const { createTodayScreen } = await import("/screens/today/index.js");
    window.todayLoadCount = 0;
    await createTodayScreen({ load: async () => {
      window.todayLoadCount += 1;
      return { needs_you: [], agents_working_on: [], done_today: [], truncated: false, completion_dates_unavailable: 0 };
    } }).render(document.getElementById("app"));
    document.getElementById("app").textContent = "Another view";
  });
  await page.clock.fastForward(10_000);
  assert.equal(await page.evaluate(() => window.todayLoadCount), 1);

  // A pending response cannot repaint after navigation, or keep polling.
  await page.evaluate(async () => {
    const { createTodayScreen } = await import("/screens/today/index.js");
    let finish;
    window.todayLoadCount = 0;
    window.todayPendingRender = createTodayScreen({ load: () => {
      window.todayLoadCount += 1;
      return new Promise((resolve) => { finish = resolve; });
    } }).render(document.getElementById("app"));
    document.getElementById("app").textContent = "Another view";
    finish({ needs_you: [], agents_working_on: [], done_today: [], truncated: false, completion_dates_unavailable: 0 });
    await window.todayPendingRender;
  });
  await page.clock.fastForward(10_000);
  assert.equal(await page.locator("main").innerText(), "Another view");
  assert.equal(await page.evaluate(() => window.todayLoadCount), 1);
  assert.deepEqual(errors, []);
});

/** Orbit behind both Today and Approvals: accepted, dismissed and approved tasks leave Needs you. */
function approvalOrbit(fake) {
  const proposal = (title) => JSON.stringify({ kind: "knowledge_add", markdown: `# ${title}\n\nWhat the team agreed, with the sources it came from.` });
  const draft = JSON.stringify({
    schema_version: 1, id: "gd-0123456789abcdef", connection: "acme-crm",
    request: { method: "POST", url: "https://api.acme.example/v2/accounts/acme/notes", headers: {}, body: "{\"note\":\"Following up on Tuesday's call.\"}" },
    sha256: "b".repeat(64), created_at: "2026-10-27T00:00:00.000Z",
  });
  const files = { review: ["brief.md", "artifact-manifest.json"], script: ["receipts.csv", "summary.md", "artifact-manifest.json", "script-run.json"] };
  // `deny` refuses the app's authority the way installed Orbit does: a nonzero exit and its stderr line.
  const state = { calls: [], refuse: {}, replies: {}, deny: new Set() };
  const find = (id) => fake.state.tasks.find((row) => row.id === id);
  state.client = {
    async callTool(name, input) {
      state.calls.push({ name, input });
      if (state.deny.has(name)) {
        throw new OrbitExitError(name, 1, null, JSON.stringify({ error: "capability denied: operation 'plugin.tool.mutating' requires the operator or runner capability", code: "capability_denied" }));
      }
      const refusal = state.refuse[`${name} ${input.id ?? input.task}`] ?? state.refuse[name];
      if (refusal) return { ok: false, error: { code: refusal, message: "fixture", retryable: false } };
      const task = find(input.id ?? input.task);
      switch (name) {
        case "orbit.task.show":
          return { ...task, artifacts: (files[task.id] ?? (task.tags.includes("gateway-draft") ? ["gateway-draft.json"] : ["proposal.json"])).map((path) => ({ path })) };
        case "orbit.task.update":
          if (input.status) task.status = input.status;
          return task;
        case "orbit.task.artifact.get":
          return { content: input.path === "gateway-draft.json" ? draft : proposal(task.title) };
        case "office.artifact-promote":
          return { ok: true, output: { id: "art", path: "p", files: [], series: "s", version: 1, supersedes: null } };
        case "office.proposal-accept":
          task.status = "done";
          return { ok: true, output: { id: task.id, accepted: true, change: { page: "p", path: "p" } } };
        case "gateway.http-send":
          return state.replies[name] ?? { outcome: "sent", status: 201, task_closed: true };
        default:
          throw new Error(`unexpected ${name}`);
      }
    },
  };
  return state;
}

test("Needs you approves, sends back, accepts and dismisses at desktop and phone widths", { timeout: 60_000 }, async (t) => {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source the verified ORBIT_BROWSER_KIT_ENV first");
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const { createApprovalsFeature } = await import("../../server/dist/features/approvals/index.js");
  const { approvalTasks } = await import("../../server/test/today-fixture.mjs");
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const fake = fakeTodayClient(approvalTasks());
  const orbit = approvalOrbit(fake);
  const today = createTodayFeature({ client: fake.client, now: () => now });
  const approvals = createApprovalsFeature({ client: orbit.client, approver: () => "Dana" });
  const fallback = createHandler();
  const server = createServer((request, response) => {
    const path = (request.url ?? "/").split("?")[0];
    const route = [...today.routes, ...approvals.routes].find((item) => item.method === request.method && item.path === path);
    if (path === "/api/setup") { response.writeHead(200, { "content-type": "application/json" }); response.end(JSON.stringify({ state: "ready" })); }
    else if (route) void route.handle(request, response);
    else fallback(request, response);
  });
  t.after(() => new Promise((resolve) => server.close(resolve)));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.clock.install();
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  const needs = page.getByRole("region", { name: "Needs you", exact: true });
  await needs.getByRole("heading", { name: "Set up your Library" }).waitFor();
  const cardOf = (title) => needs.locator("article.card").filter({ has: page.getByRole("heading", { name: title, exact: true }) });

  // One card per group; each page is reviewable on its own.
  const onboarding = cardOf("Set up your Library");
  assert.equal(await onboarding.locator(".approval-page").count(), 2);
  assert.equal(await cardOf("Ingest service").locator(".approval-page").count(), 2);
  assert.equal(await cardOf("Pages about your processes and policies").locator(".approval-page").count(), 1);
  assert.equal(await needs.locator("article.card").count(), 8);
  await onboarding.getByText("Review this page").first().click();
  await onboarding.getByText("What the team agreed").first().waitFor();

  // The draft shows exactly what would be sent; script and review cards list their files.
  const draft = cardOf("Add a follow-up note to the Acme account");
  await draft.getByText("https://api.acme.example/v2/accounts/acme/notes").waitFor();
  assert.match(await draft.innerText(), /POST/);
  assert.match(await draft.innerText(), /Following up on Tuesday's call\./);
  await cardOf("Receipt catalog for October").getByText("receipts.csv").waitFor();
  assert.equal(await cardOf("Receipt catalog for October").getByText("script-run.json").count(), 0);
  await cardOf("The project brief is ready to read").getByText("brief.md").waitFor();
  assert.equal(await cardOf("Send your feedback on the plan").getByRole("button").count(), 0);

  const visible = async () => page.locator("main").innerText();
  for (const internal of ["onboard-1", "boot-1", "bootstrap:", "subject:", "gd-0123", "script-output", "gateway-draft", "proposal:", "bbbbbbbb"]) {
    assert.ok(!(await visible()).includes(internal), `internal value ${internal} is not shown`);
  }
  const screenshotDir = process.env.TODAY_SCREENSHOTS_DIR;
  if (screenshotDir) await mkdir(screenshotDir, { recursive: true });
  const shoot = async (name) => {
    for (const width of [1440, 375]) {
      await page.setViewportSize({ width, height: 1100 });
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `no horizontal overflow at ${width}px`);
      if (screenshotDir) await page.screenshot({ path: resolve(screenshotDir, `${name}-light-${width}.png`), fullPage: true });
    }
  };
  await shoot("today-needs-you");

  // A refused promote leaves the work in review and says so.
  const review = cardOf("The project brief is ready to read");
  orbit.refuse["office.artifact-promote"] = "task_has_no_artifacts";
  await review.getByRole("button", { name: "Approve" }).click();
  await review.getByRole("alert").waitFor();
  assert.match(await review.getByRole("alert").innerText(), /still waiting for your review\. There is no output to add yet\./);
  assert.equal(fake.state.tasks.find((row) => row.id === "review").status, "review");
  await shoot("today-approve-refused");

  // Send back needs a comment and carries it.
  await review.getByRole("button", { name: "Send back" }).click();
  await review.getByLabel("What should change?").fill("Add the budget table.");
  await review.getByRole("button", { name: "Send back" }).click();
  await page.getByText("Sent “The project brief is ready to read” back with your comment.").waitFor();
  const sentBack = orbit.calls.find(({ name, input }) => name === "orbit.task.update" && input.status === "backlog");
  assert.match(sentBack.input.comment, /Add the budget table\./);

  // Accept all: one refused page stays, the other is accepted and the card names the refused one.
  orbit.refuse["office.proposal-accept onboard-2"] = "proposal_closed";
  await onboarding.getByRole("button", { name: "Accept all" }).click();
  await onboarding.getByText(/Accepted 1 of 2 pages\. “Your goals this quarter” couldn’t be accepted/).waitFor();
  assert.deepEqual(orbit.calls.filter(({ name }) => name === "office.proposal-accept").map(({ input }) => input.id), ["onboard-1", "onboard-2"]);
  await shoot("today-accept-all-partial");
  await page.clock.fastForward(5_000);
  await page.waitForFunction(() => document.querySelectorAll('[data-group="onboarding"] .approval-page').length === 1);

  // Orbit refusing the app's authority is said plainly, never "Please try again", and nothing changes.
  orbit.deny.add("office.proposal-accept");
  const ingest = cardOf("Ingest service");
  const overview = ingest.locator('.approval-page[data-key="boot-1"]');
  await overview.getByRole("button", { name: "Accept", exact: true }).click();
  await overview.getByRole("alert").waitFor();
  const denied = await overview.getByRole("alert").innerText();
  assert.match(denied, /^Couldn’t accept “Ingest service: overview”\. The app isn’t allowed to do this yet, so nothing changed\. Trying again won’t help\.$/);
  assert.doesNotMatch(denied, /Please try again/);
  await ingest.getByRole("button", { name: "Accept all" }).click();
  await ingest.getByText(/Accepted 0 of 2 pages\. .* still here to review\. The app isn’t allowed to do this yet, so nothing changed\./).waitFor();
  orbit.deny.add("orbit.task.update");
  const policies = cardOf("Pages about your processes and policies");
  await policies.getByRole("button", { name: "Dismiss", exact: true }).click();
  await policies.getByText("Couldn’t dismiss this. The app isn’t allowed to do this yet, so nothing changed. Trying again won’t help.").waitFor();
  for (const id of ["boot-1", "boot-2", "boot-3"]) assert.equal(fake.state.tasks.find((row) => row.id === id).status, "proposed");
  await shoot("today-not-permitted");
  orbit.deny.clear();

  // A lost gateway reply is reported as unknown, not as a failure.
  orbit.replies["gateway.http-send"] = { outcome: "outcome_unknown", status: null, task_closed: false };
  await draft.getByRole("button", { name: "Approve and send" }).click();
  await draft.getByText(/no reply came back/).first().waitFor();
  assert.equal(await draft.getByRole("alert").count(), 0);
  const sends = orbit.calls.filter(({ name }) => name === "gateway.http-send");
  assert.deepEqual(sends.map(({ input }) => input), [{ draft: "gd-0123456789abcdef", sha256: "b".repeat(64) }]);
  await shoot("today-send-outcome-unknown");

  // Script output: Approve promotes and closes; a single proposal can be dismissed.
  delete orbit.refuse["office.artifact-promote"];
  const script = cardOf("Receipt catalog for October");
  await script.getByRole("button", { name: "Approve" }).click();
  await page.getByText("Added the files from “Receipt catalog for October” to your Library.").waitFor();
  assert.equal(fake.state.tasks.find((row) => row.id === "script").status, "done");
  await cardOf("Turn meeting notes into a weekly brief").getByRole("button", { name: "Dismiss" }).click();
  await page.getByText("Dismissed “Turn meeting notes into a weekly brief”.").waitFor();
  assert.equal(fake.state.tasks.find((row) => row.id === "suggestion").status, "rejected");
  await page.clock.fastForward(5_000);
  await page.waitForFunction(() => !document.body.innerText.includes("Receipt catalog for October"));
  assert.deepEqual(errors, []);
});
