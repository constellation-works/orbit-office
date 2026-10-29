import assert from "node:assert/strict";
import { createServer } from "node:http";
import { test } from "node:test";

import { createAgentsFeature } from "../dist/features/agents/index.js";
import { agentsWorkspaces, fakeAgentsClient, now } from "./agents-fixture.mjs";

async function endpoint(t, client) {
  const feature = createAgentsFeature({ client, now: () => now });
  assert.equal(feature.name, "agents");
  assert.equal(feature.status, "built");
  const server = createServer((request, response) => {
    const route = feature.routes.find((route) => route.method === request.method && route.path === request.url);
    if (route) void route.handle(request, response);
    else { response.writeHead(404); response.end(); }
  });
  t.after(() => new Promise((resolve) => server.close(resolve)));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  return () => fetch(`http://127.0.0.1:${server.address().port}/api/agents`);
}

test("tasks are grouped across the office workspace and two followed workspaces, newest first", async (t) => {
  const fake = fakeAgentsClient();
  const get = await endpoint(t, fake.client);
  const response = await get();
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("cache-control"), "no-store");
  const body = await response.json();
  assert.deepEqual(body.workspaces, ["office", "ws_alpha", "ws_beta"]);
  assert.deepEqual(body.unavailable, []);
  assert.deepEqual(body.working_now.map(({ id }) => id), ["ORB-1", "ORB-10", "ORB-11"]);
  assert.deepEqual(body.in_review.map(({ id }) => id), ["ORB-2", "ORB-20"]);
  assert.deepEqual(body.queued.map(({ id }) => id), ["ORB-3", "ORB-12"]);
  assert.deepEqual(body.done_today.map(({ id }) => id), ["ORB-4", "ORB-21"]);
  assert.deepEqual(body.working_now[0], {
    id: "ORB-1", title: "Office working task", workspace: "office",
    type: "feature", priority: "medium", crew: "muse-wonder",
    updated_at: body.working_now[0].updated_at,
  });
  assert.equal(body.working_now[1].workspace, "ws_alpha");
  assert.equal(body.in_review[0].priority, "high");
  // Only office.status and orbit.task.list are called; the followed list
  // comes from the status fixture, and extra workspaces pass a selector.
  assert.equal(fake.state.calls[0].name, "office.status");
  assert.ok(fake.state.calls.slice(1).every(({ name }) => name === "orbit.task.list"));
  const selectors = fake.state.calls.slice(1).map(({ input }) => input.workspace ?? null);
  assert.ok(selectors.includes(null) && selectors.includes("ws_alpha") && selectors.includes("ws_beta"));
  assert.ok(fake.state.calls.slice(1).every(({ input }) => input.fields.includes("updated_at")));
});

test("done today is only completions since local midnight; proposed tasks are never returned", async (t) => {
  const get = await endpoint(t, fakeAgentsClient().client);
  const body = await (await get()).json();
  const ids = [...body.working_now, ...body.in_review, ...body.queued, ...body.done_today].map(({ id }) => id);
  assert.ok(!ids.includes("ORB-5"), "proposed task is not listed");
  assert.ok(!ids.includes("ORB-6"), "done yesterday is not done today");
  assert.deepEqual(body.done_today.map(({ id }) => id), ["ORB-4", "ORB-21"]);
});

test("a failed followed workspace is named and the other workspaces still answer", async (t) => {
  const fake = fakeAgentsClient(agentsWorkspaces(), ["ws_alpha"]);
  const get = await endpoint(t, fake.client);
  const response = await get();
  assert.equal(response.status, 200);
  const body = await response.json();
  assert.deepEqual(body.unavailable, ["ws_alpha"]);
  assert.deepEqual(body.working_now.map(({ id }) => id), ["ORB-1"]);
  assert.deepEqual(body.in_review.map(({ id }) => id), ["ORB-2", "ORB-20"]);
  assert.deepEqual(body.done_today.map(({ id }) => id), ["ORB-4", "ORB-21"]);
});

test("no tasks anywhere carries empty groups", async (t) => {
  const get = await endpoint(t, fakeAgentsClient({ office: [], ws_alpha: [], ws_beta: [] }).client);
  const body = await (await get()).json();
  assert.deepEqual(body.working_now, []);
  assert.deepEqual(body.in_review, []);
  assert.deepEqual(body.queued, []);
  assert.deepEqual(body.done_today, []);
  assert.deepEqual(body.unavailable, []);
  assert.deepEqual(body.workspaces, ["office", "ws_alpha", "ws_beta"]);
});

test("an uninitialized Library follows no extra workspaces", async (t) => {
  const fake = fakeAgentsClient();
  fake.client.callTool = async (name, input) => {
    fake.state.calls.push({ name, input });
    return { ok: true, output: { brief: null } };
  };
  const get = await endpoint(t, fake.client);
  const body = await (await get()).json();
  assert.deepEqual(body.workspaces, ["office"]);
  assert.deepEqual(body.working_now.map(({ id }) => id), ["ORB-1"]);
});
