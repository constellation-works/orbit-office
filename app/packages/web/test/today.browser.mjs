/** Run with the verified host browser kit after `npm --prefix app run check`. */
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";

import { createTodayFeature } from "../../server/dist/features/today/index.js";
import { createHandler } from "../../server/dist/http/router.js";
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
    if (request.url === "/api/today") void route.handle(request, response);
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
