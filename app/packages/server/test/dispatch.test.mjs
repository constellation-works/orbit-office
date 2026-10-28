import assert from "node:assert/strict";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  createDispatchPass,
  createDispatcher,
  listBacklogAgentTasks,
  MAX_SHIP_STDERR_BYTES,
  selectDispatchIds,
  shipTasks,
} from "../dist/dispatch/pass.js";
import { createOrbitClient, OrbitError } from "../dist/orbit/client.js";
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

const SCHEDULED_AT = "2026-11-06T09:00:00Z";
const BEFORE = new Date("2026-11-06T08:59:30Z");
const AFTER = new Date("2026-11-06T09:00:30Z");
const SCHEDULED = [
  { id: "ORB-S1", status: "backlog", tags: ["owner:agent"], not_before: SCHEDULED_AT },
  { id: "ORB-S2", status: "backlog", tags: ["owner:agent"], not_before: "2026-11-05T09:00:00Z" },
  { id: "ORB-S3", status: "backlog", tags: ["owner:agent"], not_before: null },
  { id: "ORB-S4", status: "backlog", tags: ["owner:you"], not_before: "2026-11-05T09:00:00Z" },
];

test("selectDispatchIds holds a task until its not_before, then lets it go", () => {
  assert.deepEqual(selectDispatchIds(SCHEDULED, BEFORE), ["ORB-S2", "ORB-S3"]);
  assert.deepEqual(selectDispatchIds(SCHEDULED, new Date(SCHEDULED_AT)), ["ORB-S1", "ORB-S2", "ORB-S3"], "at the time is not before it");
  assert.deepEqual(selectDispatchIds(SCHEDULED, AFTER), ["ORB-S1", "ORB-S2", "ORB-S3"]);
});

