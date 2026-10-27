import assert from "node:assert/strict";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";

import { createDispatchPass, createDispatcher, selectDispatchIds, shipTasks } from "../dist/dispatch/pass.js";
import { createOrbitClient } from "../dist/orbit/client.js";
import { installFakeOrbit } from "./fake-orbit.mjs";
import { installSupervisionDoubles, timerAt } from "./supervision-doubles.mjs";

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

const SHIP_SUPERVISION = {
  timeoutMs: 100,
  command: "orbit-supervision-double",
  env: { PATH: "" },
};

test("ship execution does not signal a process group after its leader exits", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const pending = shipTasks(["ORB-1"], SHIP_SUPERVISION).then(
      () => {
        throw new Error("expected the timed-out ship to reject");
      },
      (error) => error,
    );
    assert.equal(doubles.children.length, 1);
    assert.equal(doubles.children[0].options.detached, process.platform !== "win32");
    assert.deepEqual(doubles.children[0].args, ["run", "ship", "ORB-1"]);
    timerAt(doubles.timers, 100).fn();
    const groupPid = process.platform === "win32" ? doubles.children[0].pid : -doubles.children[0].pid;
    assert.deepEqual(
      doubles.signals.map((entry) => entry.signal),
      ["SIGTERM"],
    );
    assert.equal(doubles.signals[0].pid, groupPid);
    assert.equal(doubles.signals[0].closed, false);
    doubles.children[0].finish(0, null);
    const error = await pending;
    assert.match(error.message, /orbit run ship timed out after 100 ms/);

    const escalation = timerAt(doubles.timers, 250);
    const before = doubles.signals.length;
    escalation.fn();
    assert.deepEqual(
      doubles.signals.slice(before),
      [],
      "escalation must not signal a reaped process group",
    );
    assert.equal(escalation.cleared, true);
  } finally {
    doubles.restore();
  }
});

test("a live ship child that ignores SIGTERM is killed and the call times out", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const pending = shipTasks(["ORB-1"], SHIP_SUPERVISION).then(
      () => {
        throw new Error("expected the timed-out ship to reject");
      },
      (error) => error,
    );
    timerAt(doubles.timers, 100).fn();
    const escalation = timerAt(doubles.timers, 250);
    assert.equal(escalation.cleared, false);
    escalation.fn();
    assert.equal(doubles.signals.length, 2);
    assert.equal(doubles.signals[1].signal, "SIGKILL");
    assert.equal(doubles.signals[1].closed, false);
    assert.equal(doubles.signals[1].exitCode, null);
    const groupPid = process.platform === "win32" ? doubles.children[0].pid : -doubles.children[0].pid;
    assert.equal(doubles.signals[0].pid, groupPid);
    assert.equal(doubles.signals[1].pid, groupPid);
    doubles.children[0].finish(null, "SIGKILL");
    const error = await pending;
    assert.match(error.message, /orbit run ship timed out after 100 ms/);
  } finally {
    doubles.restore();
  }
});
