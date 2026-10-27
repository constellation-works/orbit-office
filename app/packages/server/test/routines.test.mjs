import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";
import { createRoutinesFeature } from "../dist/features/routines/index.js";
const listing = [{ schemaVersion: 1, name: "meeting-sweep", description: "process new meetings", enabled: true, schedule: { every_minutes: 15 }, template: { title: "Process meetings" }, effective_enabled: false, skipped_reason: "seeded by plugin:meetings@1, which is switched off in this workspace" }];
test("routines HTTP routes list definitions, toggle once, and forward retention refusals", async t => {
  const calls = [];
  const client = { async callTool(name, input) {
    calls.push([name, input]);
    if (name === "orbit.auto_task.list") return listing;
    if (name === "office.config-set") return { ok: false, error: { code: "invalid_library_setting", message: "Retention must be a positive number of days." } };
    return {};
  } };
  const feature = createRoutinesFeature(client);
  const server = createServer((req, res) => {
    const route = feature.routes.find(r => r.method === req.method && r.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  const url = `http://127.0.0.1:${server.address().port}`;
  const result = await (await fetch(`${url}/api/routines`)).json();
  assert.deepEqual(result.routines[0], { name: "meeting-sweep", description: "process new meetings", schedule: { every_minutes: 15 }, enabled: false, skipped_reason: listing[0].skipped_reason });
  const post = (path, data) => fetch(`${url}${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
  assert.equal((await post("/api/routines/toggle", { name: "meeting-sweep", enabled: true })).status, 200);
  assert.deepEqual(calls.filter(([name]) => name === "orbit.auto_task.update"), [["orbit.auto_task.update", { name: "meeting-sweep", enabled: true }]]);
  const refused = await post("/api/routines/retention", { kind: "meeting", value: -1 });
  assert.equal(refused.status, 400);
  assert.equal((await refused.json()).reason, "Retention must be a positive number of days.");
  assert.deepEqual(calls.at(-1), ["office.config-set", { key: "retention.meeting", value: -1 }]);
  assert.equal((await post("/api/routines/toggle", { name: "meeting-sweep", enabled: true, extra: 1 })).status, 400);
});
