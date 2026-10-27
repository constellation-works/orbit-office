import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { resolve } from "node:path";
import { createTodayFeature } from "../../server/dist/features/today/index.js";
import { OrbitExitError } from "../../server/dist/orbit/client.js";
import { createHandler } from "../../server/dist/http/router.js";
import { fakeTodayClient, now } from "../../server/test/today-fixture.mjs";

test("Library setup works at desktop and phone widths", { timeout: 60000 }, async (t) => {
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const fake = fakeTodayClient();
  const state = { initialized: false, calls: 0, failure: null, already: false };
  fake.client.callTool = async (name) => {
    if (name === "office.status") {
      if (state.failure === "disabled") throw new OrbitExitError(name, 1, null, "tool not found: office.status");
      if (state.failure === "unregistered") throw new OrbitExitError(name, 1, null, "workspace is not registered");
      return { ok: true, output: { library: { initialized: state.initialized } } };
    }
    assert.equal(name, "office.init");
    state.calls++;
    if (state.failure) return { ok: false, error: { code: state.failure } };
    state.initialized = true;
    return { ok: true, output: { created: state.already ? [] : [".office/config.yaml"], already_initialized: state.already } };
  };
  const today = createTodayFeature({ client: fake.client, now: () => now });
  const fallback = createHandler();
  const server = createServer((req, res) => {
    const route = today.routes.find((r) => r.path === req.url && r.method === req.method);
    if (route) void route.handle(req, res); else fallback(req, res);
  });
  t.after(() => new Promise((done) => server.close(done)));
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const page = await browser.newPage();
  for (const width of [1440, 375]) {
    await page.setViewportSize({ width, height: 900 });
    state.initialized = false; state.failure = null; state.already = false;
    await page.goto(`http://127.0.0.1:${server.address().port}/#today`);
    await page.reload();
    await page.getByRole("button", { name: "Set up" }).waitFor();
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    if (process.env.SETUP_SCREENSHOTS_DIR) await page.screenshot({ path: resolve(process.env.SETUP_SCREENSHOTS_DIR, `setup-${width}.png`) });
    await page.goto(`http://127.0.0.1:${server.address().port}/#library`);
    await page.getByText("Your Library isn’t set up yet").waitFor();
    if (process.env.SETUP_SCREENSHOTS_DIR) await page.screenshot({ path: resolve(process.env.SETUP_SCREENSHOTS_DIR, `setup-library-${width}.png`) });
    await page.getByRole("link", { name: "Go to Today" }).click();
    const before = state.calls;
    await page.getByRole("button", { name: "Set up" }).click();
    await page.getByRole("heading", { name: "Needs you", exact: true }).waitFor();
    assert.equal(state.calls, before + 1);
    state.initialized = false; state.already = true;
    await page.reload();
    await page.getByRole("button", { name: "Set up" }).click();
    await page.getByRole("heading", { name: "Needs you", exact: true }).waitFor();
    state.initialized = false; state.failure = "permission_denied";
    await page.reload();
    await page.getByRole("button", { name: "Set up" }).click();
    await page.getByText(/could not write the Library/).waitFor();
    if (process.env.SETUP_SCREENSHOTS_DIR) await page.screenshot({ path: resolve(process.env.SETUP_SCREENSHOTS_DIR, `setup-error-${width}.png`) });
    const calls = state.calls;
    await page.waitForTimeout(300);
    assert.equal(state.calls, calls);
    assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
    for (const failure of ["disabled", "unregistered"]) {
      state.failure = failure;
      await page.reload();
      await page.getByText(/Run office init in your workspace/).waitFor();
      assert.equal(await page.getByRole("button", { name: "Set up" }).count(), 0);
    }
  }
});
