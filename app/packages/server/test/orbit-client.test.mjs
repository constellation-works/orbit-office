import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { test } from "node:test";

import { createOrbitClient, OrbitExitError, OrbitTimeoutError } from "../dist/orbit/client.js";
import { installFakeOrbit } from "./fake-orbit.mjs";

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
