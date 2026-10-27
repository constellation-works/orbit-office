import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";

import {
  createOrbitClient,
  OrbitError,
  OrbitExitError,
  OrbitNotPermittedError,
  OrbitQueueFullError,
  OrbitQueueTimeoutError,
  OrbitTimeoutError,
  OrbitUnknownAutoTaskError,
} from "../dist/orbit/client.js";
import { installFakeOrbit } from "./fake-orbit.mjs";
import { installSupervisionDoubles, timerAt } from "./supervision-doubles.mjs";

async function readLines(path) {
  return (await readFile(path, "utf8").catch(() => ""))
    .split("\n")
    .filter((line) => line !== "");
}

/** Replay S/E events: the highest number of subprocesses alive at once. */
async function maxConcurrency(state) {
  const events = await readLines(join(state, "events.log"));
  let current = 0;
  let max = 0;
  for (const event of events) {
    current += event === "S" ? 1 : -1;
    if (current > max) max = current;
  }
  return max;
}

test("concurrency never exceeds the configured cap", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    maxConcurrent: 3,
    timeoutMs: 10_000,
    env: fake.env(),
  });
  const calls = Array.from({ length: 9 }, (_, index) =>
    client.callTool("orbit.task.show", { id: `T-${index}`, delayMs: 90 }),
  );
  const results = await Promise.all(calls);
  assert.equal(results.length, 9);
  for (const result of results) assert.equal(result.ok, true);
  const invocations = await readLines(join(fake.state, "invocations.log"));
  assert.equal(invocations.length, 9);
  const max = await maxConcurrency(fake.state);
  assert.ok(max <= 3, `concurrency peaked at ${max}, cap is 3`);
  assert.ok(max >= 2, `expected real parallelism, peaked at ${max}`);
});

test("the default cap is four", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  // No maxConcurrent passed: the documented default applies.
  const client = createOrbitClient({ timeoutMs: 10_000, env: fake.env() });
  await Promise.all(
    Array.from({ length: 8 }, (_, index) =>
      client.callTool("orbit.task.show", { id: `D-${index}`, delayMs: 90 }),
    ),
  );
  const max = await maxConcurrency(fake.state);
  assert.ok(max <= 4, `concurrency peaked at ${max}, default cap is 4`);
});

test("a hung call times out with a typed error", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({ timeoutMs: 150, env: fake.env() });
  const started = Date.now();
  await assert.rejects(client.callTool("orbit.task.show", { id: "H-1", hang: true }), (error) => {
    assert.ok(error instanceof OrbitTimeoutError, `expected OrbitTimeoutError, got ${error}`);
    assert.equal(error.code, "timeout");
    assert.equal(error.timeoutMs, 150);
    return true;
  });
  assert.ok(Date.now() - started < 5_000, "timeout must end the call promptly");
});

test("a non-zero exit is a typed error carrying the exit code", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({ timeoutMs: 5_000, env: fake.env() });
  await assert.rejects(client.callTool("orbit.task.show", { id: "F-1", fail: true }), (error) => {
    assert.ok(error instanceof OrbitExitError, `expected OrbitExitError, got ${error}`);
    assert.equal(error.code, "exit_nonzero");
    assert.equal(error.exitCode, 3);
    return true;
  });
});

test("a missing executable is a typed spawn error", async () => {
  const client = createOrbitClient({
    timeoutMs: 5_000,
    env: { ...process.env, PATH: "/nonexistent-orbit-test-dir" },
  });
  await assert.rejects(client.callTool("orbit.task.show", { id: "X-1" }), (error) => {
    assert.equal(error.code, "spawn_failed");
    return true;
  });
});

test("two concurrent identical polls spawn one subprocess", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 10_000,
    pollCacheTtlMs: 0,
    env: fake.env(),
  });
  const input = { status: "backlog", delayMs: 80 };
  const [first, second] = await Promise.all([
    client.pollTool("orbit.task.list", input),
    // Same content, different key order: still the same poll.
    client.pollTool("orbit.task.list", { delayMs: 80, status: "backlog" }),
  ]);
  assert.deepEqual(first, second);
  const invocations = await readLines(join(fake.state, "invocations.log"));
  assert.equal(invocations.length, 1);

  // A different poll is a different call.
  await client.pollTool("orbit.task.list", { status: "review", delayMs: 10 });
  const after = await readLines(join(fake.state, "invocations.log"));
  assert.equal(after.length, 2);
});

