import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer, request as httpRequest } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";

import {
  agentCliStatus,
  mcpServersHasOrbit,
  tomlHasOrbitMcp,
} from "../dist/features/settings/agent-cli.js";
import { createHandler } from "../dist/http/router.js";
import { createSettingsFeature } from "../dist/features/settings/index.js";
import { checkLoopbackRequest } from "../dist/http/guard.js";
import { OrbitExitError, OrbitTimeoutError } from "../dist/orbit/client.js";
import { installFakeOrbit } from "./fake-orbit.mjs";
import { gatewayFixtures } from "./settings-fixture.mjs";

function request(port, { method = "GET", path = "/", headers = {}, body } = {}) {
  return new Promise((resolve, reject) => {
    const req = httpRequest(
      { host: "127.0.0.1", port, method, path, headers },
      (res) => {
        const chunks = [];
        res.on("data", (chunk) => chunks.push(chunk));
        res.on("end", () =>
          resolve({
            status: res.statusCode,
            headers: res.headers,
            body: Buffer.concat(chunks).toString("utf8"),
          }),
        );
      },
    );
    req.on("error", reject);
    req.end(body);
  });
}

/** A fake home directory holding `files` (relative path -> content). */
async function fixtureHome(t, files) {
  const home = await mkdtemp(join(tmpdir(), "office-settings-"));
  t.after(async () => {
    await rm(home, { recursive: true, force: true });
  });
  for (const [relative, content] of Object.entries(files)) {
    const path = join(home, relative);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }
  return home;
}

const CONFIG_OVERRIDES = ["XDG_CONFIG_HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/**
 * Run `run` with HOME pointed at a fixture (or removed) and the config-dir
 * overrides cleared, so detection never reads the real home directory.
 */
async function withIsolatedHome(home, run) {
  const saved = { HOME: process.env.HOME };
  for (const key of CONFIG_OVERRIDES) saved[key] = process.env[key];
  if (home === undefined) delete process.env.HOME;
  else process.env.HOME = home;
  for (const key of CONFIG_OVERRIDES) delete process.env[key];
  try {
    await run();
  } finally {
    for (const [key, value] of Object.entries(saved)) {
      if (value === undefined) delete process.env[key];
      else process.env[key] = value;
    }
  }
}

test("mcpServers JSON counts an Orbit entry, nothing else", () => {
  assert.equal(mcpServersHasOrbit(JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } })), true);
  assert.equal(mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { command: "orbit" } } })), true);
  assert.equal(
    mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { url: "https://orbit.example.com/mcp" } } })),
    true,
  );
  assert.equal(
    mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { command: "helper", args: ["--config", "orbit"] } } })),
    true,
  );
  assert.equal(
    mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { command: "helper", arguments: ["orbit"] } } })),
    true,
  );
  assert.equal(
    mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { command: "helper", cwd: "/work/orbit" } } })),
    false,
  );
  assert.equal(
    mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { command: "helper", env: { TOKEN: "orbit" } } } })),
    false,
  );
  assert.equal(mcpServersHasOrbit(JSON.stringify({ mcpServers: {} })), false);
  assert.equal(mcpServersHasOrbit(JSON.stringify({})), false);
  assert.equal(mcpServersHasOrbit(JSON.stringify({ mcpServers: "orbit" })), false);
  assert.equal(mcpServersHasOrbit("{not json"), false);
  assert.equal(mcpServersHasOrbit('"just a string"'), false);
});

const connectionInput = {
  name: "acme-crm", base_url: "https://api.acme.example/v2/",
  auth: { scheme: "bearer" }, methods: ["GET", "HEAD"], drafts: ["PATCH"],
  timeout_seconds: 20, max_response_bytes: 1048576, disabled: false,
};

