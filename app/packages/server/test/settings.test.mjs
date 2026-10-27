import assert from "node:assert/strict";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
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

function request(port, { method = "GET", path = "/", headers = {} } = {}) {
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
    req.end();
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
  assert.equal(
    mcpServersHasOrbit(JSON.stringify({ mcpServers: { helper: { url: "https://orbit.example.com/mcp" } } })),
    true,
  );
  assert.equal(mcpServersHasOrbit(JSON.stringify({ mcpServers: {} })), false);
  assert.equal(mcpServersHasOrbit(JSON.stringify({})), false);
  assert.equal(mcpServersHasOrbit(JSON.stringify({ mcpServers: "orbit" })), false);
  assert.equal(mcpServersHasOrbit("{not json"), false);
  assert.equal(mcpServersHasOrbit('"just a string"'), false);
});

test("Codex TOML counts an Orbit server section, nothing else", () => {
  assert.equal(tomlHasOrbitMcp('[mcp_servers.orbit]\ncommand = "orbit"\n'), true);
  assert.equal(tomlHasOrbitMcp('[mcp_servers.helper]\nurl = "https://orbit.example.com/mcp"\n'), true);
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
});
