import assert from "node:assert/strict";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";
import { createRoutinesFeature } from "../../server/dist/features/routines/index.js";
import { createHandler } from "../../server/dist/http/router.js";

test("distribution routines render, toggle and explain unavailable status at desktop and phone widths", { timeout: 60000 }, async t => {
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const auto = (name, description, enabled) => ({ name, description, effective_enabled: enabled, schedule: { every_minutes: 60 } });
  const autos = [
    auto("office-review", "Write a cited review at the end of each quarter.", false),
    auto("office-knowledge-maintenance", "Keep Library knowledge current and useful.", true),
    ...Array.from({ length: 22 }, (_, index) => auto(`developer-task-${index}`, "Developer maintenance", false)),
  ];
  const routine = (name, description) => ({ name, description, enabled: true, effective: true, schedule: { cron: "5 0 * * *" }, target: `job:${name}`, last_fire: null });
  const jobs = [
    routine("office-daily-digest", "Summarize the previous day's work with citations."),
    routine("office-signals", "Find opportunities to automate repeatable work."),
    ...Array.from({ length: 13 }, (_, index) => routine(`developer-routine-${index}`, "Developer maintenance")),
  ];
  let denied = false;
  const client = { async callTool(name, input) {
    if (name === "orbit.auto_task.list") return autos;
    if (name === "orbit.auto_task.update") {
      autos.find(row => row.name === input.name).effective_enabled = input.enabled;
      return {};
    }
    assert.equal(name, "orbit.routine.control");
    if (denied) throw Object.assign(new Error("operator required"), { code: "not_permitted" });
    if (input.action === "toggle") {
      const row = jobs.find(row => row.name === input.name);
      assert.equal(input.expected_enabled, row.enabled);
      assert.equal(input.target, row.target);
      row.enabled = input.enabled; row.effective = input.enabled;
      return {};
    }
    return { items: jobs, pagination: { next_offset: null }, notes: [] };
  } };
  const feature = createRoutinesFeature(client);
  const fallback = createHandler();
  const server = createServer((req, res) => {
    const route = feature.routes.find(route => route.path === req.url && route.method === req.method);
    if (route) { void route.handle(req, res); return; }
    // Every other API is a fixture: the browser never reaches the user's Orbit.
    if (req.url.startsWith("/api/") && !req.url.endsWith(".js")) {
      const value = req.url === "/api/setup" ? { state: "ready" }
        : req.url === "/api/today" ? { needs_you: [] } : { route: null };
      res.writeHead(200, { "content-type": "application/json" }); res.end(JSON.stringify(value)); return;
    }
    fallback(req, res);
  });
  await new Promise(done => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise(done => server.close(done)));
  const page = await browser.newPage({ serviceWorkers: "block" });
  const errors = [];
  page.on("pageerror", error => errors.push(error.message));
  page.setDefaultTimeout(5000);
  for (const width of [1440, 375]) {
    denied = false;
    await page.setViewportSize({ width, height: 1000 });
    await page.goto(`http://127.0.0.1:${server.address().port}/#routines`);
    await page.reload();
    try { await page.getByText("35 Orbit developer routines hidden", { exact: true }).waitFor(); }
    catch (error) { console.error({ body: await page.locator("body").innerText(), errors }); throw error; }
    assert.equal(await page.locator(".routine-row").count(), 4);
    assert.equal(await page.getByText("Developer maintenance", { exact: true }).count(), 0);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    if (process.env.ROUTINES_SCREENSHOTS_DIR) await page.screenshot({ path: resolve(process.env.ROUTINES_SCREENSHOTS_DIR, `routines-dark-${width}.png`), fullPage: true });
    const toggle = page.getByRole("switch", { name: "Office daily digest on", exact: true });
    await toggle.uncheck();
    await page.locator('.routine-group[aria-label="Paused"]').getByRole("heading", { name: "Office daily digest", exact: true }).waitFor();
    await page.getByText("35 Orbit developer routines hidden", { exact: true }).waitFor();
    await toggle.check();
    await page.locator('.routine-group[aria-label="On"]').getByRole("heading", { name: "Office daily digest", exact: true }).waitFor();
    denied = true;
    await page.reload();
    await page.getByText("22 Orbit developer routines hidden", { exact: true }).waitFor();
    await page.getByText("Some routines are unavailable with this connection. The hidden count covers only the routines this connection can read.", { exact: true }).waitFor();
    assert.equal(await page.locator(".routine-row").count(), 2);
    if (process.env.ROUTINES_SCREENSHOTS_DIR) await page.screenshot({ path: resolve(process.env.ROUTINES_SCREENSHOTS_DIR, `routines-unavailable-dark-${width}.png`), fullPage: true });
  }
  assert.deepEqual(errors, []);
});
