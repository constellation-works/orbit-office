import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { ApiError, getApiCalls, getConnections, getHealth, getJson, getPrivateRoute, putConnection, savePrivateRoute } from "../dist/api/client.js";

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

test("private route helpers read status and post the block without putting the token in an error", async () => {
  const calls = [];
  const input = { endpoint: "https://muse.example/v1", model: "muse", kind: "company", label: "Acme", token: "route-token" };
  globalThis.fetch = async (path, options) => {
    calls.push({ path, options });
    if (options?.method === "POST") return new Response(JSON.stringify({ route: { label: "Acme" }, check: { routable: true, checks: [] } }));
    return new Response(JSON.stringify({ route: null, token: "unset" }));
  };
  assert.deepEqual(await getPrivateRoute(), { route: null, token: "unset" });
  assert.equal((await savePrivateRoute(input)).check.routable, true);
  assert.deepEqual(calls.map((call) => call.path), ["/api/settings/private-route", "/api/settings/private-route"]);
  assert.equal(calls[1].options.method, "POST");
  assert.deepEqual(JSON.parse(calls[1].options.body), input);
  globalThis.fetch = async () => new Response('{"error":"secret_not_saved","route_saved":true}', { status: 502 });
  await assert.rejects(savePrivateRoute(input), (error) => error instanceof ApiError && error.code === "secret_not_saved" && error.routeSaved === true && !error.message.includes(input.token));
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
