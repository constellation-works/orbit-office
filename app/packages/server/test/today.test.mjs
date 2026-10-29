import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { join } from "node:path";
import { test } from "node:test";

import { createTodayFeature } from "../dist/features/today/index.js";
import { createOrbitClient, OrbitExitError } from "../dist/orbit/client.js";
import { installFakeOrbit } from "./fake-orbit.mjs";
import { approvalTasks, fakeTodayClient, now, sharedTasks, todayTasks } from "./today-fixture.mjs";

async function endpoint(t, client) {
  const feature = createTodayFeature({ client, now: () => now });
  assert.equal(feature.status, "built");
  const server = createServer((request, response) => {
    const route = feature.routes.find((route) => route.method === request.method && route.path === request.url);
    if (route) void route.handle(request, response);
    else { response.writeHead(404); response.end(); }
  });
  t.after(() => new Promise((resolve) => server.close(resolve)));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return () => fetch(`http://127.0.0.1:${server.address().port}/api/today`);
}

test("Today HTTP sections contain proposals, reviews, your to-dos, active agents and today's outcomes", async (t) => {
  const fake = fakeTodayClient();
  const get = await endpoint(t, fake.client);
  const response = await get();
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("cache-control"), "no-store");
  const body = await response.json();
  assert.deepEqual(body.needs_you.map(({ key, kind }) => [key, kind]), [
    ["suggestion", "proposal"], ["review", "approval"], ["personal", "todo"],
  ]);
  assert.deepEqual(body.agents_working_on.map(({ key }) => key), ["working"]);
  assert.deepEqual(body.done_today.map(({ key }) => key), ["completed"]);
  assert.equal(body.done_today[0].result, todayTasks()[4].execution_summary);
  assert.equal(body.done_today[0].background, todayTasks()[4].description);
  assert.equal(body.truncated, false);
  assert.equal(body.completion_dates_unavailable, 0);
  assert.equal(fake.state.calls.length, 2);
  assert.ok(fake.state.calls.every(({ name, input }) => name === "orbit.task.list" && input.fields.includes("history")));
});

