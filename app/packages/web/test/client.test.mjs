import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { ApiError, getApiCalls, getConnections, getHealth, getJson, putConnection } from "../dist/api/client.js";

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

test("Settings helpers read connections and audit and post entered fields", async () => {
  const calls = [];
  const input = { name: "acme", base_url: "https://example.com/", auth: { scheme: "bearer" }, secret: "fixture-secret" };
  globalThis.fetch = async (path, options) => {
    calls.push({ path, options });
    return new Response(JSON.stringify(path.endsWith("/put") ? { created: true } : { records: [] }));
  };
  await getConnections(); await getApiCalls();
  assert.deepEqual(await putConnection(input), { created: true });
  assert.deepEqual(calls.map((call) => call.path), ["/api/settings/connections", "/api/settings/api-calls", "/api/settings/connections/put"]);
  assert.equal(calls[2].options.method, "POST");
  assert.equal(calls[2].options.headers["content-type"], "application/json");
  assert.deepEqual(JSON.parse(calls[2].options.body), input);
  globalThis.fetch = async () => new Response('{"error":"secret_not_saved"}', { status: 502 });
  await assert.rejects(putConnection(input), (error) => error instanceof ApiError && error.code === "secret_not_saved" && !error.message.includes(input.secret));
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