/** Run the real HTTP router in a child with fixture-only home and Orbit. */
async function gatewayServer(t, extra = {}) {
  const fake = await installFakeOrbit();
  t.after(() => rm(fake.root, { recursive: true, force: true }));
  await writeFile(join(fake.state, "gateway.json"), JSON.stringify(gatewayFixtures));
  const router = new URL("../dist/http/router.js", import.meta.url).href;
  const child = spawn(process.execPath, ["--input-type=module", "-e", `
    import { createServer } from "node:http";
    import { handle } from ${JSON.stringify(router)};
    const server = createServer(handle);
    server.listen(0, "127.0.0.1", () => console.log(server.address().port));
  `], {
    env: fake.env({ HOME: fake.root, OFFICE_DATA_ROOT: fake.root, OFFICE_CONFIG_ROOT: fake.root, ...extra }),
    stdio: ["ignore", "pipe", "pipe"],
  });
  const closed = once(child, "close");
  t.after(async () => { child.kill("SIGTERM"); await closed; });
  const logs = [];
  child.stdout.on("data", (chunk) => logs.push(chunk.toString()));
  child.stderr.on("data", (chunk) => logs.push(chunk.toString()));
  const port = await new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error("fixture server did not start")), 5000);
    child.once("error", (error) => { clearTimeout(timer); reject(error); });
    child.stdout.once("data", (chunk) => { clearTimeout(timer); resolve(Number(chunk.toString().trim())); });
  });
  const responses = [];
  return { fake, logs, responses, async call(path, body, headers = {}) {
    const res = await request(port, { path, method: body === undefined ? "GET" : "POST",
      headers: { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}`, "content-type": "application/json", ...headers },
      body: body === undefined ? undefined : JSON.stringify(body) });
    responses.push(res);
    return res;
  } };
}

test("Settings preserves a persisted gateway idempotency header across edits and toggles", async (t) => {
  const root = await fixtureHome(t, {});
  const source = resolve(import.meta.dirname, "../../../../gateway/src");
  const python = `import json, sys
from orbit_gateway.connections import tool_list, tool_put
from orbit_gateway.envelope import Call
request = json.load(sys.stdin)
call = Call(request["tool"], request["input"], {"plugin_state": sys.argv[1]})
print(json.dumps(tool_put(call) if request["tool"] == "gateway.connection-put" else tool_list(call)))`;
  const client = { async callTool(tool, input) {
    const result = spawnSync("python3", ["-c", python, root], {
      input: JSON.stringify({ tool, input }), encoding: "utf8",
      env: { ...process.env, HOME: root, OFFICE_DATA_ROOT: root, OFFICE_CONFIG_ROOT: root, PYTHONPATH: source },
    });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  } };
  await client.callTool("gateway.connection-put", { ...connectionInput, idempotency_header: "Idempotency-Key" });
  const feature = createSettingsFeature(client);
  const server = createServer((req, res) => {
    if (!checkLoopbackRequest(req).ok) { res.writeHead(403); res.end(); return; }
    const route = feature.routes.find((item) => item.method === req.method && item.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise((done) => { server.closeAllConnections(); server.close(done); }));
  const { port } = server.address();
  const headers = { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}` };
  for (const change of [{ timeout_seconds: 35 }, { disabled: true }, { disabled: false }]) {
    const listed = await request(port, { path: "/api/settings/connections", headers });
    assert.equal(listed.status, 200);
    const { slot_state, ...config } = JSON.parse(listed.body).connections[0];
    assert.equal(config.idempotency_header, "Idempotency-Key");
    const saved = await request(port, { method: "POST", path: "/api/settings/connections/put", headers, body: JSON.stringify({ ...config, ...change }) });
    assert.equal(saved.status, 200);
    assert.equal(JSON.parse(saved.body).connection.idempotency_header, "Idempotency-Key");
    assert.equal((await client.callTool("gateway.connection-list", {})).connections[0].idempotency_header, "Idempotency-Key");
    assert.equal(JSON.parse(await readFile(join(root, "connections.json"), "utf8")).connections[0].idempotency_header, "Idempotency-Key");
  }
  for (const header of ["X-Request-Id", undefined]) {
    const input = { ...connectionInput, ...(header === undefined ? {} : { idempotency_header: header }) };
    const saved = await request(port, { method: "POST", path: "/api/settings/connections/put", headers, body: JSON.stringify(input) });
    assert.equal(saved.status, 200);
    assert.equal((await client.callTool("gateway.connection-list", {})).connections[0].idempotency_header, header);
  }
  const invalid = await request(port, { method: "POST", path: "/api/settings/connections/put", headers,
    body: JSON.stringify({ ...connectionInput, idempotency_header: 42 }) });
  assert.equal(invalid.status, 400);
});

test("Settings adds, edits and disables via gateway and sends secrets only on stdin", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  const secret = "fixture-secret-with-$()-and-newline\nsecond-line";
  const listed = await server.call("/api/settings/connections");
  assert.equal(listed.status, 200);
  assert.equal(JSON.parse(listed.body).connections[0].slot_state, "set");
  for (const input of [connectionInput, { ...connectionInput, drafts: ["POST", "PATCH"], timeout_seconds: 35 }, { ...connectionInput, disabled: true }]) {
    const res = await server.call("/api/settings/connections/put", { ...input, secret });
    assert.equal(res.status, 200);
    assert.equal(JSON.parse(res.body).connection.slot_state, "set");
  }
  const stdin = (await readFile(join(server.fake.state, "secret-stdin.log"), "utf8")).trim().split("\n").map(JSON.parse);
  assert.deepEqual(stdin, Array.from({ length: 3 }, () => ({ slot: "conn_03", value: secret })));
  const argvText = await readFile(join(server.fake.state, "invocations.log"), "utf8");
  const invocations = argvText.trim().split("\n").map(JSON.parse);
  assert.deepEqual(invocations.filter((args) => args[0] === "plugin"), Array.from({ length: 3 }, () => ["plugin", "secret", "set", "gateway", "conn_03"]));
  const puts = invocations.filter((args) => args[2] === "gateway.connection-put").map((args) => JSON.parse(args[args.indexOf("--input") + 1]));
  assert.deepEqual(puts, [connectionInput, { ...connectionInput, drafts: ["POST", "PATCH"], timeout_seconds: 35 }, { ...connectionInput, disabled: true }]);
  const audit = await server.call("/api/settings/api-calls");
  assert.equal(audit.status, 200);
  assert.deepEqual(JSON.parse(audit.body).records.map((row) => row.path), ["deals", "accounts", "contacts"]);
  assert.ok(!audit.body.includes("private-task-id"));
  assert.ok(!argvText.includes("fixture-secret"));
  for (const line of server.logs) assert.ok(!line.includes("fixture-secret"), "every server log excludes the secret");
  for (const response of server.responses) assert.ok(!JSON.stringify(response).includes("fixture-secret"), "every HTTP response excludes the secret");
});

test("Settings handles exec envelopes, errors and malformed output without forwarding diagnostics", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  for (const [payload, status, code] of [
    [{ ok: true, output: gatewayFixtures["gateway.connection-list"] }, 200, undefined],
    [{ ok: false, error: { code: "backend_error", message: "private diagnostic" } }, 502, "gateway_unavailable"],
    [{ connections: [{}], free_slots: 15 }, 502, "invalid_gateway_data"],
    [{}, 502, "invalid_gateway_data"],
  ]) {
    await writeFile(join(server.fake.state, "gateway.json"), JSON.stringify({ "gateway.connection-list": payload }));
    const response = await server.call("/api/settings/connections");
    assert.equal(response.status, status);
    if (code) assert.equal(JSON.parse(response.body).error, code);
    assert.ok(!response.body.includes("private diagnostic"));
  }
});

test("changing origin cleans up the released slot without forwarding a secret", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  await writeFile(join(server.fake.state, "gateway.json"), JSON.stringify({ ...gatewayFixtures, released_slot: "conn_02" }));
  const res = await server.call("/api/settings/connections/put", { ...connectionInput, base_url: "https://new.example/" });
  assert.equal(res.status, 200);
  const calls = (await readFile(join(server.fake.state, "invocations.log"), "utf8")).trim().split("\n").map(JSON.parse);
  assert.deepEqual(calls.at(-1), ["plugin", "secret", "rm", "gateway", "conn_02"]);
});