test("a fresh poll result is reused inside the cache window", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 10_000,
    pollCacheTtlMs: 60_000,
    env: fake.env(),
  });
  await client.pollTool("orbit.task.list", { status: "backlog" });
  await client.pollTool("orbit.task.list", { status: "backlog" });
  const invocations = await readLines(join(fake.state, "invocations.log"));
  assert.equal(invocations.length, 1);
});

/** Parse the `--input` JSON of every logged fake-orbit invocation. */
async function invocationInputs(state) {
  const lines = await readLines(join(state, "invocations.log"));
  return lines.map((line) => {
    const args = JSON.parse(line);
    const index = args.indexOf("--input");
    return { tool: args[2], input: JSON.parse(args[index + 1]) };
  });
}

test("a bound client sends the workspace selector for orbit.routine.control", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 5_000,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/installed/workspace" }),
  });
  const result = await client.callTool("orbit.routine.control", { action: "list" });
  assert.equal(result.input.workspace, "/installed/workspace");
  const invocations = await invocationInputs(fake.state);
  assert.equal(invocations.length, 1);
  assert.equal(invocations[0].tool, "orbit.routine.control");
  assert.deepEqual(invocations[0].input, { action: "list", workspace: "/installed/workspace" });
});

test("an explicit workspace option overrides the environment binding", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 5_000,
    workspace: "/explicit/workspace",
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/env/workspace" }),
  });
  await client.callTool("orbit.routine.control", { action: "list" });
  const invocations = await invocationInputs(fake.state);
  assert.equal(invocations[0].input.workspace, "/explicit/workspace");
});

test("workspace-less and non-requiring calls send input unchanged", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const unboundEnv = fake.env();
  delete unboundEnv.ORBIT_OFFICE_WORKSPACE;
  const unbound = createOrbitClient({ timeoutMs: 5_000, env: unboundEnv });
  await unbound.callTool("orbit.routine.control", { action: "list" });
  const bound = createOrbitClient({
    timeoutMs: 5_000,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/installed/workspace" }),
  });
  await bound.callTool("orbit.task.list", { status: "backlog" });
  const invocations = await invocationInputs(fake.state);
  assert.deepEqual(invocations[0].input, { action: "list" });
  assert.deepEqual(invocations[1].input, { status: "backlog" });
});

test("a caller-supplied workspace is not overwritten, including via pollTool", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 5_000,
    pollCacheTtlMs: 0,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/installed/workspace" }),
  });
  await client.callTool("orbit.routine.control", { action: "list", workspace: "/caller/workspace" });
  await client.pollTool("orbit.routine.control", { action: "list" });
  const invocations = await invocationInputs(fake.state);
  assert.equal(invocations[0].input.workspace, "/caller/workspace");
  assert.equal(invocations[1].input.workspace, "/installed/workspace");
});

test("mintAutoTask runs orbit.auto_task.mint with the workspace binding", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 5_000,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/installed/workspace" }),
  });
  const result = await client.mintAutoTask("office-review");
  assert.equal(result.minted, true);
  assert.equal(result.name, "office-review");
  assert.equal(result.task.id, "ORB-FAKE-MINT-1");
  const invocations = await invocationInputs(fake.state);
  assert.equal(invocations.length, 1);
  assert.equal(invocations[0].tool, "orbit.auto_task.mint");
  assert.deepEqual(invocations[0].input, {
    name: "office-review",
    acknowledge_unconditional: true,
    workspace: "/installed/workspace",
  });
});

test("mintAutoTask maps an unknown auto-task to a typed error", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 5_000,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/installed/workspace" }),
  });
  await assert.rejects(client.mintAutoTask("no-such-auto-task"), (error) => {
    assert.ok(error instanceof OrbitUnknownAutoTaskError, `expected OrbitUnknownAutoTaskError, got ${error}`);
    assert.equal(error.code, "unknown_auto_task");
    assert.equal(error.autoTaskName, "no-such-auto-task");
    assert.match(error.message, /no auto-task named "no-such-auto-task"/);
    return true;
  });
});

