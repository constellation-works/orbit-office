import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";

/**
 * Root HTTP router integration suite.
 *
 * Every test runs the real production router (`handle` from the built
 * server) in an isolated child process. The child receives a minimal
 * environment built from scratch — only `PATH` is inherited — with
 * `HOME`, `XDG_CONFIG_HOME`, `CLAUDE_CONFIG_DIR` and `CODEX_HOME` all
 * bound to explicit temporary fixture roots created per test. The
 * parent test process environment is never read by the route under
 * test and never mutated, so the suite cannot read the invoking
 * user's agent configuration and parallel tests stay isolated
 * (AGENTS.md: tests never touch real host state).
 *
 * Fixture files are written relative to the temporary root, which
 * holds `home/`, `xdg/`, `claude/` and `codex/` subdirectories, e.g.
 * `xdg/muse/settings.json`, `claude/.claude.json`, `codex/config.toml`.
 * Requires the built server (`npm test` runs `tsc -b` first), matching
 * the `packages/server/test` suites that import from `dist/`.
 */
const ROUTER_URL = new URL("../packages/server/dist/http/router.js", import.meta.url).href;

const CHILD_SOURCE = `
import { createServer } from "node:http";
import { handle } from ${JSON.stringify(ROUTER_URL)};
const server = createServer(handle);
server.listen(0, "127.0.0.1", () => console.log(server.address().port));
`;

type FixtureFiles = Readonly<Record<string, string>>;

async function withServer<T>(
  files: FixtureFiles,
  run: (baseUrl: string) => Promise<T>,
): Promise<T> {
  const root = await mkdtemp(join(tmpdir(), "office-http-router-"));
  const home = join(root, "home");
  const xdg = join(root, "xdg");
  const claude = join(root, "claude");
  const codex = join(root, "codex");
  for (const dir of [home, xdg, claude, codex]) {
    await mkdir(dir, { recursive: true });
  }
  for (const [relative, content] of Object.entries(files)) {
    const path = join(root, relative);
    await mkdir(dirname(path), { recursive: true });
    await writeFile(path, content);
  }

  const child = spawn(process.execPath, ["--input-type=module", "-e", CHILD_SOURCE], {
    env: {
      PATH: process.env["PATH"] ?? "",
      HOME: home,
      XDG_CONFIG_HOME: xdg,
      CLAUDE_CONFIG_DIR: claude,
      CODEX_HOME: codex,
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const stderr: string[] = [];
  child.stderr.on("data", (chunk: Buffer) => stderr.push(chunk.toString()));

  try {
    const port = await new Promise<number>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`fixture server did not start: ${stderr.join("")}`));
      }, 10_000);
      child.once("error", (error) => {
        clearTimeout(timer);
        reject(error);
      });
      child.once("exit", (code) => {
        clearTimeout(timer);
        reject(new Error(`fixture server exited early (${code}): ${stderr.join("")}`));
      });
      child.stdout.once("data", (chunk: Buffer) => {
        clearTimeout(timer);
        const parsed = Number(chunk.toString().trim());
        if (!Number.isInteger(parsed) || parsed <= 0) {
          reject(new Error(`fixture server reported no port: ${stderr.join("")}`));
          return;
        }
        resolve(parsed);
      });
    });
    return await run(`http://127.0.0.1:${port}`);
  } finally {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill("SIGTERM");
      await Promise.race([
        new Promise<void>((resolve) => child.once("close", () => resolve())),
        new Promise<void>((resolve) =>
          setTimeout(() => {
            child.kill("SIGKILL");
            resolve();
          }, 5_000),
        ),
      ]);
    }
    await rm(root, { recursive: true, force: true });
  }
}

const ORBIT_MUSE_SETTINGS = JSON.stringify({
  mcpServers: { orbit: { command: "orbit" } },
});

test("GET /healthz reports the app and its built features", async () => {
  await withServer({}, async (baseUrl) => {
    const response = await fetch(`${baseUrl}/healthz`);

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      ok: true,
      app: "orbit-office",
      features: ["today", "calendar", "sources", "todos", "approvals", "routines", "library", "insights", "review", "selection", "settings"],
    });
  });
});

test("GET /api/settings reports disconnected with an empty agent fixture", async () => {
  await withServer({}, async (baseUrl) => {
    const response = await fetch(`${baseUrl}/api/settings`);

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      agentCli: { cli: "Muse Code", connected: false },
    });
  });
});

test("GET /api/settings reports connected with an Orbit Muse fixture", async () => {
  await withServer(
    { "xdg/muse/settings.json": ORBIT_MUSE_SETTINGS },
    async (baseUrl) => {
      const response = await fetch(`${baseUrl}/api/settings`);

      assert.equal(response.status, 200);
      assert.deepEqual(await response.json(), {
        agentCli: { cli: "Muse Code", connected: true },
      });
    },
  );
});

test("the PWA shell is served: manifest, icons and worker", async () => {
  await withServer({}, async (baseUrl) => {
    const manifest = await fetch(`${baseUrl}/manifest.webmanifest`);
    assert.equal(manifest.status, 200);
    assert.match(manifest.headers.get("content-type") ?? "", /application\/manifest\+json/);
    const body = (await manifest.json()) as {
      start_url?: unknown;
      icons?: { src?: unknown; sizes?: unknown }[];
    };
    assert.equal(body.start_url, "/");
    const sizes = (body.icons ?? []).map((icon) => icon.sizes);
    assert.ok(sizes.includes("192x192"), "manifest carries a 192px icon");
    assert.ok(sizes.includes("512x512"), "manifest carries a 512px icon");

    const icon = await fetch(`${baseUrl}/icons/icon-192.png`);
    assert.equal(icon.status, 200);
    assert.match(icon.headers.get("content-type") ?? "", /image\/png/);
    assert.ok((await icon.arrayBuffer()).byteLength > 0);

    const worker = await fetch(`${baseUrl}/pwa/sw.js`);
    assert.equal(worker.status, 200);
    assert.match(worker.headers.get("content-type") ?? "", /javascript/);
    assert.equal(worker.headers.get("service-worker-allowed"), "/");
  });
});

test("unknown paths return the not-found envelope", async () => {
  await withServer({}, async (baseUrl) => {
    const response = await fetch(`${baseUrl}/missing`);

    assert.equal(response.status, 404);
    assert.deepEqual(await response.json(), { error: "not_found" });
  });
});