test("Settings refuses concurrent writes until the entire first save settles", { timeout: 5000 }, async (t) => {
  let enter;
  const entered = new Promise((resolve) => { enter = resolve; });
  let release;
  const waiting = new Promise((resolve) => { release = resolve; });
  let calls = 0;
  const feature = createSettingsFeature({ async callTool() {
    calls += 1;
    if (calls === 1) { enter(); await waiting; }
    return { connection: { ...connectionInput, slot: "conn_03", slot_state: "unset" }, created: true, released_slot: null };
  } });
  const route = feature.routes.find((route) => route.path.endsWith("/put"));
  const server = createServer((req, res) => {
    if (!checkLoopbackRequest(req).ok) { res.writeHead(403); res.end(); return; }
    void route.handle(req, res);
  });
  t.after(() => new Promise((resolve) => { release(); server.closeAllConnections(); server.close(resolve); }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const call = () => request(port, { method: "POST", path: "/api/settings/connections/put",
    headers: { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}` }, body: JSON.stringify(connectionInput) });
  const first = call();
  try {
    await entered;
    const second = await call();
    assert.equal(second.status, 503);
    assert.equal(JSON.parse(second.body).error, "settings_busy");
    assert.equal(calls, 1);
  } finally { release(); }
  assert.equal((await first).status, 200);
  assert.equal((await call()).status, 200);
  assert.equal(calls, 2);
});

test("Settings refuses unknown fields, invalid secrets and foreign origins before tool calls", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  for (const body of [
    { ...connectionInput, surprise: "private" }, { ...connectionInput, secret: 42 },
    { ...connectionInput, auth: { scheme: "none" }, secret: "private" },
    { ...connectionInput, auth: { scheme: "bearer", secret: "private" } },
  ]) assert.equal((await server.call("/api/settings/connections/put", body)).status, 400);
  assert.equal((await server.call("/api/settings/connections/put", connectionInput, { origin: "https://other.example" })).status, 403);
  await assert.rejects(readFile(join(server.fake.state, "invocations.log")), { code: "ENOENT" });
});

test("a secret command failure cannot leak its echoed stdout or stderr", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t, { FAKE_SECRET_FAIL: "1" });
  const res = await server.call("/api/settings/connections/put", { ...connectionInput, secret: "fixture-secret-on-failure" });
  assert.equal(res.status, 502);
  assert.equal(JSON.parse(res.body).error, "secret_not_saved");
  for (const line of server.logs) assert.ok(!line.includes("fixture-secret"));
  for (const response of server.responses) assert.ok(!JSON.stringify(response).includes("fixture-secret"));
});

const deviceConnection = {
  name: "outlook", base_url: "https://graph.microsoft.com/v1.0/",
  auth: { scheme: "oauth2_device", token_url: "https://login.example/token", device_url: "https://login.example/devicecode",
    client_id: "client-fixture", scopes: ["Calendars.Read", "offline_access"] },
  methods: ["GET"], drafts: [], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false,
};

test("the sign-in routes start and poll the device-code flow and pass on only what the user needs", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  const fixtures = (extra) => writeFile(join(server.fake.state, "gateway.json"), JSON.stringify({ ...gatewayFixtures, ...extra }));
  // A start answer carrying more than the projection allows (a device code) never reaches HTTP.
  await fixtures({ "gateway.oauth-device-start": { connection: "outlook", state: "pending", user_code: "WDJB-MJHT",
    verification_uri: "https://microsoft.example/devicelogin", expires_in: 900, interval: 5, device_code: "fixture-device-code" } });
  const started = await server.call("/api/settings/connections/sign-in", { name: "outlook" });
  assert.equal(started.status, 200);
  assert.deepEqual(JSON.parse(started.body), { connection: "outlook", user_code: "WDJB-MJHT",
    verification_uri: "https://microsoft.example/devicelogin", expires_in: 900, interval: 5 });
  for (const [answer, status, body] of [
    [{ connection: "outlook", state: "pending", interval: 10 }, 200, { connection: "outlook", state: "pending", interval: 10 }],
    [{ connection: "outlook", state: "connected" }, 200, { connection: "outlook", state: "connected" }],
    [{ connection: "outlook", state: "denied" }, 200, { connection: "outlook", state: "denied" }],
    [{ ok: false, error: { code: "device_flow_missing", message: "private diagnostic" } }, 409, { error: "sign_in_not_started" }],
    [{ ok: false, error: { code: "auth_failed", message: "private diagnostic" } }, 502, { error: "sign_in_failed" }],
    [{ connection: "another", state: "connected" }, 502, { error: "invalid_gateway_data" }],
    [{ connection: "outlook", state: "connected", refresh_token: "x" }, 200, { connection: "outlook", state: "connected" }],
  ]) {
    await fixtures({ "gateway.oauth-device-poll": answer });
    const res = await server.call("/api/settings/connections/sign-in/poll", { name: "outlook" });
    assert.equal(res.status, status, res.body);
    assert.deepEqual(JSON.parse(res.body), body);
  }
  const invocations = (await readFile(join(server.fake.state, "invocations.log"), "utf8")).trim().split("\n").map(JSON.parse);
  const calls = invocations.filter((args) => args[2]?.startsWith("gateway.oauth-device-")).map((args) => [args[2], JSON.parse(args.at(-1))]);
  assert.deepEqual(calls[0], ["gateway.oauth-device-start", { connection: "outlook" }]);
  assert.ok(calls.slice(1).every(([tool, input]) => tool === "gateway.oauth-device-poll" && input.connection === "outlook"));
  for (const response of server.responses) {
    assert.ok(!response.body.includes("fixture-device-code") && !response.body.includes("private diagnostic"));
  }
});

test("the sign-in routes refuse anything but a connection name before any tool call", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  for (const body of [{}, { name: "Outlook" }, { name: "outlook", device_code: "x" }, { name: 42 }, []]) {
    assert.equal((await server.call("/api/settings/connections/sign-in", body)).status, 400);
    assert.equal((await server.call("/api/settings/connections/sign-in/poll", body)).status, 400);
  }
  assert.equal((await server.call("/api/settings/connections/sign-in", { name: "outlook" }, { origin: "https://other.example" })).status, 403);
  await assert.rejects(readFile(join(server.fake.state, "invocations.log")), { code: "ENOENT" });
});

test("device-code and secret address connections round-trip through the real gateway, and a refresh token is never accepted", { timeout: 15000 }, async (t) => {
  const root = await fixtureHome(t, {});
  const source = resolve(import.meta.dirname, "../../../../gateway/src");
  const python = `import json, sys
from orbit_gateway.connections import tool_list, tool_put
from orbit_gateway.envelope import Call, GatewayError
request = json.load(sys.stdin)
call = Call(request["tool"], request["input"], {"plugin_state": sys.argv[1]})
try:
    print(json.dumps(tool_put(call) if request["tool"] == "gateway.connection-put" else tool_list(call)))
except GatewayError as error:
    print(json.dumps({"ok": False, "error": error.to_json()}))`;
  const client = { async callTool(tool, input) {
    const result = spawnSync("python3", ["-c", python, root], {
      input: JSON.stringify({ tool, input }), encoding: "utf8",
      env: { ...process.env, HOME: root, OFFICE_DATA_ROOT: root, OFFICE_CONFIG_ROOT: root, PYTHONPATH: source },
    });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  } };
  const feature = createSettingsFeature(client);
  const server = createServer((req, res) => {
    if (!checkLoopbackRequest(req).ok) { res.writeHead(403); res.end(); return; }
    const route = feature.routes.find((item) => item.method === req.method && item.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  t.after(() => new Promise((done) => { server.closeAllConnections(); server.close(done); }));
  const { port } = server.address();
  const headers = { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}` };
  const put = (body) => request(port, { method: "POST", path: "/api/settings/connections/put", headers, body: JSON.stringify(body) });
  const refused = await put({ ...deviceConnection, secret: "rt-fixture-refresh-token" });
  assert.equal(refused.status, 400);
  const saved = await put(deviceConnection);
  assert.equal(saved.status, 200, saved.body);
  assert.deepEqual(JSON.parse(saved.body).connection, { ...deviceConnection, slot_state: "unset", auth_state: "signed_out" });
  const feed = { name: "team-calendar", base_url: "https://outlook.office365.com/", auth: { scheme: "secret_path" },
    methods: ["GET"], drafts: [], timeout_seconds: 20, max_response_bytes: 1048576, disabled: false };
  // The gateway refuses a path in a secret address connection's base URL.
  assert.equal((await put({ ...feed, base_url: "https://outlook.office365.com/owa/" })).status, 400);
  assert.equal((await put(feed)).status, 200);
  const listed = JSON.parse((await request(port, { path: "/api/settings/connections", headers })).body).connections;
  assert.deepEqual(listed.map((item) => [item.name, item.auth.scheme, item.auth_state]), [["outlook", "oauth2_device", "signed_out"], ["team-calendar", "secret_path", undefined]]);
  // Saving what was listed back (as Edit and Disable do, without auth_state) keeps every OAuth field.
  const { slot_state: _slot, auth_state: _auth, ...config } = listed[0];
  const toggled = await put({ ...config, disabled: true });
  assert.equal(toggled.status, 200, toggled.body);
  assert.deepEqual(JSON.parse(toggled.body).connection.auth, deviceConnection.auth);
  assert.ok(!(await readFile(join(root, "connections.json"), "utf8")).includes("rt-fixture"));
});

test("Codex TOML counts an Orbit server section, nothing else", () => {
  assert.equal(tomlHasOrbitMcp('[mcp_servers.orbit]\ncommand = "orbit"\n'), true);
  assert.equal(tomlHasOrbitMcp('[mcp_servers.helper]\ncommand = "orbit"\n'), true);
  assert.equal(tomlHasOrbitMcp('[mcp_servers.helper]\nurl = "https://orbit.example.com/mcp"\n'), true);
  assert.equal(tomlHasOrbitMcp('[mcp_servers.helper]\ncommand = "helper"\nargs = ["--config", "orbit"]\n'), true);
  assert.equal(
    tomlHasOrbitMcp('[mcp_servers.helper]\ncwd = "/work/orbit"\nenv = { TOKEN = "orbit" }\n'),
    false,
  );
  assert.equal(
    tomlHasOrbitMcp('[mcp_servers.helper]\ncommand = "helper" # orbit is only a comment\n'),
    false,
  );
  assert.equal(
    tomlHasOrbitMcp('[mcp_servers.helper]\ncommand = "helper"\n[mcp_servers.helper.env]\nTOKEN = "x"\n'),
    false,
  );
  assert.equal(tomlHasOrbitMcp('[mcp_servers.other]\ncommand = "other"\n'), false);
  assert.equal(tomlHasOrbitMcp(''), false);
  // An "orbit" path outside any server section (a trusted project dir) is not a server.
  assert.equal(
    tomlHasOrbitMcp('[projects."/home/user/orbit"]\ntrust_level = "trusted"\n'),
    false,
  );
});

test("agentCliStatus reads the three CLI configs in order", async (t) => {
  await withIsolatedHome(undefined, () => {
    assert.deepEqual(agentCliStatus({}), { cli: "Muse Code", connected: false });
  });

  const empty = await fixtureHome(t, {});
  await withIsolatedHome(empty, () => {
    assert.deepEqual(agentCliStatus(), { cli: "Muse Code", connected: false });
  });

  const muse = await fixtureHome(t, {
    ".config/muse/settings.json": JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } }),
  });
  await withIsolatedHome(muse, () => {
    assert.deepEqual(agentCliStatus(), { cli: "Muse Code", connected: true });
  });

  const xdg = await fixtureHome(t, {
    "custom/muse/settings.json": JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } }),
  });
  await withIsolatedHome(undefined, () => {
    assert.deepEqual(agentCliStatus({ HOME: xdg, XDG_CONFIG_HOME: join(xdg, "custom") }), {
      cli: "Muse Code",
      connected: true,
    });
  });

  const broken = await fixtureHome(t, { ".config/muse/settings.json": "{not json" });
  await withIsolatedHome(broken, () => {
    assert.deepEqual(agentCliStatus(), { cli: "Muse Code", connected: false });
  });

  const claude = await fixtureHome(t, {
    ".claude.json": JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } }),
  });
  await withIsolatedHome(claude, () => {
    assert.deepEqual(agentCliStatus(), { cli: "Claude Code", connected: true });
  });

  const codex = await fixtureHome(t, {
    ".codex/config.toml": '[mcp_servers.orbit]\ncommand = "orbit"\n',
  });
  await withIsolatedHome(codex, () => {
    assert.deepEqual(agentCliStatus(), { cli: "Codex", connected: true });
  });

  const both = await fixtureHome(t, {
    ".config/muse/settings.json": JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } }),
    ".claude.json": JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } }),
  });
  await withIsolatedHome(both, () => {
    assert.deepEqual(agentCliStatus(), { cli: "Muse Code", connected: true });
  });
});