test("mintAutoTask maps capability_denied to not_permitted", async (t) => {
  const fake = await installFakeOrbit();
  t.after(async () => {
    const { rm } = await import("node:fs/promises");
    await rm(fake.root, { recursive: true, force: true });
  });
  const client = createOrbitClient({
    timeoutMs: 5_000,
    env: fake.env({ ORBIT_OFFICE_WORKSPACE: "/installed/workspace" }),
  });
  await assert.rejects(client.mintAutoTask("denied-auto-task"), (error) => {
    assert.ok(error instanceof OrbitNotPermittedError, `expected OrbitNotPermittedError, got ${error}`);
    assert.ok(error instanceof OrbitError);
    assert.equal(error.code, "not_permitted");
    assert.equal(error.autoTaskName, "denied-auto-task");
    assert.match(error.message, /not permitted to mint auto-task "denied-auto-task"/);
    return true;
  });
  const invocations = await invocationInputs(fake.state);
  assert.equal(invocations[0].tool, "orbit.auto_task.mint");
  assert.equal(invocations[0].input.workspace, "/installed/workspace");
});

const SUPERVISION_CLIENT = {
  timeoutMs: 100,
  command: "orbit-supervision-double",
  env: { PATH: "" },
};

test("tool execution does not signal a process group after its leader exits", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const pending = createOrbitClient(SUPERVISION_CLIENT)
      .callTool("orbit.task.show", { id: "reaped" })
      .then(
        () => {
          throw new Error("expected the timed-out call to reject");
        },
        (error) => error,
      );
    assert.equal(doubles.children.length, 1);
    assert.equal(doubles.children[0].options.detached, process.platform !== "win32");
    timerAt(doubles.timers, 100).fn();
    const groupPid = process.platform === "win32" ? doubles.children[0].pid : -doubles.children[0].pid;
    assert.deepEqual(
      doubles.signals.map((entry) => entry.signal),
      ["SIGTERM"],
    );
    assert.equal(doubles.signals[0].pid, groupPid);
    assert.equal(doubles.signals[0].closed, false);
    assert.equal(doubles.signals[0].exitCode, null);
    doubles.children[0].finish(0, null);
    const error = await pending;
    assert.ok(error instanceof OrbitTimeoutError, `expected OrbitTimeoutError, got ${error}`);
    assert.equal(error.timeoutMs, 100);

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

test("tool execution settles on its deadline when exit precedes close", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = createOrbitClient(SUPERVISION_CLIENT);
    let settled = false;
    const pending = client.callTool("orbit.task.show", { id: "pipe-held" }).then(
      () => {
        settled = true;
        throw new Error("expected the pipe-held call to reject");
      },
      (error) => {
        settled = true;
        return error;
      },
    );
    // Leader exits 0 but a descendant holds stdout/stderr: no close yet.
    doubles.children[0].exitOnly(0, null);
    assert.equal(timerAt(doubles.timers, 100).cleared, false, "exit must not clear the deadline");
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.equal(settled, false, "call must wait for close or the deadline, not settle on exit");
    timerAt(doubles.timers, 100).fn();
    const error = await pending;
    assert.equal(settled, true);
    assert.ok(error instanceof OrbitTimeoutError, `expected OrbitTimeoutError, got ${error}`);
    assert.deepEqual(doubles.signals, [], "no signal may be sent after the leader is reaped");
    assert.equal(client.activeCount, 0);
    doubles.children[0].closeOnly(0, null);
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.deepEqual(doubles.signals, []);
    assert.equal(client.activeCount, 0, "a late close must not settle twice");
  } finally {
    doubles.restore();
  }
});