test("a scheduled agent task waits without a refusal, then ships on the first pass after its time", async (t) => {
  const fake = await installFakeOrbit();
  t.after(() => rm(fake.root, { recursive: true, force: true }));
  await writeFile(join(fake.state, "tasks.json"), JSON.stringify(SCHEDULED));
  // The fake's ship refuses a held task as Orbit does; the pass must never reach that refusal.
  const env = fake.env({ FAKE_ORBIT_NOW: BEFORE.toISOString() });
  await assert.rejects(shipTasks(["ORB-S1"], { env, timeoutMs: 10_000 }), /task_scheduled/);
  let clock = BEFORE;
  const errors = [];
  const client = createOrbitClient({ timeoutMs: 10_000, env });
  const dispatcher = createDispatcher({ client, env, timeoutMs: 10_000, now: () => clock, onError: (error) => errors.push(error) });

  assert.deepEqual(await dispatcher.runOnce(), { skipped: false, submitted: ["ORB-S2", "ORB-S3"] });
  assert.deepEqual(await dispatcher.runOnce(), { skipped: false, submitted: ["ORB-S2", "ORB-S3"] }, "every pass before the time leaves it out");
  clock = AFTER;
  const afterEnv = fake.env({ FAKE_ORBIT_NOW: AFTER.toISOString() });
  const later = createDispatcher({ client, env: afterEnv, timeoutMs: 10_000, now: () => clock, onError: (error) => errors.push(error) });
  assert.deepEqual(await later.runOnce(), { skipped: false, submitted: ["ORB-S1", "ORB-S2", "ORB-S3"] });
  assert.deepEqual(errors, []);

  const shipLog = (await readFile(join(fake.state, "ship.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
  assert.deepEqual(shipLog, [["ORB-S2", "ORB-S3"], ["ORB-S2", "ORB-S3"], ["ORB-S1", "ORB-S2", "ORB-S3"]]);
  const listCall = (await readFile(join(fake.state, "invocations.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line))
    .find((args) => args[2] === "orbit.task.list");
  assert.deepEqual(JSON.parse(listCall[listCall.indexOf("--input") + 1]).fields, ["id", "status", "tags", "not_before"]);
});

test("a not_before the pass cannot read fails the listing and ships nothing", async () => {
  const error = await rejectionOf(
    listBacklogAgentTasks(listingClient({ tasks: [{ id: "ORB-1", status: "backlog", tags: ["owner:agent"], not_before: "Friday at nine" }] })),
  );
  assert.ok(error instanceof OrbitError);
  assert.equal(error.code, "invalid_output");
});

/** A client whose `orbit.task.list` answers with `output`, as the Orbit CLI would print it. */
function listingClient(output) {
  return { callTool: async () => output };
}

/** The error a call rejects with; fails the test when the call resolves. */
function rejectionOf(promise) {
  return promise.then(
    () => {
      throw new Error("expected the call to reject");
    },
    (error) => error,
  );
}

/** Install a fake orbit whose `orbit.task.list` answers with `listing`. */
async function fakeListing(t, listing) {
  const fake = await installFakeOrbit();
  t.after(async () => {
    await rm(fake.root, { recursive: true, force: true });
  });
  await writeFile(join(fake.state, "tasks.json"), JSON.stringify(listing));
  return fake;
}

/** The ids `orbit run ship` was called with, or "" when it never ran. */
function shipLog(fake) {
  return readFile(join(fake.state, "ship.log"), "utf8").catch(() => "");
}

test("a listing without a tasks array is an invalid_output failure, not an empty pass", async () => {
  for (const output of [{ unexpected: true }, { tasks: { id: "ORB-1" } }, null]) {
    const error = await rejectionOf(listBacklogAgentTasks(listingClient(output)));
    assert.ok(error instanceof OrbitError, `expected OrbitError for ${JSON.stringify(output)}`);
    assert.equal(error.code, "invalid_output", `code for ${JSON.stringify(output)}`);
  }
});

test("malformed task rows are an invalid_output failure, never silently dropped", async () => {
  const malformedRows = [
    [{ id: "ORB-1" }],
    [{ status: "backlog" }],
    [1],
    [null],
    [{ id: 7, status: "backlog" }],
    [{ id: "ORB-1", status: "backlog", tags: "owner:agent" }],
    [{ id: "ORB-1", status: "backlog", tags: ["owner:agent"] }, { status: "backlog" }],
  ];
  for (const rows of malformedRows) {
    for (const output of [rows, { tasks: rows }]) {
      const error = await rejectionOf(listBacklogAgentTasks(listingClient(output)));
      assert.ok(error instanceof OrbitError, `expected OrbitError for ${JSON.stringify(output)}`);
      assert.equal(error.code, "invalid_output", `code for ${JSON.stringify(output)}`);
    }
  }
});

test("valid listings still parse: empty ones are no work, and tags may be absent", async () => {
  assert.deepEqual(await listBacklogAgentTasks(listingClient([])), []);
  assert.deepEqual(await listBacklogAgentTasks(listingClient({ tasks: [] })), []);
  assert.deepEqual(
    await listBacklogAgentTasks(
      listingClient({
        tasks: [
          { id: "ORB-1", status: "backlog", tags: ["owner:agent"] },
          { id: "ORB-2", status: "backlog" },
        ],
      }),
    ),
    [
      { id: "ORB-1", status: "backlog", tags: ["owner:agent"] },
      { id: "ORB-2", status: "backlog" },
    ],
  );
});

test("a scheduled pass reports a malformed listing through onError and ships nothing", { timeout: 10_000 }, async (t) => {
  const fake = await fakeListing(t, { unexpected: true });
  const env = fake.env();
  const client = createOrbitClient({ timeoutMs: 10_000, env });
  const reported = new Promise((resolve) => {
    const dispatcher = createDispatcher({ client, env, timeoutMs: 10_000, intervalMs: 20, onError: resolve });
    t.after(() => dispatcher.stop());
    dispatcher.start();
  });
  // The dispatcher's interval is unref'd; keep the test process alive until it fires.
  const keepAlive = setInterval(() => {}, 20);
  try {
    const error = await reported;
    assert.ok(error instanceof OrbitError);
    assert.equal(error.code, "invalid_output");
    assert.equal(await shipLog(fake), "");
  } finally {
    clearInterval(keepAlive);
  }
});

test("a manual pass rejects a listing without a tasks field and ships nothing", async (t) => {
  const fake = await fakeListing(t, []);
  const env = fake.env();
  const dispatcher = createDispatcher({ client: listingClient({ unexpected: true }), env, timeoutMs: 10_000 });
  const error = await rejectionOf(dispatcher.runOnce());
  assert.ok(error instanceof OrbitError);
  assert.equal(error.code, "invalid_output");
  assert.equal(await shipLog(fake), "");
  assert.equal(dispatcher.running, false, "a failed pass releases the single-flight guard");
});

test("a manual pass rejects malformed rows from orbit and ships nothing", async (t) => {
  const fake = await fakeListing(t, [{ id: "ORB-1" }]);
  const env = fake.env();
  const dispatcher = createDispatcher({ client: createOrbitClient({ timeoutMs: 10_000, env }), env, timeoutMs: 10_000 });
  const error = await rejectionOf(dispatcher.runOnce());
  assert.ok(error instanceof OrbitError);
  assert.equal(error.code, "invalid_output");
  assert.equal(await shipLog(fake), "");
});

test("a valid empty listing is a successful empty pass with no ship call", async (t) => {
  const fake = await fakeListing(t, []);
  const env = fake.env();
  const dispatcher = createDispatcher({ client: createOrbitClient({ timeoutMs: 10_000, env }), env, timeoutMs: 10_000 });
  assert.deepEqual(await dispatcher.runOnce(), { skipped: false, submitted: [] });
  assert.equal(await shipLog(fake), "");
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

test("ship settles on its deadline when exit precedes close", async () => {
  const doubles = installSupervisionDoubles();
  try {
    let settled = false;
    const pending = shipTasks(["ORB-1"], SHIP_SUPERVISION).then(
      () => {
        settled = true;
        throw new Error("expected the pipe-held ship to reject");
      },
      (error) => {
        settled = true;
        return error;
      },
    );
    doubles.children[0].exitOnly(0, null);
    assert.equal(timerAt(doubles.timers, 100).cleared, false, "exit must not clear the deadline");
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.equal(settled, false, "ship must wait for close or the deadline, not settle on exit");
    timerAt(doubles.timers, 100).fn();
    const error = await pending;
    assert.equal(settled, true);
    assert.match(error.message, /orbit run ship timed out after 100 ms/);
    assert.deepEqual(doubles.signals, [], "no signal may be sent after the leader is reaped");
    doubles.children[0].closeOnly(0, null);
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.deepEqual(doubles.signals, [], "a late close must not settle twice");
  } finally {
    doubles.restore();
  }
});

test("ship settles after exit when its deadline already fired", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const pending = shipTasks(["ORB-1"], SHIP_SUPERVISION).then(
      () => {
        throw new Error("expected the pipe-held ship to reject");
      },
      (error) => error,
    );
    timerAt(doubles.timers, 100).fn();
    assert.deepEqual(doubles.signals.map((e) => e.signal), ["SIGTERM"]);
    doubles.children[0].exitOnly(null, "SIGTERM");
    const escalation = timerAt(doubles.timers, 250);
    assert.equal(escalation.cleared, false, "exit must not clear the escalation bound");
    escalation.fn();
    const error = await pending;
    assert.match(error.message, /orbit run ship timed out after 100 ms/);
    assert.deepEqual(doubles.signals.map((e) => e.signal), ["SIGTERM"], "no signal after reap");
    doubles.children[0].closeOnly(null, "SIGTERM");
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.deepEqual(doubles.signals.map((e) => e.signal), ["SIGTERM"], "a late close must not settle twice");
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

/**
 * Make a tiny executable that writes small stderr chunks (2 KiB each) past
 * the capture ceiling, then exits. Total output stays in the tens of KiB,
 * so the test proves the ceiling without stress or large allocation.
 */
async function installStderrEmitter(t, exitCode) {
  const root = await mkdtemp(join(tmpdir(), "ship-stderr-"));
  t.after(async () => {
    await rm(root, { recursive: true, force: true });
  });
  const executable = join(root, exitCode === 0 ? "ship-ok" : "ship-noisy-fail");
  const chunkBytes = 2048;
  // Enough 2 KiB writes to exceed the 8 KiB ceiling with small allocations.
  const chunks = Math.ceil(MAX_SHIP_STDERR_BYTES / chunkBytes) + 4;
  await writeFile(
    executable,
    `#!/usr/bin/env node\n` +
      `const chunk = "e".repeat(${chunkBytes - 32}) + "\\n";\n` +
      `for (let i = 0; i < ${chunks}; i++) process.stderr.write("chunk-" + i + ":" + chunk);\n` +
      `process.exitCode = ${exitCode};\n`,
  );
  await import("node:fs/promises").then((fs) => fs.chmod(executable, 0o755));
  return executable;
}

test("ship failure with stderr above the capture ceiling is bounded and marked", async (t) => {
  const command = await installStderrEmitter(t, 3);
  const error = await shipTasks(["ORB-1"], { command, timeoutMs: 10_000 }).then(
    () => {
      throw new Error("expected the noisy ship to reject");
    },
    (failure) => failure,
  );
  assert.match(error.message, /orbit run ship failed \(code 3, signal none\):/);
  assert.match(error.message, /\.\.\. \[truncated\]/);
  assert.ok(error.message.length < 1000, `bounded message, got ${error.message.length}`);
  assert.match(error.message, /chunk-0:/, "retains stderr from the first chunk");
});

test("ship success drains large stderr without failing", async (t) => {
  const command = await installStderrEmitter(t, 0);
  await shipTasks(["ORB-1"], { command, timeoutMs: 10_000 });
});
