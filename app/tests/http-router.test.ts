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
      features: [],
    });
  });
});

test("unknown paths return the not-found envelope", async () => {
  await withServer(async (baseUrl) => {
    const response = await fetch(`${baseUrl}/missing`);

    assert.equal(response.status, 404);
    assert.deepEqual(await response.json(), { error: "not_found" });
  });
});
