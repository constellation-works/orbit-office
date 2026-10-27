import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { acceptProposal, acceptProposals, ApiError, approveOutput, dismissNeed, getApiCalls, getApprovalDetail, sendBack, sendDraft, getConnections, getHealth, getJson, getPrivateRoute, putConnection, savePrivateRoute } from "../dist/api/client.js";

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

test("approval helpers post closed bodies and carry a refusal reason", async () => {
  const calls = [];
  globalThis.fetch = async (path, options) => {
    calls.push({ path, method: options.method ?? "GET", body: options.body === undefined ? undefined : JSON.parse(options.body) });
    return new Response(JSON.stringify({ ok: true }));
  };
  const sha = "c".repeat(64);
  await getApprovalDetail("ORB-1 x");
  await approveOutput("ORB-1");
  await sendBack("ORB-1", "More detail");
  await dismissNeed("ORB-2");
  await dismissNeed("ORB-2", "Not now");
  await acceptProposal("ORB-3", sha);
  await acceptProposals([{ key: "ORB-3", sha256: sha }]);
  await sendDraft("ORB-4", sha);
  assert.deepEqual(calls, [
    { path: "/api/approvals/detail?key=ORB-1%20x", method: "GET", body: undefined },
    { path: "/api/approvals/approve", method: "POST", body: { key: "ORB-1" } },
    { path: "/api/approvals/send-back", method: "POST", body: { key: "ORB-1", comment: "More detail" } },
    { path: "/api/approvals/dismiss", method: "POST", body: { key: "ORB-2" } },
    { path: "/api/approvals/dismiss", method: "POST", body: { key: "ORB-2", reason: "Not now" } },
    { path: "/api/approvals/accept", method: "POST", body: { key: "ORB-3", sha256: sha } },
    { path: "/api/approvals/accept-all", method: "POST", body: { proposals: [{ key: "ORB-3", sha256: sha }] } },
    { path: "/api/approvals/send", method: "POST", body: { key: "ORB-4", sha256: sha } },
  ]);
  globalThis.fetch = async () => new Response('{"error":"promote_refused","reason":"task_has_no_artifacts"}', { status: 409 });
  await assert.rejects(approveOutput("ORB-1"), (error) =>
    error instanceof ApiError && error.status === 409 && error.code === "promote_refused" && error.reason === "task_has_no_artifacts");
});