test("agentCliStatus ignores Orbit in unrelated Muse JSON fields", async (t) => {
  const muse = await fixtureHome(t, {
    ".config/muse/settings.json": JSON.stringify({
      mcpServers: { helper: { command: "helper", cwd: "/work/orbit", env: { TOKEN: "orbit" } } },
    }),
  });
  assert.deepEqual(agentCliStatus({ HOME: muse }), { cli: "Muse Code", connected: false });
});

test("agentCliStatus ignores Orbit in unrelated Claude JSON fields", async (t) => {
  const claude = await fixtureHome(t, {
    ".claude.json": JSON.stringify({
      mcpServers: { helper: { command: "helper", cwd: "/work/orbit", env: { TOKEN: "orbit" } } },
    }),
  });
  assert.deepEqual(agentCliStatus({ HOME: claude }), { cli: "Muse Code", connected: false });
});

test("agentCliStatus ignores Orbit in unrelated Codex fields and comments", async (t) => {
  const codex = await fixtureHome(t, {
    ".codex/config.toml": '[mcp_servers.helper]\ncwd = "/work/orbit"\n# orbit appears only in a comment\n',
  });
  assert.deepEqual(agentCliStatus({ HOME: codex }), { cli: "Muse Code", connected: false });
});

test("empty config-directory overrides use the documented home defaults", async (t) => {
  const museSettings = JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } });
  const codexConfig = '[mcp_servers.orbit]\ncommand = "orbit"\n';

  const muse = await fixtureHome(t, { ".config/muse/settings.json": museSettings });
  assert.deepEqual(agentCliStatus({ HOME: muse, XDG_CONFIG_HOME: "" }), {
    cli: "Muse Code",
    connected: true,
  });

  const claude = await fixtureHome(t, { ".claude.json": museSettings });
  assert.deepEqual(
    agentCliStatus({ HOME: claude, XDG_CONFIG_HOME: "", CLAUDE_CONFIG_DIR: "", CODEX_HOME: "" }),
    { cli: "Claude Code", connected: true },
  );

  const codex = await fixtureHome(t, { ".codex/config.toml": codexConfig });
  assert.deepEqual(
    agentCliStatus({ HOME: codex, XDG_CONFIG_HOME: "", CLAUDE_CONFIG_DIR: "", CODEX_HOME: "" }),
    { cli: "Codex", connected: true },
  );
});

