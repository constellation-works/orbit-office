/**
 * Test helper: install a fake `orbit` executable on a temp PATH.
 *
 * The fake logs every invocation (one JSON line per call) to
 * `<state>/invocations.log`, brackets tool calls with `S`/`E` events in
 * `<state>/events.log` so tests can replay concurrency, answers
 * `orbit.task.list` from `<state>/tasks.json`, and records `orbit run
 * ship` ids in `<state>/ship.log`. Behaviour knobs arrive through the
 * environment the client is given: `FAKE_ORBIT_STATE`, plus per-call
 * `delayMs` / `hang` / `fail` fields in the tool input.
 */
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const FAKE_SOURCE = `#!/usr/bin/env node
import { appendFileSync, readFileSync } from "node:fs";
import { join } from "node:path";

const state = process.env.FAKE_ORBIT_STATE ?? ".";
const args = process.argv.slice(2);
appendFileSync(join(state, "invocations.log"), JSON.stringify(args) + "\\n");

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  if (args[0] === "tool" && args[1] === "run") {
    const tool = args[2] ?? "";
    const inputIndex = args.indexOf("--input");
    let input = {};
    try {
      input = JSON.parse(args[inputIndex + 1] ?? "{}");
    } catch {
      input = {};
    }
    appendFileSync(join(state, "events.log"), "S\\n");
    try {
      const listDelay = Number(process.env.FAKE_ORBIT_LIST_DELAY_MS ?? 0);
      if (tool === "orbit.task.list" && listDelay > 0) await sleep(listDelay);
      if (typeof input.delayMs === "number") await sleep(input.delayMs);
      if (input.hang === true) await sleep(60_000);
      if (input.fail === true) {
        process.stderr.write("fake orbit failure\\n");
        process.exitCode = 3;
        return;
      }
      if (tool === "orbit.task.list") {
        let tasks = [];
        try {
          tasks = JSON.parse(readFileSync(join(state, "tasks.json"), "utf8"));
        } catch {
          tasks = [];
        }
        process.stdout.write(JSON.stringify({ tasks, total: tasks.length, truncated: false }) + "\\n");
        return;
      }
      process.stdout.write(JSON.stringify({ ok: true, tool, input }) + "\\n");
    } finally {
      appendFileSync(join(state, "events.log"), "E\\n");
    }
    return;
  }
  if (args[0] === "run" && args[1] === "ship") {
    const shipDelay = Number(process.env.FAKE_ORBIT_SHIP_DELAY_MS ?? 0);
    if (shipDelay > 0) await sleep(shipDelay);
    appendFileSync(join(state, "ship.log"), JSON.stringify(args.slice(2)) + "\\n");
    process.stdout.write("shipped\\n");
    return;
  }
  process.stderr.write("unknown fake orbit invocation\\n");
  process.exitCode = 2;
}

void main();
`;

/**
 * Create a temp dir holding a fake `orbit`. Returns its `bin` dir, the
 * state dir and an env (extending `baseEnv`) that puts the fake first on
 * PATH. Caller owns cleanup via `t.after` or manual removal.
 */
export async function installFakeOrbit() {
  const root = await mkdtemp(join(tmpdir(), "fake-orbit-"));
  const bin = join(root, "bin");
  const state = join(root, "state");
  mkdirSync(bin, { recursive: true });
  mkdirSync(state, { recursive: true });
  const executable = join(bin, "orbit");
  writeFileSync(executable, FAKE_SOURCE);
  chmodSync(executable, 0o755);
  const env = (extra = {}) => ({
    ...process.env,
    ...extra,
    PATH: `${bin}:${process.env.PATH ?? ""}`,
    FAKE_ORBIT_STATE: state,
  });
  return { root, bin, state, env };
}
