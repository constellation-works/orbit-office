import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, mkdir, writeFile, rm, cp } from "node:fs/promises";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { test } from "node:test";
import { createRoutinesFeature } from "../dist/features/routines/index.js";
const listing = [{ schemaVersion: 1, name: "office-review", description: "process new meetings", enabled: true, schedule: { every_minutes: 15 }, template: { title: "Process meetings" }, effective_enabled: false, skipped_reason: "seeded by plugin:office@1, which is switched off in this workspace" }];
test("routines HTTP routes list definitions, toggle once, and forward retention refusals", async t => {
  const calls = [];
  const client = { async callTool(name, input) {
    calls.push([name, input]);
    if (name === "orbit.auto_task.list") return listing;
    if (name === "orbit.routine.control") return { items: [], pagination: { next_offset: null }, notes: [] };
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
  assert.deepEqual(result.routines[0], { kind: "auto_task", name: "office-review", description: "process new meetings", schedule: { every_minutes: 15 }, enabled: false, skipped_reason: listing[0].skipped_reason, last_fire: null });
  const post = (path, data) => fetch(`${url}${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(data) });
  assert.equal((await post("/api/routines/toggle", { name: "office-review", enabled: true })).status, 200);
  assert.deepEqual(calls.filter(([name]) => name === "orbit.auto_task.update"), [["orbit.auto_task.update", { name: "office-review", enabled: true }]]);
  const refused = await post("/api/routines/retention", { kind: "meeting", value: -1 });
  assert.equal(refused.status, 400);
  assert.equal((await refused.json()).reason, "Retention must be a positive number of days.");
  assert.deepEqual(calls.at(-1), ["office.config-set", { key: "retention.meeting", value: -1 }]);
  assert.equal((await post("/api/routines/toggle", { name: "meeting-sweep", enabled: true, extra: 1 })).status, 400);
});

async function fixture(t) {
  const scratch = new URL("../../../../.orbit/tmp/", import.meta.url);
  await mkdir(scratch, { recursive: true });
  const root = await mkdtemp(new URL("routines-test-", scratch));
  t.after(() => rm(root, { recursive: true, force: true }));
  const roots = [join(root, "office"), join(root, "gateway")];
  for (const path of roots) await mkdir(path);
  return { roots, async definitions(index, routines, autoTasks) {
    const paths = names => names.map(name => `definitions/${name}.yaml`);
    await mkdir(join(roots[index], "definitions"), { recursive: true });
    await writeFile(join(roots[index], "plugin.yaml"), `schemaVersion: 2\nkind: Plugin\nmetadata:\n  name: ${index === 0 ? "office" : "gateway"}\nspec:\n  definitions:\n    routines: [${paths(routines).map(path => JSON.stringify(path)).join(", ")}]\n    auto_tasks:\n${paths(autoTasks).map(path => `      - '${path}'\n`).join("") || "      []\n"}`);
    for (const name of [...routines, ...autoTasks]) await writeFile(join(roots[index], "definitions", `${name}.yaml`), `schemaVersion: 1\nname: '${name}' # the definition's name, not its filename\n`);
  } };
}

async function serve(t, feature) {
  const server = createServer((req, res) => {
    const route = feature.routes.find(r => r.method === req.method && r.path === req.url);
    if (route) void route.handle(req, res); else { res.writeHead(404); res.end(); }
  });
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  t.after(() => new Promise(resolve => server.close(resolve)));
  return `http://127.0.0.1:${server.address().port}`;
}

const autoTask = name => ({ name, description: `${name} description`, schedule: { every_minutes: 15 }, effective_enabled: false });
const routine = name => ({ name, description: `${name} description`, schedule: { cron: "5 0 * * *" }, enabled: true, effective: true, target: `job:${name}`, last_fire: null });

test("HTTP listing filters both kinds by shipped office/gateway names and discovers an added definition", async t => {
  const files = await fixture(t);
  await files.definitions(0, ["daily-digest"], ["review"]);
  await files.definitions(1, ["gateway-sync"], ["gateway-audit"]);
  // A file is shipped only when declared by the manifest. A prefix or tag is not membership.
  await writeFile(join(files.roots[0], "definitions", "unshipped.yaml"), "name: unshipped\n");
  const autos = [autoTask("review"), autoTask("gateway-audit"), autoTask("office-fake"), autoTask("qa-sweep"), autoTask("unshipped"), autoTask("daily-digest")];
  const jobs = [routine("daily-digest"), routine("gateway-sync"), routine("code-review")];
  jobs[0].last_fire = { at: "2026-10-27T00:05:00Z", state: "completed", run_id: "private-run-id" };
  const calls = [];
  const client = { async callTool(name, input) {
    calls.push([name, input]);
    if (name === "orbit.auto_task.list") return autos;
    assert.equal(name, "orbit.routine.control");
    if (input.action === "toggle") return {};
    const offset = input.offset ?? 0;
    // Exercise pagination even with a small fixture.
    return { items: jobs.slice(offset, offset + 1), pagination: { next_offset: offset + 1 < jobs.length ? offset + 1 : null }, notes: [] };
  } };
  const start = () => serve(t, createRoutinesFeature(client, { pluginRoots: files.roots }));
  const url = await start();
  const initial = await (await fetch(`${url}/api/routines`)).json();
  assert.deepEqual(initial.routines.map(row => [row.kind, row.name]), [["auto_task", "review"], ["auto_task", "gateway-audit"], ["routine", "daily-digest"], ["routine", "gateway-sync"]]);
  assert.equal(initial.hidden_count, 5);
  assert.equal(initial.routines_unavailable, false);
  assert.equal(initial.routines[2].enabled, true);
  assert.deepEqual(initial.routines[2].last_fire, { kind: "fired", at: "2026-10-27T00:05:00Z", task_status: null, run_state: "completed" });
  const toggled = await fetch(`${url}/api/routines/toggle`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ kind: "routine", name: "daily-digest", enabled: false }) });
  assert.equal(toggled.status, 200);
  assert.deepEqual(calls.at(-1), ["orbit.routine.control", { action: "toggle", name: "daily-digest", expected_enabled: true, enabled: false, target: "job:daily-digest" }]);
  const before = calls.length;
  const hidden = await fetch(`${url}/api/routines/toggle`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ name: "qa-sweep", enabled: false }) });
  assert.equal(hidden.status, 400);
  assert.equal(calls.length, before);
  await files.definitions(0, ["daily-digest", "new-summary"], ["review"]);
  await writeFile(join(files.roots[0], "definitions", "new-summary.yaml"), 'name: "added-summary"\n');
  jobs.push(routine("added-summary"));
  // Restart the feature: no application code or name list changes.
  const restarted = await start();
  const updated = await (await fetch(`${restarted}/api/routines`)).json();
  assert.ok(updated.routines.some(row => row.name === "added-summary" && row.kind === "routine"));
  assert.equal(updated.hidden_count, 5);
});

