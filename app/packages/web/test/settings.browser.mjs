/** Run after the app build with the verified host browser kit. No real Orbit state. */
import assert from "node:assert/strict";
import { mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";

import { createSettingsFeature } from "../../server/dist/features/settings/index.js";
import { checkLoopbackRequest } from "../../server/dist/http/guard.js";
import { createHandler } from "../../server/dist/http/router.js";
import { createOrbitClient } from "../../server/dist/orbit/client.js";
import { installFakeOrbit } from "../../server/test/fake-orbit.mjs";
import { gatewayFixtures } from "../../server/test/settings-fixture.mjs";

test("Settings browser: add, edit, disable, write-only secret, newest-first audit and responsive views", { timeout: 45000 }, async (t) => {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source ORBIT_BROWSER_KIT_ENV first");
  const fake = await installFakeOrbit();
  t.after(() => rm(fake.root, { recursive: true, force: true }));
  const fixturePath = resolve(fake.state, "gateway.json");
  await writeFile(fixturePath, JSON.stringify(gatewayFixtures));
  const env = fake.env({ HOME: fake.root, OFFICE_DATA_ROOT: fake.root, OFFICE_CONFIG_ROOT: fake.root });
  const settings = createSettingsFeature(createOrbitClient({ env }), { env });
  const fallback = createHandler();
  const server = createServer((request, response) => {
    if (!checkLoopbackRequest(request).ok) { response.writeHead(403); response.end(); return; }
    if (request.url === "/api/settings") {
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({ agentCli: { cli: "Muse Code", connected: true } })); return;
    }
    const route = settings.routes.find((route) => route.path === request.url && route.method === request.method);
    if (route) void route.handle(request, response);
    else fallback(request, response);
  });
  t.after(() => new Promise((done) => { server.closeAllConnections(); server.close(done); }));
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { chromium } = await import(process.env.PLAYWRIGHT_MODULE);
  const browser = await chromium.launch({ headless: true });
  t.after(() => browser.close());
  const page = await browser.newPage({ colorScheme: "light" });
  page.setDefaultTimeout(5000);
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(`http://127.0.0.1:${server.address().port}/#settings`);
  const settingsLink = page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: "Settings", exact: true });
  await settingsLink.waitFor();
  assert.equal(await settingsLink.getAttribute("href"), "#settings");
  assert.equal(await settingsLink.getAttribute("aria-current"), "page");
  const region = page.getByRole("region", { name: "Connections", exact: true });
  await region.getByRole("heading", { name: "acme-crm" }).waitFor();
  const shots = process.env.SETTINGS_SCREENSHOTS_DIR;
  if (shots) await mkdir(shots, { recursive: true });
  async function capture(name) {
    for (const width of [1440, 375]) {
      await page.setViewportSize({ width, height: 1100 });
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `no overflow at ${width}px`);
      if (shots) await page.screenshot({ path: resolve(shots, `${name}-light-${width}.png`), fullPage: true });
    }
  }
  await capture("settings-connections");
  await region.getByRole("button", { name: "Edit", exact: true }).click();
  assert.equal(await page.getByLabel("Secret", { exact: true }).inputValue(), "");
  await page.getByLabel("Timeout (seconds)").fill("35");
  await page.getByRole("button", { name: "Save connection", exact: true }).click();
  await page.getByRole("status").filter({ hasText: "Connection saved." }).waitFor();

  await region.getByRole("button", { name: "Disable", exact: true }).click();
  await region.getByText("Disabled", { exact: true }).waitFor();
  await region.getByRole("button", { name: "Enable", exact: true }).click();
  await region.getByText("Enabled", { exact: true }).waitFor();

  await page.getByRole("button", { name: "Add connection", exact: true }).click();
  await page.getByLabel("Connection name", { exact: true }).fill("billing-api");
  await page.getByLabel("Base URL", { exact: true }).fill("https://billing.example/v1/");
  await page.getByLabel("Authentication", { exact: true }).selectOption("header");
  await page.getByLabel("Header name", { exact: true }).fill("X-Api-Key");
  await page.getByRole("checkbox", { name: "HEAD", exact: true }).check();
  await page.getByRole("checkbox", { name: "POST", exact: true }).check();
  await capture("settings-connection-editor");
  const secret = "browser-fixture-secret";
  await page.getByLabel("Secret", { exact: true }).fill(secret);
  assert.deepEqual(await page.locator("form input:invalid").evaluateAll((nodes) => nodes.map((node) => ({ name: node.name, message: node.validationMessage }))), []);
  const posted = page.waitForRequest((request) => request.url().endsWith("/api/settings/connections/put"));
  // Read the input in the submit event's turn, before any HTTP reply can remove it.
  assert.equal(await page.locator("form").evaluate((form) => {
    form.requestSubmit(); return form.querySelector('[name="secret"]').value;
  }), "");
  assert.equal(JSON.parse((await posted).postData()).secret, secret);
  await page.getByRole("status").filter({ hasText: "Connection saved." }).waitFor();
  const billing = region.locator("article").filter({ has: page.getByRole("heading", { name: "billing-api" }) });
  await billing.getByText("Secret set", { exact: true }).waitFor();
  assert.ok(!(await page.content()).includes(secret));
  await billing.getByRole("button", { name: "Disable", exact: true }).click();
  await billing.getByText("Disabled", { exact: true }).waitFor();
  const argv = (await readFile(resolve(fake.state, "invocations.log"), "utf8")).trim().split("\n").map(JSON.parse);
  const puts = argv.filter((args) => args[2] === "gateway.connection-put").map((args) => JSON.parse(args.at(-1)));
  assert.equal(puts[0].timeout_seconds, 35);
  assert.equal(puts[0].idempotency_header, "Idempotency-Key");
  assert.equal(puts[1].idempotency_header, "Idempotency-Key");
  assert.equal(puts[1].disabled, true);
  assert.equal(puts[2].idempotency_header, "Idempotency-Key");
  assert.equal(puts[2].disabled, false);
  assert.deepEqual(puts[3], { name: "billing-api", base_url: "https://billing.example/v1/", auth: { scheme: "header", header: "X-Api-Key" }, methods: ["GET", "HEAD"], drafts: ["POST"], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false });
  assert.deepEqual(puts[4], { ...puts[3], disabled: true });
  await page.getByRole("button", { name: "API calls", exact: true }).click();
  await page.locator(".api-call").first().waitFor();
  assert.deepEqual(await page.locator(".api-call h3").allTextContents(), ["GET deals", "GET accounts", "GET contacts"]);
  assert.ok(!(await page.locator("main").innerText()).includes("private-task-id"));
  await capture("settings-api-calls");

  // Each section distinguishes a failed read from a successful empty response.
  await writeFile(fixturePath, JSON.stringify({ ...gatewayFixtures, "gateway.audit-list": { records: [], skipped: 0 } }));
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  await page.getByRole("heading", { name: "No API calls yet", exact: true }).waitFor();
  await writeFile(fixturePath, JSON.stringify({ ...gatewayFixtures, "gateway.audit-list": {} }));
  await page.getByRole("button", { name: "Refresh", exact: true }).click();
  await page.getByRole("heading", { name: "API calls are unavailable", exact: true }).waitFor();
  await writeFile(fixturePath, JSON.stringify({ ...gatewayFixtures, "gateway.connection-list": { connections: [], free_slots: 16 } }));
  await page.getByRole("button", { name: "Connections", exact: true }).click();
  await page.getByRole("heading", { name: "No connections yet", exact: true }).waitFor();
  await page.getByRole("button", { name: "Add connection", exact: true }).click();
  await page.getByLabel("Authentication", { exact: true }).selectOption("basic");
  assert.equal(await page.getByLabel("Username", { exact: true }).isVisible(), true);
  await page.getByLabel("Authentication", { exact: true }).selectOption("none");
  assert.equal(await page.getByLabel("Secret", { exact: true }).isVisible(), false);
  await page.getByRole("button", { name: "Cancel", exact: true }).click();

  assert.equal((await page.locator("#site-footer").innerText()).trim(), "");
  await page.getByRole("button", { name: "Private route", exact: true }).click();
  const privateRegion = page.getByRole("region", { name: "Private route", exact: true });
  await privateRegion.getByText("Private means not sent to Meta's hosted API, sent only to the endpoint you chose", { exact: true }).waitFor();
  assert.equal(await privateRegion.locator(".settings-attribution").isHidden(), true);
  await privateRegion.getByLabel("Endpoint kind", { exact: true }).selectOption("company");
  await privateRegion.getByText("Your employer's infrastructure. The strongest option for work material.", { exact: true }).waitFor();
  await capture("settings-private-route");
  await privateRegion.getByLabel("Endpoint URL", { exact: true }).fill("https://muse.internal.example/v1");
  await privateRegion.getByLabel("Model", { exact: true }).fill("muse-spark-open");
  await privateRegion.getByLabel("Label", { exact: true }).fill("Acme's server");
  await privateRegion.getByText("Private: Acme's server", { exact: true }).waitFor();
  const routeToken = "browser-private-token";
  await privateRegion.getByLabel("Endpoint token", { exact: true }).fill(routeToken);
  const postedRoute = page.waitForRequest((request) => request.url().endsWith("/api/settings/private-route") && request.method() === "POST");
  assert.equal(await privateRegion.locator("form").evaluate((form) => {
    form.requestSubmit();
    return form.querySelector('[name="token"]').value;
  }), "");
  assert.equal(JSON.parse((await postedRoute).postData()).token, routeToken);
  await privateRegion.getByText("this server doesn't take audio, so private videos can't be transcribed", { exact: true }).waitFor();
  await privateRegion.getByRole("status").filter({ hasText: "Private route saved." }).waitFor();
  await privateRegion.getByText("A token is saved. Leave this empty to keep it. Stored with Orbit and never shown again.", { exact: true }).waitFor();
  await privateRegion.getByText("Built with Muse Spark Open", { exact: true }).waitFor();
  await page.locator("#site-footer").getByText("Built with Muse Spark Open", { exact: true }).waitFor();
  assert.equal(await page.getByText("Built with Muse Spark Open", { exact: true }).count(), 2);
  assert.ok(!(await page.content()).includes(routeToken));
  await capture("settings-private-route-configured");
  await page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: "Today", exact: true }).click();
  await page.locator("#site-footer").getByText("Built with Muse Spark Open", { exact: true }).waitFor();
  const routeArgv = (await readFile(resolve(fake.state, "invocations.log"), "utf8")).trim().split("\n").map(JSON.parse);
  const routeNames = routeArgv.map((args) => args[0] === "plugin" ? `secret:${args[3]}:${args[4]}` : args[2]);
  const routeStart = routeNames.lastIndexOf("office.config-set");
  assert.deepEqual(routeNames.slice(routeStart, routeStart + 3), ["office.config-set", "secret:office:open_weights_token", "office.private-route-check"]);
  const routeStdin = JSON.parse((await readFile(resolve(fake.state, "secret-stdin.log"), "utf8")).trim().split("\n").at(-1));
  assert.equal(routeStdin.value, routeToken);
  assert.ok(!(await readFile(resolve(fake.state, "invocations.log"), "utf8")).includes(routeToken));
  assert.deepEqual(errors, []);
});