test("GET /api/today returns 200 from cwd / with the workspace binding set (real Orbit client)", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  await writeFile(join(fake.state, "tasks.json"), JSON.stringify(todayTasks()));
  const client = createOrbitClient({
    timeoutMs: 5_000,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: fake.root }),
  });
  const previousCwd = process.cwd();
  process.chdir("/");
  try {
    const get = await endpoint(t, client);
    const response = await get();
    assert.equal(response.status, 200);
    const body = await response.json();
    assert.ok(Array.isArray(body.needs_you));
  } finally {
    process.chdir(previousCwd);
  }
  const cwds = (await readFile(join(fake.state, "cwds.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
  assert.ok(cwds.length > 0);
  assert.ok(cwds.every((cwd) => cwd === fake.root), `child cwd must be the workspace, got ${cwds.join(", ")}`);
});

test("an empty Orbit workspace produces three empty sections", async (t) => {
  const get = await endpoint(t, fakeTodayClient([]).client);
  assert.deepEqual(await (await get()).json(), {
    needs_you: [], agents_working_on: [], done_today: [], truncated: false, completion_dates_unavailable: 0,
  });
});

test("personal ownership wins over agent ownership; proposal and review labels win over personal labels", async (t) => {
  const rows = todayTasks().slice(0, 4);
  rows[0].tags.push("owner:you");
  rows[1].tags.push("owner:you");
  rows[2].status = "blocked";
  rows[3].tags.push("owner:you");
  const get = await endpoint(t, fakeTodayClient(rows).client);
  const body = await (await get()).json();
  assert.deepEqual(body.needs_you.map(({ kind }) => kind), ["proposal", "approval", "todo", "todo"]);
  assert.equal(new Set(body.needs_you.map(({ key }) => key)).size, 4);
  assert.deepEqual(body.agents_working_on, []);
});

test("Done today uses the latest completion transition and local midnight, never last edit", async (t) => {
  const template = todayTasks()[4];
  const rows = [
    { ...template, id: "midnight", execution_summary: "", history: [{ at: new Date(2026, 9, 26).toISOString(), to_status: "done" }] },
    { ...template, id: "yesterday", history: [{ at: new Date(2026, 9, 25, 23, 59, 59).toISOString(), to_status: "done" }] },
    { ...template, id: "future", history: [{ at: new Date(2026, 9, 26, 15).toISOString(), to_status: "done" }] },
    { ...template, id: "completed-again", history: [
      { at: new Date(2026, 9, 25).toISOString(), to_status: "done" },
      { at: new Date(2026, 9, 26, 12).toISOString(), to_status: "done" },
      { at: now.toISOString(), to_status: "review" },
    ] },
    { ...template, id: "unknown", history: [] },
    { ...template, id: "invalid-date", history: [{ at: "not a timestamp", to_status: "done" }] },
  ];
  const get = await endpoint(t, fakeTodayClient(rows).client);
  const body = await (await get()).json();
  assert.deepEqual(body.done_today.map(({ key }) => key), ["completed-again", "midnight"]);
  assert.equal(body.done_today[1].result, null);
  assert.equal(body.completion_dates_unavailable, 2);
});

test("listing limits are exposed rather than claiming the snapshot is complete", async (t) => {
  const fake = fakeTodayClient();
  fake.state.truncated = true;
  const get = await endpoint(t, fake.client);
  assert.equal((await (await get()).json()).truncated, true);
});

test("failed and malformed Orbit reads are errors, never empty successful sections", async (t) => {
  for (const [value, status, code] of [
    [new Error("private diagnostic"), 503, "today_unavailable"],
    [{ error: "tool_error" }, 502, "invalid_today_data"],
    [{ tasks: [null], truncated: false }, 502, "invalid_today_data"],
  ]) {
    const get = await endpoint(t, {
      async pollTool() { if (value instanceof Error) throw value; return value; },
    });
    const response = await get();
    assert.equal(response.status, status);
    assert.deepEqual(await response.json(), { error: code });
  }
});

test("Needs you classifies drafts, script output and grouped proposals, in a stable order", async (t) => {
  const get = await endpoint(t, fakeTodayClient(approvalTasks()).client);
  const body = await (await get()).json();
  assert.deepEqual(body.needs_you.map(({ key, kind, group }) => [key, kind, group]), [
    ["suggestion", "proposal", null],
    ["boot-2", "proposal", { key: "bootstrap:docs:ingest-service", title: "Ingest service" }],
    ["boot-1", "proposal", { key: "bootstrap:docs:ingest-service", title: "Ingest service" }],
    ["boot-3", "proposal", { key: "bootstrap:process", title: "Pages about your processes and policies" }],
    ["onboard-1", "proposal", { key: "onboarding", title: "Set up your Library" }],
    ["onboard-2", "proposal", { key: "onboarding", title: "Set up your Library" }],
    ["draft", "gateway_draft", null],
    ["script", "script_output", null],
    ["review", "approval", null],
    ["personal", "todo", null],
  ]);
});

test("a dismissed or accepted proposal leaves Needs you", async (t) => {
  const rows = approvalTasks();
  rows[0].status = "rejected";
  rows[1].status = "done";
  rows[6].status = "rejected";
  rows[7].status = "rejected";
  const get = await endpoint(t, fakeTodayClient(rows).client);
  const keys = (await (await get()).json()).needs_you.map(({ key }) => key);
  for (const key of ["onboard-1", "onboard-2", "draft", "script"]) assert.ok(!keys.includes(key), key);
});

test("a shared-library sync's updates and conflicts share one card, each marked for its actions", async (t) => {
  const get = await endpoint(t, fakeTodayClient(sharedTasks()).client);
  const body = await (await get()).json();
  const group = { key: "shared-library", title: "Changes from the shared library" };
  assert.deepEqual(body.needs_you.map(({ key, group, shared }) => [key, group, shared]), [
    ["suggestion", null, undefined],
    ["shared-conflict", group, "conflict"],
    ["shared-update", group, "incoming"],
    ["onboard-1", { key: "onboarding", title: "Set up your Library" }, undefined],
  ]);
});

test("a failed Today call logs one redacted line and returns the safe orbit code", async (t) => {
  const lines = [];
  const original = console.error;
  console.error = (line) => lines.push(String(line));
  t.after(() => { console.error = original; });
  const secret = "child-secret-must-not-leak";
  const get = await endpoint(t, {
    async pollTool() {
      throw new OrbitExitError("orbit.task.list", 3, null, `${secret} stdout\n{"error":"private diagnostic ${secret}","code":"workspace_not_found"}\n${secret} stderr`);
    },
  });
  const response = await get();
  console.error = original;
  assert.equal(response.status, 503);
  assert.deepEqual(await response.json(), { error: "today_unavailable", code: "workspace_not_found" });
  assert.equal(lines.length, 1);
  assert.match(lines[0], /route=\/api\/today/);
  assert.match(lines[0], /tool=orbit\.task\.list/);
  assert.match(lines[0], /kind=exit_nonzero/);
  assert.match(lines[0], /code=workspace_not_found/);
  assert.match(lines[0], /exit_code=3/);
  assert.ok(!lines[0].includes(secret), "log line must not contain child stdout/stderr");
  assert.ok(!lines[0].includes("private diagnostic"), "log line must not contain the orbit error message");
});