test("non-empty config-directory overrides select those directories", async (t) => {
  const museSettings = JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } });
  const codexConfig = '[mcp_servers.orbit]\ncommand = "orbit"\n';

  const selected = await fixtureHome(t, {
    "xdg/muse/settings.json": museSettings,
    "claude/.claude.json": museSettings,
    "codex/config.toml": codexConfig,
  });
  assert.deepEqual(agentCliStatus({ HOME: selected, XDG_CONFIG_HOME: join(selected, "xdg") }), {
    cli: "Muse Code",
    connected: true,
  });
  assert.deepEqual(agentCliStatus({ HOME: selected, CLAUDE_CONFIG_DIR: join(selected, "claude") }), {
    cli: "Claude Code",
    connected: true,
  });
  assert.deepEqual(agentCliStatus({ HOME: selected, CODEX_HOME: join(selected, "codex") }), {
    cli: "Codex",
    connected: true,
  });

  const defaultsOnly = await fixtureHome(t, {
    ".config/muse/settings.json": museSettings,
    ".claude.json": museSettings,
    ".codex/config.toml": codexConfig,
    "xdg/muse/settings.json": "{}",
    "claude/.claude.json": "{}",
    "codex/config.toml": "",
  });
  assert.deepEqual(
    agentCliStatus({
      HOME: defaultsOnly,
      XDG_CONFIG_HOME: join(defaultsOnly, "xdg"),
      CLAUDE_CONFIG_DIR: join(defaultsOnly, "claude"),
      CODEX_HOME: join(defaultsOnly, "codex"),
    }),
    { cli: "Muse Code", connected: false },
  );
});

test("GET /api/settings reports the agent CLI connection", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "office-settings-http-"));
  t.after(async () => {
    await rm(root, { recursive: true, force: true });
  });
  const staticDir = join(root, "static");
  const distDir = join(root, "dist");
  await mkdir(staticDir, { recursive: true });
  await mkdir(distDir, { recursive: true });
  const server = createServer(createHandler({ staticDir, distDir }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    await new Promise((resolve) => server.close(resolve));
  });
  const { port } = server.address();
  const host = `127.0.0.1:${port}`;

  const connected = await fixtureHome(t, {
    ".config/muse/settings.json": JSON.stringify({ mcpServers: { orbit: { command: "orbit" } } }),
  });
  await withIsolatedHome(connected, async () => {
    const res = await request(port, { path: "/api/settings", headers: { host } });
    assert.equal(res.status, 200);
    assert.deepEqual(JSON.parse(res.body), { agentCli: { cli: "Muse Code", connected: true } });
  });

  const empty = await fixtureHome(t, {});
  await withIsolatedHome(empty, async () => {
    const res = await request(port, { path: "/api/settings", headers: { host } });
    assert.equal(res.status, 200);
    assert.deepEqual(JSON.parse(res.body), { agentCli: { cli: "Muse Code", connected: false } });
  });

  const unrelated = await fixtureHome(t, {
    ".config/muse/settings.json": JSON.stringify({
      mcpServers: { helper: { command: "helper", cwd: "/work/orbit", env: { TOKEN: "orbit" } } },
    }),
  });
  await withIsolatedHome(unrelated, async () => {
    const res = await request(port, { path: "/api/settings", headers: { host } });
    assert.equal(res.status, 200);
    assert.deepEqual(JSON.parse(res.body), { agentCli: { cli: "Muse Code", connected: false } });
  });
});

const privateRoute = {
  endpoint: "https://muse.internal.example/v1",
  model: "muse-spark-open",
  kind: "company",
  label: "Acme's server",
};
const audioNote = "this server doesn't take audio, so private videos can't be transcribed";

function invocationsOf(state) {
  return readFile(join(state, "invocations.log"), "utf8").then((text) => text.trim().split("\n").filter(Boolean).map(JSON.parse));
}

test("private route refuses invalid bodies and foreign origins before any Orbit call", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  for (const body of [
    { ...privateRoute, surprise: "no" },
    { ...privateRoute, token: "   " },
    { ...privateRoute, token: "" },
    { ...privateRoute, kind: "cloud" },
    { ...privateRoute, model: "has space" },
    { ...privateRoute, label: "   " },
    { ...privateRoute, endpoint: "https://muse.example/\n" },
  ]) assert.equal((await server.call("/api/settings/private-route", body)).status, 400);
  assert.equal((await server.call("/api/settings/private-route", privateRoute, { origin: "https://other.example" })).status, 403);
  await assert.rejects(readFile(join(server.fake.state, "invocations.log")), { code: "ENOENT" });
});

