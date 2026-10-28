import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { createTodayFeature } from "../dist/features/today/index.js";
import { OrbitExitError } from "../dist/orbit/client.js";

test("setup routes report health, initialize once per request, and translate refusals", async (t) => {
  const state = { initialized: false, calls: [], error: null };
  const client = {
    async callTool(name, input) {
      state.calls.push({ name, input });
      if (state.error) throw state.error;
      if (name === "office.status") return { ok: true, output: { library: { initialized: state.initialized } } };
      const already = state.initialized;
      state.initialized = true;
      return { ok: true, output: { created: already ? [] : ["config"], already_initialized: already } };
    },
    async pollTool() { throw new Error("unexpected poll"); },
  };
  const feature = createTodayFeature({ client });
  const server = createServer((req, res) => {
    const route = feature.routes.find((item) => item.path === req.url && item.method === req.method);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  t.after(() => new Promise((done) => server.close(done)));
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const url = `http://127.0.0.1:${server.address().port}/api/setup`;
  assert.deepEqual(await (await fetch(url)).json(), { state: "missing" });
  assert.deepEqual(await (await fetch(url, { method: "POST" })).json(), { created: ["config"], already_initialized: false });
  assert.deepEqual(await (await fetch(url, { method: "POST" })).json(), { created: [], already_initialized: true });
  assert.equal(state.calls.filter((call) => call.name === "office.init").length, 2);
  assert.deepEqual(await (await fetch(url)).json(), { state: "ready" });
  for (const [message, expected] of [["tool not found: office.status", "plugin_disabled"], ["workspace is not registered", "workspace_unregistered"]]) {
    state.error = new OrbitExitError("office.status", 1, null, message);
    const result = await (await fetch(url)).json();
    assert.equal(result.state, expected);
    assert.match(result.fix, /orbit-office init/);
  }
});