test("tool execution settles after exit when its deadline already fired", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = createOrbitClient(SUPERVISION_CLIENT);
    let settled = false;
    const pending = client.callTool("orbit.task.show", { id: "pipe-held-late" }).then(
      () => {
        settled = true;
        throw new Error("expected the pipe-held call to reject");
      },
      (error) => {
        settled = true;
        return error;
      },
    );
    timerAt(doubles.timers, 100).fn();
    assert.deepEqual(doubles.signals.map((e) => e.signal), ["SIGTERM"]);
    doubles.children[0].exitOnly(null, "SIGTERM");
    const escalation = timerAt(doubles.timers, 250);
    assert.equal(escalation.cleared, false, "exit must not clear the escalation bound");
    escalation.fn();
    const error = await pending;
    assert.equal(settled, true);
    assert.ok(error instanceof OrbitTimeoutError, `expected OrbitTimeoutError, got ${error}`);
    assert.deepEqual(doubles.signals.map((e) => e.signal), ["SIGTERM"], "no signal after reap");
    doubles.children[0].closeOnly(null, "SIGTERM");
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.equal(client.activeCount, 0, "a late close must not settle twice");
  } finally {
    doubles.restore();
  }
});

test("oversized output holds its slot until the child closes", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = createOrbitClient({
      maxConcurrent: 1,
      timeoutMs: 10_000,
      maxOutputBytes: 16,
      command: "orbit-supervision-double",
      env: { PATH: "" },
    });
    let firstSettled = false;
    let firstError;
    const first = client.callTool("orbit.task.show", { id: "oversized" }).then(
      () => {
        throw new Error("expected the oversized call to reject");
      },
      (error) => {
        firstSettled = true;
        firstError = error;
      },
    );
    const second = client.callTool("orbit.task.show", { id: "queued" });
    assert.equal(doubles.children.length, 1);
    assert.equal(client.activeCount, 1);
    assert.equal(client.queuedCount, 1);

    doubles.children[0].stdout.emit("data", Buffer.from("12345678901234567"));
    assert.deepEqual(
      doubles.signals.map((entry) => entry.signal),
      ["SIGKILL"],
    );
    for (let i = 0; i < 5; i += 1) await Promise.resolve();
    assert.equal(firstSettled, false, "oversized call must not settle before close");
    assert.equal(doubles.children.length, 1, "queued call must not spawn before close");
    assert.equal(client.activeCount, 1);
    assert.equal(client.queuedCount, 1);

    doubles.children[0].finish(null, "SIGKILL");
    await first;
    assert.ok(firstError instanceof OrbitError, `expected OrbitError, got ${firstError}`);
    assert.equal(firstError.code, "output_too_large");
    assert.equal(doubles.children.length, 2, "queued call spawns after close");
    assert.equal(client.activeCount, 1);
    assert.equal(client.queuedCount, 0);

    doubles.children[1].stdout.emit("data", Buffer.from('{"ok":true}\n'));
    doubles.children[1].finish(0, null);
    assert.deepEqual(await second, { ok: true });
    assert.equal(client.activeCount, 0);
  } finally {
    doubles.restore();
  }
});

test("a live tool child that ignores SIGTERM is killed and the call times out", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const pending = createOrbitClient(SUPERVISION_CLIENT)
      .callTool("orbit.task.show", { id: "live" })
      .then(
        () => {
          throw new Error("expected the timed-out call to reject");
        },
        (error) => error,
      );
    timerAt(doubles.timers, 100).fn();
    const escalation = timerAt(doubles.timers, 250);
    assert.equal(escalation.cleared, false);
    assert.equal(doubles.children[0].exitCode, null);
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
    assert.ok(error instanceof OrbitTimeoutError, `expected OrbitTimeoutError, got ${error}`);
    assert.equal(error.code, "timeout");
    assert.equal(error.timeoutMs, 100);
  } finally {
    doubles.restore();
  }
});

/** Client whose subprocesses are the supervision doubles. */
function queuedClient(options = {}) {
  return createOrbitClient({
    ...SUPERVISION_CLIENT,
    maxConcurrent: 1,
    maxQueued: 4,
    timeoutMs: 1_000,
    queueWaitMs: 5_000,
    ...options,
  });
}

function childInput(child) {
  return JSON.parse(child.args[5]);
}

