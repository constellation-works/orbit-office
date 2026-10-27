import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { ApiError, getHealth, getJson } from "../dist/api/client.js";

const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; });

test("health requests return the contract", async () => {
  globalThis.fetch = async (path, options) => {
    assert.equal(path, "/healthz");
    assert.equal(options.headers.accept, "application/json");
    return new Response(JSON.stringify({ ok: true, app: "orbit-office", features: [] }));
  };
  assert.deepEqual(await getHealth(), { ok: true, app: "orbit-office", features: [] });
});

test("HTTP, network and invalid JSON failures use ApiError", async () => {
  for (const [fetcher, status, code] of [
    [async () => new Response('{"error":"unavailable"}', { status: 503 }), 503, "unavailable"],
    [async () => { throw new TypeError("network"); }, 0, "network_error"],
    [async () => new Response("not json"), 200, "invalid_json"],
  ]) {
    globalThis.fetch = fetcher;
    await assert.rejects(getJson("/example"), (error) => error instanceof ApiError && error.status === status && error.code === code);
  }
});