test("malformed manifest, escaping definition and malformed listing fail visibly", async t => {
  const files = await fixture(t);
  await files.definitions(0, [], ["review"]);
  await files.definitions(1, [], ["audit"]);
  const client = { async callTool(name) {
    if (name === "orbit.auto_task.list") return [autoTask("review")];
    return { items: [routine("qa-sweep")], pagination: { next_offset: 0 }, notes: [] };
  } };
  let url = await serve(t, createRoutinesFeature(client, { pluginRoots: files.roots }));
  assert.equal((await fetch(`${url}/api/routines`)).status, 502);
  await writeFile(join(files.roots[0], "plugin.yaml"), "spec: { definitions: broken }\n");
  url = await serve(t, createRoutinesFeature(client, { pluginRoots: files.roots }));
  assert.equal((await fetch(`${url}/api/routines`)).status, 502);
  await writeFile(join(files.roots[0], "plugin.yaml"), "spec:\n  definitions:\n    auto_tasks: [../gateway/definitions/audit.yaml]\n");
  url = await serve(t, createRoutinesFeature(client, { pluginRoots: files.roots }));
  assert.equal((await fetch(`${url}/api/routines`)).status, 502);
});

test("routine permission refusal preserves filtered auto-tasks and marks the count incomplete", async t => {
  const files = await fixture(t);
  await files.definitions(0, [], ["review"]);
  await files.definitions(1, [], ["audit"]);
  const client = { async callTool(name) {
    if (name === "orbit.auto_task.list") return [autoTask("review"), autoTask("qa-sweep")];
    throw Object.assign(new Error("operator required"), { code: "not_permitted" });
  } };
  const url = await serve(t, createRoutinesFeature(client, { pluginRoots: files.roots }));
  const result = await (await fetch(`${url}/api/routines`)).json();
  assert.deepEqual(result.routines.map(row => row.name), ["review"]);
  assert.equal(result.hidden_count, 1);
  assert.equal(result.routines_unavailable, true);
});

test("missing shipped definitions refuse the snapshot instead of hiding everything", async t => {
  const files = await fixture(t);
  await files.definitions(0, [], ["review"]);
  await files.definitions(1, [], ["audit"]);
  await rm(join(files.roots[0], "definitions", "review.yaml"));
  const url = await serve(t, createRoutinesFeature({ async callTool() { return []; } }, { pluginRoots: files.roots }));
  assert.equal((await fetch(`${url}/api/routines`)).status, 502);
});

test("a standalone app reads the shipped trees in its bundle without a source checkout", async t => {
  const files = await fixture(t);
  await files.definitions(0, [], ["review"]);
  await files.definitions(1, [], ["audit"]);
  const app = join(files.roots[0], "app");
  const featureDir = join(app, "packages/server/dist/features/routines");
  const orbitDir = join(app, "packages/server/dist/orbit");
  await mkdir(featureDir, { recursive: true });
  await mkdir(orbitDir, { recursive: true });
  await writeFile(join(app, "package.json"), '{"type":"module"}');
  for (const file of ["index.js", "definitions.js"]) await cp(new URL(`../dist/features/routines/${file}`, import.meta.url), join(featureDir, file));
  await cp(new URL("../dist/orbit/client.js", import.meta.url), join(orbitDir, "client.js"));
  for (const [index, name] of ["office", "gateway"].entries()) {
    const target = join(app, "plugins", name, ".orbit-plugin");
    await mkdir(target, { recursive: true });
    // Copy just the runtime resources, never the enclosing fixture/app itself.
    await cp(join(files.roots[index], "plugin.yaml"), join(target, "plugin.yaml"));
    await cp(join(files.roots[index], "definitions"), join(target, "definitions"), { recursive: true });
  }
  const bundled = await import(pathToFileURL(join(featureDir, "index.js")).href);
  const client = { async callTool(name) {
    return name === "orbit.auto_task.list" ? [autoTask("review"), autoTask("qa-sweep")]
      : { items: [], pagination: { next_offset: null }, notes: [] };
  } };
  const url = await serve(t, bundled.createRoutinesFeature(client));
  const result = await (await fetch(`${url}/api/routines`)).json();
  assert.deepEqual(result.routines.map(row => row.name), ["review"]);
  assert.equal(result.hidden_count, 1);
});
