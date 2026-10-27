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

/** The Settings feature over a fake Orbit, a fixture server and a headless page at `#settings`. */
async function openSettings(t, fixtures) {
  assert.ok(process.env.PLAYWRIGHT_MODULE, "source ORBIT_BROWSER_KIT_ENV first");
  const fake = await installFakeOrbit();
  t.after(() => rm(fake.root, { recursive: true, force: true }));
  const fixturePath = resolve(fake.state, "gateway.json");
  await writeFile(fixturePath, JSON.stringify(fixtures));
  const env = fake.env({ HOME: fake.root, OFFICE_DATA_ROOT: fake.root, OFFICE_CONFIG_ROOT: fake.root });
  const settings = createSettingsFeature(createOrbitClient({ env }), { env });
  const fallback = createHandler();
  const server = createServer((request, response) => {
    if (!checkLoopbackRequest(request).ok) { response.writeHead(403); response.end(); return; }
    if (request.url === "/api/settings") {
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({ agentCli: { cli: "Muse Code", connected: true } })); return;
    }
    // Screens other than Today render only once the Library is ready.
    if (request.url === "/api/setup") {
      response.setHeader("content-type", "application/json");
      response.end(JSON.stringify({ state: "ready" })); return;
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
  const shots = process.env.SETTINGS_SCREENSHOTS_DIR;
  if (shots) await mkdir(shots, { recursive: true });
  async function capture(name) {
    for (const width of [1440, 375]) {
      await page.setViewportSize({ width, height: 1100 });
      assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `no overflow at ${width}px (${name})`);
      if (shots) await page.screenshot({ path: resolve(shots, `${name}-light-${width}.png`), fullPage: true });
    }
  }
  const region = page.getByRole("region", { name: "Connections", exact: true });
  return { fake, fixturePath, page, errors, capture, region };
}

test("Settings browser: add, edit, disable, write-only secret, newest-first audit and responsive views", { timeout: 45000 }, async (t) => {
  const { fake, fixturePath, page, errors, capture, region } = await openSettings(t, gatewayFixtures);
  const settingsLink = page.getByRole("navigation", { name: "Main navigation" }).getByRole("link", { name: "Settings", exact: true });
  await settingsLink.waitFor();
  assert.equal(await settingsLink.getAttribute("href"), "#settings");
  assert.equal(await settingsLink.getAttribute("aria-current"), "page");
  await region.getByRole("heading", { name: "acme-crm" }).waitFor();
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

test("Settings browser: secret address and device-code connections, the sign-in view and Sign in again", { timeout: 45000 }, async (t) => {
  const outlook = {
    name: "outlook", base_url: "https://graph.microsoft.com/v1.0/",
    auth: { scheme: "oauth2_device", token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
      device_url: "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode", client_id: "client-fixture",
      scopes: ["Calendars.Read", "offline_access"] },
    methods: ["GET"], drafts: [], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false,
    slot: "conn_04", slot_state: "set", auth_state: "expired",
  };
  const feed = {
    name: "team-calendar", base_url: "https://outlook.office365.com/", auth: { scheme: "secret_path" },
    methods: ["GET"], drafts: [], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false, slot: "conn_05", slot_state: "set",
  };
  const started = (connection) => ({ connection, state: "pending", user_code: "WDJB-MJHT",
    verification_uri: "https://microsoft.example/devicelogin", expires_in: 900, interval: 1 });
  const fixtures = {
    ...gatewayFixtures,
    "gateway.connection-list": { connections: [gatewayFixtures["gateway.connection-list"].connections[0], outlook, feed], free_slots: 13 },
    "gateway.oauth-device-start": started("outlook"),
    "gateway.oauth-device-poll": { connection: "outlook", state: "pending", interval: 1 },
  };
  const { fake, fixturePath, page, errors, capture, region } = await openSettings(t, fixtures);
  const card = (name) => region.locator("article").filter({ has: page.getByRole("heading", { name, exact: true }) });
  await card("outlook").getByText("Sign-in expired", { exact: true }).waitFor();
  await card("team-calendar").getByText("Secret address set", { exact: true }).waitFor();
  assert.equal(await card("acme-crm").getByRole("button", { name: /Sign in/ }).count(), 0);
  await capture("settings-connections-sign-in-expired");

  // Sign in again: the code and where to enter it, then the connected state.
  await card("outlook").getByRole("button", { name: "Sign in again", exact: true }).click();
  const panel = page.getByRole("region", { name: "Sign in to outlook", exact: true });
  assert.equal((await panel.getByLabel("Sign-in code", { exact: true }).innerText()).trim(), "WDJB-MJHT");
  const address = panel.getByRole("link", { name: "https://microsoft.example/devicelogin", exact: true });
  assert.equal(await address.getAttribute("href"), "https://microsoft.example/devicelogin");
  assert.equal(await address.getAttribute("rel"), "noopener noreferrer");
  await panel.getByRole("status").filter({ hasText: "Waiting for you to approve the sign-in…" }).waitFor();
  await capture("settings-device-code");
  const invoked = async () => (await readFile(resolve(fake.state, "invocations.log"), "utf8")).trim().split("\n").map(JSON.parse);
  for (let tries = 0; !(await invoked()).some((args) => args[2] === "gateway.oauth-device-poll"); tries += 1) {
    assert.ok(tries < 50, "the panel polls at the interval the service gave");
    await page.waitForTimeout(100);
  }
  await writeFile(fixturePath, JSON.stringify({ ...fixtures,
    "gateway.connection-list": { ...fixtures["gateway.connection-list"],
      connections: fixtures["gateway.connection-list"].connections.map((item) => item.name === "outlook" ? { ...item, auth_state: "signed_in" } : item) },
    "gateway.oauth-device-poll": { connection: "outlook", state: "connected" } }));
  await page.getByRole("status").filter({ hasText: "Signed in to outlook." }).waitFor();
  await card("outlook").getByText("Signed in", { exact: true }).waitFor();
  assert.equal(await card("outlook").getByRole("button", { name: /Sign in/ }).count(), 0);
  const starts = (await invoked()).filter((args) => args[2] === "gateway.oauth-device-start").map((args) => JSON.parse(args.at(-1)));
  assert.deepEqual(starts, [{ connection: "outlook" }]);
  // Disable saves the listed fields back: every OAuth field, never the listing's sign-in state.
  const postedToggle = page.waitForRequest((request) => request.url().endsWith("/api/settings/connections/put"));
  await card("outlook").getByRole("button", { name: "Disable", exact: true }).click();
  const toggle = JSON.parse((await postedToggle).postData());
  assert.equal(toggle.disabled, true);
  assert.equal("auth_state" in toggle, false);
  assert.deepEqual(toggle.auth, outlook.auth);
  await card("outlook").getByText("Disabled", { exact: true }).waitFor();

  // A pasted feed address: the origin becomes the base URL, the path and query the secret.
  await page.getByRole("button", { name: "Add connection", exact: true }).click();
  await page.getByLabel("Connection name", { exact: true }).fill("ics-feed");
  await page.getByLabel("Authentication", { exact: true }).selectOption("secret_path");
  assert.equal(await page.getByRole("checkbox", { name: "POST", exact: true }).isVisible(), false);
  const feedPath = "/owa/calendar/abc123/feedtoken987/calendar.ics?key=xyz";
  await page.getByLabel("Secret address", { exact: true }).fill(`https://calendar.example${feedPath}`);
  const postedFeed = page.waitForRequest((request) => request.url().endsWith("/api/settings/connections/put"));
  await page.getByRole("button", { name: "Save connection", exact: true }).click();
  const feedBody = JSON.parse((await postedFeed).postData());
  assert.deepEqual({ ...feedBody, secret: undefined }, { name: "ics-feed", base_url: "https://calendar.example/", auth: { scheme: "secret_path" },
    methods: ["GET"], drafts: [], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false, secret: undefined });
  assert.equal(feedBody.secret, feedPath);
  await page.getByRole("status").filter({ hasText: "Connection saved." }).waitFor();
  const stdin = (await readFile(resolve(fake.state, "secret-stdin.log"), "utf8")).trim().split("\n").map(JSON.parse);
  assert.equal(stdin.at(-1).value, feedPath);
  assert.ok(!(await page.content()).includes("feedtoken987"));
  assert.ok(!(await readFile(resolve(fake.state, "invocations.log"), "utf8")).includes("feedtoken987"));

  // A device-code connection: no secret field, Microsoft's endpoints offered, then Sign in.
  await page.getByRole("button", { name: "Add connection", exact: true }).click();
  await page.getByLabel("Connection name", { exact: true }).fill("graph");
  await page.getByLabel("Base URL", { exact: true }).fill("https://graph.microsoft.com/v1.0/");
  await page.getByLabel("Authentication", { exact: true }).selectOption("oauth2_device");
  assert.equal(await page.getByLabel("Secret", { exact: true }).isVisible(), false);
  assert.equal(await page.getByLabel("Token URL", { exact: true }).inputValue(), "https://login.microsoftonline.com/common/oauth2/v2.0/token");
  await page.getByLabel("Client ID", { exact: true }).fill("client-abc");
  await capture("settings-device-code-editor");
  const postedGraph = page.waitForRequest((request) => request.url().endsWith("/api/settings/connections/put"));
  await page.getByRole("button", { name: "Save connection", exact: true }).click();
  assert.deepEqual(JSON.parse((await postedGraph).postData()), {
    name: "graph", base_url: "https://graph.microsoft.com/v1.0/",
    auth: { scheme: "oauth2_device", token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
      device_url: "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode", client_id: "client-abc", scopes: ["Calendars.Read", "offline_access"] },
    methods: ["GET"], drafts: [], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false,
  });
  await page.getByRole("status").filter({ hasText: "Connection saved." }).waitFor();
  await card("graph").getByText("Not signed in", { exact: true }).waitFor();

  // A declined sign-in says so and offers to try again.
  const current = JSON.parse(await readFile(fixturePath, "utf8"));
  await writeFile(fixturePath, JSON.stringify({ ...current, "gateway.oauth-device-start": started("graph"),
    "gateway.oauth-device-poll": { connection: "graph", state: "denied" } }));
  await card("graph").getByRole("button", { name: "Sign in", exact: true }).click();
  const graphPanel = page.getByRole("region", { name: "Sign in to graph", exact: true });
  await graphPanel.getByRole("alert").filter({ hasText: "The sign-in was declined." }).waitFor();
  await graphPanel.getByRole("button", { name: "Try again", exact: true }).waitFor();
  await capture("settings-device-code-declined");
  await graphPanel.getByRole("button", { name: "Cancel", exact: true }).click();
  assert.equal(await graphPanel.count(), 0);
  assert.deepEqual(errors, []);
});
