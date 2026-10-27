import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";

import { createDispatchPass, createDispatcher, selectDispatchIds } from "../dist/dispatch/pass.js";
import { createOrbitClient } from "../dist/orbit/client.js";
import { installFakeOrbit } from "./fake-orbit.mjs";

const TASKS = [
  { id: "ORB-1", status: "backlog", tags: ["owner:agent"] },
  { id: "ORB-2", status: "backlog", tags: ["office", "owner:agent"] },
  { id: "ORB-3", status: "backlog", tags: ["owner:you"] },
  { id: "ORB-4", status: "proposed", tags: ["owner:agent"] },
  { id: "ORB-5", status: "backlog", tags: [] },
  { id: "ORB-6", status: "backlog", tags: ["owner:agent", "owner:you"] },
  { id: "ORB-7", status: "in-progress", tags: ["owner:agent"] },
];

test("selectDispatchIds submits only owner:agent backlog ids", () => {
  assert.deepEqual(selectDispatchIds(TASKS), ["ORB-1", "ORB-2"]);
  assert.deepEqual(selectDispatchIds([]), []);
});

test("a dispatch pass submits only owner:agent backlog ids", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  await writeFile(join(fake.state, "tasks.json"), JSON.stringify(TASKS));
  const env = fake.env();
  const client = createOrbitClient({ timeoutMs: 10_000, env });
  const dispatcher = createDispatcher({ client, env, timeoutMs: 10_000 });

  const result = await dispatcher.runOnce();
  assert.deepEqual(result, { skipped: false, submitted: ["ORB-1", "ORB-2"] });

  const shipLog = (await readFile(join(fake.state, "ship.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
  assert.deepEqual(shipLog, [["ORB-1", "ORB-2"]]);

  // The listing went through `orbit tool run orbit.task.list --json`.
  const invocations = (await readFile(join(fake.state, "invocations.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
  const listCall = invocations.find((args) => args[0] === "tool");
  assert.deepEqual(listCall.slice(0, 4), ["tool", "run", "orbit.task.list", "--json"]);
  const shipCall = invocations.find((args) => args[0] === "run");
  assert.deepEqual(shipCall, ["run", "ship", "ORB-1", "ORB-2"]);
});

test("a pass is skipped while the previous one is running", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  await writeFile(join(fake.state, "tasks.json"), JSON.stringify(TASKS));
  // Listing takes 400 ms: the first pass is still running when ticked again.
  const env = fake.env({ FAKE_ORBIT_LIST_DELAY_MS: "400" });
  const client = createOrbitClient({ timeoutMs: 10_000, env });
  const dispatcher = createDispatcher({ client, env, timeoutMs: 10_000 });

  const first = dispatcher.runOnce();
  const second = await dispatcher.runOnce();
  assert.deepEqual(second, { skipped: true, submitted: [] });
  const completed = await first;
  assert.deepEqual(completed, { skipped: false, submitted: ["ORB-1", "ORB-2"] });

  const shipLog = (await readFile(join(fake.state, "ship.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "");
  assert.equal(shipLog.length, 1);
});

test("single-flight holds with injected dependencies too", async () => {
  let release;
  const gate = new Promise((resolve) => {
    release = resolve;
  });
  let listCalls = 0;
  const shipped = [];
  const pass = createDispatchPass({
    listTasks: async () => {
      listCalls += 1;
      await gate;
      return TASKS;
    },
    ship: async (ids) => {
      shipped.push([...ids]);
    },
  });
  const first = pass.runOnce();
  const skipped = await pass.runOnce();
  assert.deepEqual(skipped, { skipped: true, submitted: [] });
  release();
  assert.deepEqual(await first, { skipped: false, submitted: ["ORB-1", "ORB-2"] });
  assert.equal(listCalls, 1);
  assert.deepEqual(shipped, [["ORB-1", "ORB-2"]]);
});

test("nothing eligible means no ship invocation", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  await writeFile(
    join(fake.state, "tasks.json"),
    JSON.stringify([{ id: "ORB-9", status: "backlog", tags: ["owner:you"] }]),
  );
  const env = fake.env();
  const client = createOrbitClient({ timeoutMs: 10_000, env });
  const dispatcher = createDispatcher({ client, env, timeoutMs: 10_000 });
  assert.deepEqual(await dispatcher.runOnce(), { skipped: false, submitted: [] });
  const shipLog = await readFile(join(fake.state, "ship.log"), "utf8").catch(() => "");
  assert.equal(shipLog, "");
});