test("private route sends the token only on stdin, then checks once", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  const token = "private-route-token-$()\nline-two";
  const before = await server.call("/api/settings/private-route");
  assert.equal(before.status, 200);
  assert.deepEqual(JSON.parse(before.body), { route: null, token: "unset" });
  assert.ok(!before.body.includes(".office"));
  const saved = await server.call("/api/settings/private-route", { ...privateRoute, token });
  assert.equal(saved.status, 200);
  const body = JSON.parse(saved.body);
  assert.deepEqual(body.route, privateRoute);
  assert.equal(body.check.routable, true);
  assert.deepEqual(body.check.capabilities, ["text", "image"]);
  assert.equal(body.check.checks.find((line) => line.capability === "audio").note, audioNote);
  assert.equal(body.check.checks.find((line) => line.capability === "audio").accepted, false);
  assert.equal("token" in body, false);
  assert.ok(!saved.body.includes(".office"));
  assert.ok(!saved.body.includes(token));
  const stdin = (await readFile(join(server.fake.state, "secret-stdin.log"), "utf8")).trim().split("\n").map(JSON.parse);
  assert.deepEqual(stdin, [{ slot: "open_weights_token", value: token }]);
  const invocations = await invocationsOf(server.fake.state);
  assert.deepEqual(invocations.filter((args) => args[0] === "plugin"), [["plugin", "secret", "set", "office", "open_weights_token"]]);
  const names = invocations.map((args) => args[0] === "plugin" ? `secret:${args[4]}` : args[2]);
  const start = names.indexOf("office.config-set");
  assert.deepEqual(names.slice(start, start + 3), ["office.config-set", "secret:open_weights_token", "office.private-route-check"]);
  const configArgs = invocations.find((args) => args[2] === "office.config-set");
  assert.deepEqual(JSON.parse(configArgs[configArgs.indexOf("--input") + 1]), { key: "open_weights", value: privateRoute });
  const argvText = await readFile(join(server.fake.state, "invocations.log"), "utf8");
  assert.ok(!argvText.includes("private-route-token"));
  for (const line of server.logs) assert.ok(!line.includes("private-route-token"), "server logs exclude the token");
  for (const response of server.responses) assert.ok(!JSON.stringify(response).includes("private-route-token"), "HTTP responses exclude the token");
  const after = await server.call("/api/settings/private-route");
  assert.equal(after.status, 200);
  assert.equal(JSON.parse(after.body).token, "set");
  assert.equal(JSON.parse(after.body).route.endpoint, privateRoute.endpoint);
  assert.equal(JSON.parse(after.body).route.capabilities.includes("text"), true);
  assert.ok(!after.body.includes(token));
  const connections = await server.call("/api/settings/connections");
  assert.equal(JSON.parse(connections.body).connections[0].idempotency_header, "Idempotency-Key");
});

test("private route without a token writes the block and checks exactly once", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  const saved = await server.call("/api/settings/private-route", privateRoute);
  assert.equal(saved.status, 200);
  const invocations = await invocationsOf(server.fake.state);
  assert.deepEqual(invocations.map((args) => args[0] === "plugin" ? args : args[2]), ["office.config-set", "office.private-route-check"]);
  await assert.rejects(readFile(join(server.fake.state, "secret-stdin.log")), { code: "ENOENT" });
});

test("a private-route token failure does not check and does not leak the token", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t, { FAKE_SECRET_FAIL: "1" });
  const token = "private-route-token-on-failure";
  const saved = await server.call("/api/settings/private-route", { ...privateRoute, token });
  assert.equal(saved.status, 502);
  assert.deepEqual(JSON.parse(saved.body), { error: "secret_not_saved", route_saved: true });
  const invocations = await invocationsOf(server.fake.state);
  assert.deepEqual(invocations.map((args) => args[0] === "plugin" ? "secret" : args[2]), ["office.config-set", "secret"]);
  assert.deepEqual(invocations[1], ["plugin", "secret", "set", "office", "open_weights_token"]);
  for (const line of server.logs) assert.ok(!line.includes(token));
  for (const response of server.responses) assert.ok(!JSON.stringify(response).includes(token));
  const argvText = await readFile(join(server.fake.state, "invocations.log"), "utf8");
  assert.ok(!argvText.includes(token));
});

test("a private-route check failure is not forwarded", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  await writeFile(join(server.fake.state, "office.json"), JSON.stringify({
    fail: { "office.private-route-check": "fixture-diagnostic-do-not-forward" },
  }));
  const saved = await server.call("/api/settings/private-route", privateRoute);
  assert.equal(saved.status, 502);
  assert.deepEqual(JSON.parse(saved.body), { error: "private_route_check_failed", route_saved: true });
  assert.ok(!saved.body.includes("fixture-diagnostic-do-not-forward"));
  const invocations = await invocationsOf(server.fake.state);
  assert.deepEqual(invocations.map((args) => args[2]), ["office.config-set", "office.private-route-check"]);
});

test("a private-route check that echoes the token is refused", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  const token = "echo-this-token-9f3a";
  await writeFile(join(server.fake.state, "office.json"), JSON.stringify({
    tools: { "office.private-route-check": { ok: true, output: {
      label: "Acme's server", kind: "company", model: "muse-spark-open", routable: true,
      capabilities: ["text"], checks: [{ capability: "text", accepted: false, elapsed_ms: 1, error: "model_request_rejected", note: token }],
    } } },
  }));
  const saved = await server.call("/api/settings/private-route", { ...privateRoute, token });
  assert.equal(saved.status, 502);
  assert.deepEqual(JSON.parse(saved.body), { error: "private_route_check_failed", route_saved: true });
  assert.ok(!saved.body.includes(token));
  for (const line of server.logs) assert.ok(!line.includes(token));
  const argvText = await readFile(join(server.fake.state, "invocations.log"), "utf8");
  assert.ok(!argvText.includes(token));
  const stdin = JSON.parse((await readFile(join(server.fake.state, "secret-stdin.log"), "utf8")).trim());
  assert.equal(stdin.value, token);
});

test("private route status drops an unexpected field instead of returning it", { timeout: 15000 }, async (t) => {
  const server = await gatewayServer(t);
  const token = "hostile-status-token";
  await writeFile(join(server.fake.state, "office.json"), JSON.stringify({
    tools: { "office.status": { ok: true, output: {
      version: "0.1.0",
      secrets: { meta_api_key: "unset", open_weights_token: "unset" },
      open_weights: { endpoint: privateRoute.endpoint, model: privateRoute.model, kind: privateRoute.kind, label: privateRoute.label, capabilities: null, verified_at: null, token },
    } } },
  }));
  const response = await server.call("/api/settings/private-route");
  assert.equal(response.status, 502);
  assert.equal(JSON.parse(response.body).error, "private_route_unavailable");
  assert.ok(!response.body.includes(token));
  assert.ok(!response.body.includes(".office"));
});

