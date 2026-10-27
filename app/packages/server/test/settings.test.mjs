import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer, request as httpRequest } from "node:http";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";

import {
  agentCliStatus,
  mcpServersHasOrbit,
  tomlHasOrbitMcp,
} from "../dist/features/settings/agent-cli.js";
import { createHandler } from "../dist/http/router.js";
import { createSettingsFeature } from "../dist/features/settings/index.js";
import { checkLoopbackRequest } from "../dist/http/guard.js";
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
