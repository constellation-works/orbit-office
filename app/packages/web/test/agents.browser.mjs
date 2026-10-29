/** Run with the verified host browser kit after `npm --prefix app run check`. */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";

import { createHandler } from "../../server/dist/http/router.js";

const at = (hour) => new Date(2026, 9, 26, hour).toISOString();
const task = (id, title, workspace, hour, extra = {}) => ({
  id, title, workspace, type: "feature", priority: "medium", crew: "muse-wonder",
  updated_at: at(hour), ...extra,
});

function fixture() {
  return {
    working_now: [task("ORB-1", "Office working task", "office", 13), task("ORB-10", "Alpha working task", "ws_alpha", 12)],
    in_review: [task("ORB-2", "Office review task", "office", 12, { priority: "high" })],
    queued: [
      task("ORB-3", "Queued one", "office", 11), task("ORB-12", "Queued two", "ws_alpha", 10),
      task("ORB-13", "Queued three", "ws_alpha", 9), task("ORB-14", "Queued four", "ws_beta", 8),
      task("ORB-15", "Queued five", "ws_beta", 7),
    ],
    done_today: [
      task("ORB-4", "Done one", "office", 13), task("ORB-21", "Done two", "ws_beta", 12),
      task("ORB-22", "Done three", "ws_beta", 11), task("ORB-23", "Done four", "ws_beta", 10),
    ],
    workspaces: ["office", "ws_alpha", "ws_beta"],
    unavailable: [],
    updated_at: at(14),
  };
}

test("Agent work shows counts, filter, collapsed groups and the Today link", { timeout: 30_000 }, async (t) => {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source the verified ORBIT_BROWSER_KIT_ENV first");
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const fallback = createHandler();
  const server = createServer((request, response) => {
    if ((request.url ?? "/").split("?")[0] === "/api/setup") {
      response.writeHead(200, { "content-type": "application/json" });
      response.end(JSON.stringify({ state: "ready" }));
    } else fallback(request, response);
  });
  t.after(() => new Promise((resolve) => server.close(resolve)));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const page = await browser.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  const origin = `http://127.0.0.1:${server.address().port}`;
  await page.goto(origin);
  await page.evaluate(async (data) => {
    const { createAgentsScreen } = await import("/screens/agents/index.js");
    window.agentsFixture = data;
    await createAgentsScreen({ load: async () => window.agentsFixture, pollIntervalMs: 60_000 }).render(document.getElementById("app"));
  }, fixture());

  await page.getByRole("heading", { name: "Agent work", exact: true }).waitFor();
  assert.match(await page.locator("main, #app").first().innerText(), /The Orbit tasks agents are carrying/);
  assert.equal(await page.getByRole("link", { name: "Open in Orbit" }).count(), 1);
  assert.match(await page.locator(".agents-updated").innerText(), /Updated/);

  const group = (name) => page.getByRole("region", { name, exact: true });
  // Summary counts equal the group sizes.
  const summary = page.getByRole("group", { name: "Agent work at a glance" });
  assert.match(await summary.innerText(), /Working now\s*2/);
  assert.match(await summary.innerText(), /In review\s*1/);
  assert.match(await summary.innerText(), /Queued\s*5/);
  assert.match(await summary.innerText(), /Done today\s*4/);
  assert.equal(await group("Working now").locator("article.agent-row").count(), 2);
  assert.equal(await group("In review").locator("article.agent-row").count(), 1);
  // Rows carry id · workspace, type, priority, crew and time since update.
  assert.match(await group("Working now").innerText(), /ORB-1 · office/);
  assert.match(await group("Working now").innerText(), /muse-wonder/);
  assert.match(await group("In review").innerText(), /high/);

  // Queued and Done today collapse behind "Show N more".
  assert.equal(await group("Queued").locator("article.agent-row").count(), 3);
  await group("Queued").getByRole("button", { name: "Show 2 more" }).click();
  assert.equal(await group("Queued").locator("article.agent-row").count(), 5);
  assert.equal(await group("Done today").locator("article.agent-row").count(), 3);
  await group("Done today").getByRole("button", { name: "Show 1 more" }).click();
  assert.equal(await group("Done today").locator("article.agent-row").count(), 4);

  // The workspace filter narrows both rows and counts.
  await page.getByRole("button", { name: "ws_alpha", exact: true }).click();
  assert.match(await summary.innerText(), /Working now\s*1/);
  assert.match(await summary.innerText(), /Queued\s*2/);
  assert.equal(await group("Working now").locator("article.agent-row").count(), 1);
  assert.match(await group("Working now").innerText(), /Alpha working task/);
  await page.getByRole("button", { name: "All workspaces", exact: true }).click();
  assert.equal(await group("Working now").locator("article.agent-row").count(), 2);

  // Proposed tasks live on Today, linked from here.
  const todayLink = page.locator("#app").getByRole("link", { name: "Today", exact: true });
  assert.equal(await todayLink.getAttribute("href"), "#today");

  const screenshotDir = process.env.AGENTS_SCREENSHOTS_DIR;
  if (screenshotDir) await mkdir(screenshotDir, { recursive: true });
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 1100 });
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth), `no horizontal overflow at ${width}px`);
    if (screenshotDir) await page.screenshot({ path: resolve(screenshotDir, `agents-light-${width}.png`), fullPage: true });
  }

  // Nothing at all renders the empty state.
  await page.evaluate(async () => {
    const { createAgentsScreen } = await import("/screens/agents/index.js");
    window.agentsFixture = {
      working_now: [], in_review: [], queued: [], done_today: [],
      workspaces: ["office", "ws_alpha"], unavailable: [], updated_at: new Date().toISOString(),
    };
    await createAgentsScreen({ load: async () => window.agentsFixture, pollIntervalMs: 60_000 }).render(document.getElementById("app"));
  });
  await page.getByRole("heading", { name: "No agent work right now" }).waitFor();
  assert.deepEqual(errors, []);
});