test("a private-route save shares the settings write lock", { timeout: 5000 }, async (t) => {
  let entered;
  const gate = new Promise((resolve) => { entered = resolve; });
  let release;
  const waiting = new Promise((resolve) => { release = resolve; });
  const calls = [];
  const feature = createSettingsFeature({ async callTool(name) {
    calls.push(name);
    if (name === "office.config-set") return { ok: true, output: { path: ".office/config.yaml", key: "open_weights" } };
    if (name === "office.private-route-check") { entered(); await waiting; return { ok: false, error: { code: "fixture_failed", message: "do-not-forward" } }; }
    if (name === "gateway.connection-put") return { connection: { ...connectionInput, slot: "conn_03", slot_state: "unset" }, created: true, released_slot: null };
    throw new Error(name);
  } });
  const server = createServer((req, res) => {
    if (!checkLoopbackRequest(req).ok) { res.writeHead(403); res.end(); return; }
    const route = feature.routes.find((item) => item.method === req.method && item.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  t.after(() => new Promise((resolve) => { release(); server.closeAllConnections(); server.close(resolve); }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const headers = { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}`, "content-type": "application/json" };
  const first = request(port, { method: "POST", path: "/api/settings/private-route", headers, body: JSON.stringify(privateRoute) });
  try {
    await gate;
    const second = await request(port, { method: "POST", path: "/api/settings/connections/put", headers, body: JSON.stringify(connectionInput) });
    assert.equal(second.status, 503);
    assert.equal(JSON.parse(second.body).error, "settings_busy");
    assert.deepEqual(calls, ["office.config-set", "office.private-route-check"]);
  } finally { release(); }
  const saved = await first;
  assert.equal(saved.status, 502);
  assert.deepEqual(JSON.parse(saved.body), { error: "private_route_check_failed", route_saved: true });
  assert.ok(!saved.body.includes("do-not-forward"));
});

test("a refused private-route config-set does not write the token or check", { timeout: 5000 }, async (t) => {
  const calls = [];
  const feature = createSettingsFeature({ async callTool(name) {
    calls.push(name);
    return { ok: false, error: { code: "invalid_library_setting", message: "open_weights.endpoint https://secret.example/token" } };
  } });
  const server = createServer((req, res) => {
    if (!checkLoopbackRequest(req).ok) { res.writeHead(403); res.end(); return; }
    const route = feature.routes.find((item) => item.method === req.method && item.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const response = await request(port, {
    method: "POST", path: "/api/settings/private-route",
    headers: { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}`, "content-type": "application/json" },
    body: JSON.stringify({ ...privateRoute, token: "not-written" }),
  });
  assert.equal(response.status, 400);
  assert.deepEqual(JSON.parse(response.body), { error: "invalid_input" });
  assert.ok(!response.body.includes("secret.example"));
  assert.ok(!response.body.includes("not-written"));
  assert.deepEqual(calls, ["office.config-set"]);
});

// Shared library: every test drives the feature's own routes over HTTP with an
// in-memory Orbit client. No router, no real HOME, no git.

const sharedBlock = { remote: "git@github.com:acme-team/team-library.git", branch: "main", member: "daniel" };

function routineRow(effective) {
  return { name: "office-shared-sync", description: "Sync the shared library", enabled: effective, effective, target: "plugin:office", schedule: { every_minutes: 60 } };
}

function routinePage(rows) {
  return { items: rows, pagination: { next_offset: null }, notes: [] };
}

/** A finished `office.library-sync` result: three paths of one source, one page, one incoming update. */
const syncResult = {
  direction: "both", dry_run: false,
  pulled: ["knowledge/clients/dom-client-acme.md", "knowledge/runbooks/rb-lantern-spool.md"],
  pushed: [
    "members/daniel/sources/2026/10/src-20261028-lantern-kt/source.md",
    "members/daniel/sources/2026/10/src-20261028-lantern-kt/v1.md",
    "knowledge/topics/dom-lantern.md",
  ],
  removed: [],
  incoming: [{ page: "dom-client-acme", proposal: "task-1", created: true }],
  conflicts: [],
  skipped: [{ phase: "push", path: "knowledge/topics/dom-scratch-notes.md", id: "dom-scratch-notes", reason: "share_cites_unshared" }],
  commit: "0123456789abcdef0123456789abcdef01234567",
  synced_at: "2026-10-28T09:00:00Z",
};

async function sharedServer(t, client, options = {}) {
  const feature = createSettingsFeature(client, {}, options);
  const server = createServer((req, res) => {
    if (!checkLoopbackRequest(req).ok) { res.writeHead(403); res.end(); return; }
    const route = feature.routes.find((item) => item.method === req.method && item.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); }));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  const headers = { host: `127.0.0.1:${port}`, origin: `http://127.0.0.1:${port}`, "content-type": "application/json" };
  return {
    get: (path) => request(port, { path, headers: { host: headers.host } }),
    post: (path, body) => request(port, { method: "POST", path, headers, body: JSON.stringify(body) }),
  };
}

test("saving the shared library calls office.config-set with the shared block and offers the hourly routine", { timeout: 5000 }, async (t) => {
  const calls = [];
  let shared = null;
  let effective = false;
  const app = await sharedServer(t, { async callTool(name, input) {
    calls.push([name, input]);
    if (name === "office.status") return { ok: true, output: { initialized: true, shared } };
    if (name === "office.config-set") { shared = input.value; return { ok: true, output: { path: ".office/config.yaml", key: "shared" } }; }
    if (name === "orbit.routine.control" && input.action === "list") return routinePage([routineRow(effective)]);
    if (name === "orbit.routine.control" && input.action === "toggle") { effective = input.enabled; return { name: input.name, enabled: input.enabled }; }
    throw new Error(name);
  } });

  const before = await app.get("/api/settings/shared-library");
  assert.equal(before.status, 200);
  assert.deepEqual(JSON.parse(before.body), { shared: null, last_sync: null });

  const saved = await app.post("/api/settings/shared-library", sharedBlock);
  assert.equal(saved.status, 200);
  assert.deepEqual(JSON.parse(saved.body), { shared: sharedBlock, routine: "off" });
  assert.deepEqual(calls.find(([name]) => name === "office.config-set"), ["office.config-set", { key: "shared", value: sharedBlock }]);

  const enabled = await app.post("/api/settings/shared-library/routine", { enabled: true });
  assert.equal(enabled.status, 200);
  const toggle = calls.find(([name, input]) => name === "orbit.routine.control" && input.action === "toggle");
  assert.deepEqual(toggle[1], { action: "toggle", name: "office-shared-sync", expected_enabled: false, enabled: true, target: "plugin:office" });

  const after = await app.get("/api/settings/shared-library");
  assert.deepEqual(JSON.parse(after.body).shared, sharedBlock);
  const again = await app.post("/api/settings/shared-library", sharedBlock);
  assert.equal(JSON.parse(again.body).routine, "on");
});

test("a refused remote names the field and never echoes its value", { timeout: 5000 }, async (t) => {
  const calls = [];
  const app = await sharedServer(t, { async callTool(name) {
    calls.push(name);
    if (name === "office.config-set") {
      return { ok: false, error: { code: "invalid_library_setting", message: "shared.remote is not an accepted remote: https://alice:hunter2@git.example/team.git" } };
    }
    throw new Error(name);
  } });
  const remote = "https://alice:hunter2@git.example/team.git";
  const response = await app.post("/api/settings/shared-library", { ...sharedBlock, remote });
  assert.equal(response.status, 400);
  assert.deepEqual(JSON.parse(response.body), { error: "invalid_input", field: "remote" });
  assert.ok(!response.body.includes("hunter2"));
  assert.ok(!response.body.includes("git.example"));
  assert.deepEqual(calls, ["office.config-set"]);
});

test("a malformed member or an unknown key is refused before any tool call", { timeout: 5000 }, async (t) => {
  const calls = [];
  const app = await sharedServer(t, { async callTool(name) { calls.push(name); throw new Error(name); } });
  const member = await app.post("/api/settings/shared-library", { ...sharedBlock, member: "Daniel Kim" });
  assert.equal(member.status, 400);
  assert.deepEqual(JSON.parse(member.body), { error: "invalid_input", field: "member" });
  const extra = await app.post("/api/settings/shared-library", { ...sharedBlock, token: "x" });
  assert.deepEqual(JSON.parse(extra.body), { error: "invalid_input" });
  const routine = await app.post("/api/settings/shared-library/routine", { enabled: false });
  assert.equal(routine.status, 400);
  const sync = await app.post("/api/settings/shared-library/sync", { direction: "push" });
  assert.equal(sync.status, 400);
  assert.deepEqual(calls, []);
});

test("Sync now calls office.library-sync and reports document counts, remembered for Today", { timeout: 5000 }, async (t) => {
  const syncCalls = [];
  const app = await sharedServer(t, { async callTool(name) {
    if (name === "office.status") return { ok: true, output: { shared: sharedBlock } };
    throw new Error(name);
  } }, { syncClient: { async callTool(name, input) { syncCalls.push([name, input]); return { ok: true, output: syncResult }; } } });

  const response = await app.post("/api/settings/shared-library/sync", {});
  assert.equal(response.status, 200);
  const sync = {
    synced_at: "2026-10-28T09:00:00Z", shared: 2, removed: 0, received: 2, incoming: 1, conflicts: 0,
    skipped: [{ phase: "push", name: "Scratch notes", reason: "share_cites_unshared" }],
  };
  assert.deepEqual(JSON.parse(response.body), sync);
  assert.deepEqual(syncCalls, [["office.library-sync", {}]]);
  assert.ok(!response.body.includes("dom-scratch-notes"));

  const last = await app.get("/api/settings/shared-library/last-sync");
  assert.deepEqual(JSON.parse(last.body), { last_sync: { ok: true, sync } });
  const status = await app.get("/api/settings/shared-library");
  assert.deepEqual(JSON.parse(status.body), { shared: sharedBlock, last_sync: { ok: true, sync } });

  assert.equal((await app.post("/api/settings/shared-library/dismiss-skipped", {})).status, 200);
  const dismissed = await app.get("/api/settings/shared-library/last-sync");
  assert.deepEqual(JSON.parse(dismissed.body), { last_sync: { ok: true, sync: { ...sync, skipped: [] } } });
});

test("an unreachable shared library is reported as its reason, without git's diagnostics", { timeout: 5000 }, async (t) => {
  let now = 0;
  const app = await sharedServer(t, { async callTool(name) { throw new Error(name); } }, { syncClient: { async callTool() {
    now += 1;
    if (now === 1) return { ok: false, error: { code: "shared_remote_unreachable", message: "could not reach git.example: fatal: Authentication failed for alice" } };
    throw new OrbitExitError("office.library-sync", 1, null, `${JSON.stringify({ error: { code: "shared_remote_unreachable", message: "could not reach git.example" } })}\n`);
  } } });

  for (let attempt = 0; attempt < 2; attempt += 1) {
    const response = await app.post("/api/settings/shared-library/sync", {});
    assert.equal(response.status, 409);
    assert.deepEqual(JSON.parse(response.body), { error: "sync_failed", reason: "shared_remote_unreachable" });
    assert.ok(!response.body.includes("git.example"));
    assert.ok(!response.body.includes("alice"));
  }
  const last = JSON.parse((await app.get("/api/settings/shared-library/last-sync")).body).last_sync;
  assert.equal(last.ok, false);
  assert.equal(last.reason, "shared_remote_unreachable");
  assert.ok(!Number.isNaN(Date.parse(last.at)));
});

test("a sync that times out, an unknown refusal and a second concurrent sync are each reported in a closed reason", { timeout: 5000 }, async (t) => {
  let entered;
  const gate = new Promise((resolve) => { entered = resolve; });
  let release;
  const waiting = new Promise((resolve) => { release = resolve; });
  let calls = 0;
  const app = await sharedServer(t, { async callTool(name) { throw new Error(name); } }, { syncClient: { async callTool() {
    calls += 1;
    if (calls === 1) { entered(); await waiting; throw new OrbitTimeoutError("office.library-sync", 360000); }
    return { ok: false, error: { code: "fixture_internal_code", message: "do-not-forward" } };
  } } });
  const first = app.post("/api/settings/shared-library/sync", {});
  try {
    await gate;
    const second = await app.post("/api/settings/shared-library/sync", {});
    assert.equal(second.status, 409);
    assert.deepEqual(JSON.parse(second.body), { error: "sync_failed", reason: "library_writer_busy" });
  } finally { release(); }
  assert.deepEqual(JSON.parse((await first).body), { error: "sync_failed", reason: "sync_timeout" });
  const unknown = await app.post("/api/settings/shared-library/sync", {});
  assert.deepEqual(JSON.parse(unknown.body), { error: "sync_failed", reason: "unknown" });
  assert.ok(!unknown.body.includes("do-not-forward"));
  assert.equal(calls, 2);
});

test("a dry-run or malformed sync result is not reported as a finished sync", { timeout: 5000 }, async (t) => {
  const app = await sharedServer(t, { async callTool(name) { throw new Error(name); } }, { syncClient: { async callTool() {
    return { ok: true, output: { ...syncResult, dry_run: true, synced_at: null } };
  } } });
  const response = await app.post("/api/settings/shared-library/sync", {});
  assert.equal(response.status, 502);
  assert.deepEqual(JSON.parse(response.body), { error: "sync_failed", reason: "unknown" });
});

test("an unreadable routine listing is reported as unavailable, and turning it on refuses", { timeout: 5000 }, async (t) => {
  const app = await sharedServer(t, { async callTool(name) {
    if (name === "office.config-set") return { ok: true, output: { path: ".office/config.yaml", key: "shared" } };
    if (name === "orbit.routine.control") return routinePage([]);
    throw new Error(name);
  } });
  const saved = await app.post("/api/settings/shared-library", sharedBlock);
  assert.equal(JSON.parse(saved.body).routine, "unavailable");
  const enabled = await app.post("/api/settings/shared-library/routine", { enabled: true });
  assert.equal(enabled.status, 502);
  assert.deepEqual(JSON.parse(enabled.body), { error: "routine_unavailable" });
});