/** Settle a rejection without leaving it unhandled when the assertion fails first. */
function capturedRejection(promise) {
  return promise.then(
    () => {
      throw new Error("expected the call to reject");
    },
    (error) => error,
  );
}

test("a full queue refuses the next call and admitted calls stay FIFO", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = queuedClient({ maxQueued: 2 });
    const first = client.callTool("orbit.task.show", { id: "first" });
    const second = client.callTool("orbit.task.show", { id: "second" });
    const third = client.callTool("orbit.task.show", { id: "third" });
    assert.equal(client.activeCount, 1);
    assert.equal(client.queuedCount, 2);
    assert.equal(doubles.children.length, 1);

    const refused = capturedRejection(client.callTool("orbit.task.show", { id: "refused" }));
    assert.equal(client.queuedCount, 2);
    assert.equal(doubles.children.length, 1);
    const error = await refused;
    assert.ok(error instanceof OrbitQueueFullError, `expected OrbitQueueFullError, got ${error}`);
    assert.equal(error.code, "queue_full");
    assert.equal(error.maxQueued, 2);
    assert.equal(error.toolName, "orbit.task.show");

    doubles.children[0].finish(0, null);
    await first;
    assert.equal(childInput(doubles.children[1]).id, "second");
    assert.equal(client.queuedCount, 1);
    doubles.children[1].finish(0, null);
    await second;
    assert.equal(childInput(doubles.children[2]).id, "third");
    assert.equal(client.queuedCount, 0);
    doubles.children[2].finish(0, null);
    await third;
    assert.equal(client.activeCount, 0);
    assert.equal(client.queuedCount, 0);
    assert.deepEqual(
      doubles.children.map((child) => childInput(child).id),
      ["first", "second", "third"],
    );
  } finally {
    doubles.restore();
  }
});

test("a zero-length queue refuses a call that cannot start immediately", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = queuedClient({ maxQueued: 0 });
    const held = client.callTool("orbit.task.show", { id: "held" });
    assert.equal(client.queuedCount, 0);
    const error = await capturedRejection(client.callTool("orbit.task.show", { id: "refused" }));
    assert.ok(error instanceof OrbitQueueFullError, `expected OrbitQueueFullError, got ${error}`);
    assert.equal(error.code, "queue_full");
    assert.equal(error.maxQueued, 0);
    assert.equal(client.queuedCount, 0);
    assert.equal(doubles.children.length, 1);
    doubles.children[0].finish(0, null);
    assert.equal(await held, null);
    assert.equal(client.activeCount, 0);
  } finally {
    doubles.restore();
  }
});

test("a queued call is cancelled when its queue-wait deadline expires", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = queuedClient({ queueWaitMs: 40 });
    const held = client.callTool("orbit.task.show", { id: "held" });
    const cancelled = capturedRejection(client.callTool("orbit.task.show", { id: "cancelled" }));
    const later = client.callTool("orbit.task.show", { id: "later" });
    assert.equal(client.queuedCount, 2);
    assert.equal(doubles.children.length, 1);
    const executionTimers = doubles.timers.filter((timer) => timer.ms === 1_000 && !timer.cleared);
    const waitTimers = doubles.timers.filter((timer) => timer.ms === 40 && !timer.cleared);
    assert.equal(executionTimers.length, 1, "the running call keeps one execution deadline");
    assert.equal(waitTimers.length, 2, "each waiter has its own queue deadline");

    waitTimers[0].fn();
    assert.equal(client.queuedCount, 1);
    assert.equal(doubles.children.length, 1);
    const error = await cancelled;
    assert.ok(error instanceof OrbitQueueTimeoutError, `expected OrbitQueueTimeoutError, got ${error}`);
    assert.equal(error.code, "queue_timeout");
    assert.equal(error.queueWaitMs, 40);
    assert.equal(error.toolName, "orbit.task.show");
    assert.equal(waitTimers[1].cleared, false, "a later waiter keeps its own deadline");

    doubles.children[0].finish(0, null);
    await held;
    assert.equal(childInput(doubles.children[1]).id, "later");
    assert.equal(client.queuedCount, 0);
    doubles.children[1].finish(0, null);
    await later;
    assert.equal(client.activeCount, 0);
    assert.equal(client.queuedCount, 0);
    assert.deepEqual(
      doubles.children.map((child) => childInput(child).id),
      ["held", "later"],
    );
  } finally {
    doubles.restore();
  }
});

