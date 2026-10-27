import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";

import { handle } from "../packages/server/src/http/router.js";

async function withServer<T>(run: (baseUrl: string) => Promise<T>): Promise<T> {
  const server = createServer(handle);
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });

  const address = server.address();
  if (address === null || typeof address === "string") {
    throw new Error("expected the test server to listen on a TCP port");
  }

  try {
    return await run(`http://127.0.0.1:${address.port}`);
  } finally {
    await new Promise<void>((resolve, reject) => {
      server.close((error) => (error ? reject(error) : resolve()));
    });
  }
}

test("GET /healthz reports the app and its built features", async () => {
  await withServer(async (baseUrl) => {
    const response = await fetch(`${baseUrl}/healthz`);

    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), {
      ok: true,
      app: "orbit-office",
      features: ["today", "todos", "settings"],
    });
  });
});

test("GET /api/settings reports the agent CLI connection shape", async () => {
  await withServer(async (baseUrl) => {
    const response = await fetch(`${baseUrl}/api/settings`);

    assert.equal(response.status, 200);
    const body = (await response.json()) as { agentCli?: { cli?: unknown; connected?: unknown } };
    assert.equal(typeof body.agentCli?.cli, "string");
    assert.equal(typeof body.agentCli?.connected, "boolean");
  });
});

test("the PWA shell is served: manifest, icons and worker", async () => {
  await withServer(async (baseUrl) => {
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
  await withServer(async (baseUrl) => {
    const response = await fetch(`${baseUrl}/missing`);

    assert.equal(response.status, 404);
    assert.deepEqual(await response.json(), { error: "not_found" });
  });
});