test("an omitted queue deadline matches the execution timeout", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = queuedClient({ timeoutMs: 80, queueWaitMs: undefined });
    const held = client.callTool("orbit.task.show", { id: "held" });
    const waiting = capturedRejection(client.callTool("orbit.task.show", { id: "waiting" }));
    const timers = doubles.timers.filter((timer) => timer.ms === 80);
    assert.equal(timers.length, 2);
    timers[1].fn();
    const error = await waiting;
    assert.ok(error instanceof OrbitQueueTimeoutError, `expected OrbitQueueTimeoutError, got ${error}`);
    assert.equal(error.queueWaitMs, 80);
    assert.equal(error.code, "queue_timeout");
    assert.equal(timers[0].cleared, false);
    assert.equal(doubles.children.length, 1);
    assert.equal(client.queuedCount, 0);
    doubles.children[0].finish(0, null);
    assert.equal(await held, null);
  } finally {
    doubles.restore();
  }
});

test("the default queue admits 32 waiters and refuses the next", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = queuedClient({ maxQueued: undefined, queueWaitMs: 60_000 });
    // Fixed batch pinning the documented default. Not a load test.
    const admitted = [];
    for (let index = 0; index < 33; index += 1) {
      admitted.push(client.callTool("orbit.task.show", { id: `Q-${index}` }));
    }
    assert.equal(client.activeCount, 1);
    assert.equal(client.queuedCount, 32);
    assert.equal(doubles.children.length, 1);
    const error = await capturedRejection(client.callTool("orbit.task.show", { id: "refused" }));
    assert.ok(error instanceof OrbitQueueFullError, `expected OrbitQueueFullError, got ${error}`);
    assert.equal(error.code, "queue_full");
    assert.equal(error.maxQueued, 32);
    assert.equal(client.queuedCount, 32);
    assert.equal(doubles.children.length, 1);

    for (const call of admitted) {
      const running = doubles.children.find((child) => child.exitCode === null && child.closed === false);
      assert.ok(running, "expected the admitted call to be running");
      running.finish(0, null);
      assert.equal(await call, null);
    }
    assert.equal(client.activeCount, 0);
    assert.equal(client.queuedCount, 0);
    assert.equal(doubles.children.length, 33);
    assert.equal(
      doubles.children.some((child) => childInput(child).id === "refused"),
      false,
    );
  } finally {
    doubles.restore();
  }
});

test("identical polls share one queued slot", async () => {
  const doubles = installSupervisionDoubles();
  try {
    const client = queuedClient({ pollCacheTtlMs: 0 });
    const held = client.callTool("orbit.task.show", { id: "held" });
    const first = client.pollTool("orbit.task.list", { status: "backlog" });
    const second = client.pollTool("orbit.task.list", { status: "backlog" });
    assert.equal(client.queuedCount, 1);
    assert.equal(doubles.children.length, 1);
    doubles.children[0].finish(0, null);
    await held;
    assert.equal(doubles.children.length, 2);
    assert.equal(doubles.children[1].args[2], "orbit.task.list");
    doubles.children[1].finish(0, null);
    assert.equal(await first, null);
    assert.equal(await second, null);
    assert.equal(client.activeCount, 0);
    assert.equal(client.queuedCount, 0);
  } finally {
    doubles.restore();
  }
});

test("queue bounds reject a capacity or deadline that is not finite", () => {
  assert.throws(() => createOrbitClient({ maxQueued: -1 }), RangeError);
  assert.throws(() => createOrbitClient({ maxQueued: 1.5 }), RangeError);
  assert.throws(() => createOrbitClient({ queueWaitMs: 0 }), RangeError);
  assert.throws(() => createOrbitClient({ queueWaitMs: Number.POSITIVE_INFINITY }), RangeError);
});
