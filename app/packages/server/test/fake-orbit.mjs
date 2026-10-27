/**
 * Test helper: install a fake `orbit` executable on a temp PATH.
 *
 * The fake logs every invocation (one JSON line per call) to
 * `<state>/invocations.log`, brackets tool calls with `S`/`E` events in
 * `<state>/events.log` so tests can replay concurrency, answers
 * `orbit.task.list`, `orbit.task.show` and `orbit.task.update` from
 * `<state>/tasks.json` when present (updates are written back), artifacts from
 * `<state>/artifacts.json` (`{task: {path: text}}`), and records `orbit run
 * ship` ids in `<state>/ship.log`. Behaviour knobs arrive through the
 * environment the client is given: `FAKE_ORBIT_STATE`, plus per-call
 * `delayMs` / `hang` / `fail` fields in the tool input.
 */
import { chmodSync, mkdirSync, writeFileSync } from "node:fs";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const FAKE_SOURCE = `#!/usr/bin/env node
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const state = process.env.FAKE_ORBIT_STATE ?? ".";
const args = process.argv.slice(2);
appendFileSync(join(state, "invocations.log"), JSON.stringify(args) + "\\n");

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function main() {
  if (args[0] === "plugin" && args[1] === "secret") {
    if (args[2] === "set") {
      const chunks = [];
      for await (const chunk of process.stdin) chunks.push(chunk);
      const value = Buffer.concat(chunks).toString("utf8");
      appendFileSync(join(state, "secret-stdin.log"), JSON.stringify({ slot: args[4], value }) + "\\n");
      // Deliberately hostile output: the server must discard both streams.
      process.stdout.write(value);
      process.stderr.write(value);
      if (process.env.FAKE_SECRET_FAIL === "1") process.exitCode = 3;
      else {
        if (args[3] === "office" && args[4] === "open_weights_token") {
          let office = {};
          try { office = JSON.parse(readFileSync(join(state, "office.json"), "utf8")); } catch {}
          office.token = "set";
          writeFileSync(join(state, "office.json"), JSON.stringify(office));
        }
        try {
          const fixtures = JSON.parse(readFileSync(join(state, "gateway.json"), "utf8"));
          for (const item of fixtures["gateway.connection-list"].connections) {
            if (item.slot === args[4]) item.slot_state = "set";
          }
          writeFileSync(join(state, "gateway.json"), JSON.stringify(fixtures));
        } catch {}
      }
    }
    return;
  }
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
      if (tool === "orbit.auto_task.mint") {
        if (input.name === "no-such-auto-task") {
          process.stderr.write(JSON.stringify({ error: "auto-task not found: no-such-auto-task", code: "auto_task_not_found" }) + "\\n");
          process.exitCode = 1;
          return;
        }
        if (input.name === "denied-auto-task") {
          process.stderr.write(JSON.stringify({ error: "capability denied: operation requires the operator capability", code: "capability_denied" }) + "\\n");
          process.exitCode = 1;
          return;
        }
        process.stdout.write(JSON.stringify({
          minted: true,
          name: input.name,
          workspace: input.workspace ?? null,
          acknowledge_unconditional: input.acknowledge_unconditional ?? null,
          task: { id: "ORB-FAKE-MINT-1", title: "Minted " + String(input.name) },
        }) + "\\n");
        return;
      }
      let stored = null;
      if (tool === "orbit.task.show" || tool === "orbit.task.update") {
        try { stored = JSON.parse(readFileSync(join(state, "tasks.json"), "utf8")); } catch {}
      }
      // Without a tasks.json these fall through to the generic echo other tests rely on.
      if (Array.isArray(stored)) {
        const tasks = stored;
        const task = tasks.find((row) => row.id === input.id);
        if (task === undefined) {
          process.stderr.write(JSON.stringify({ error: "task not found", code: "task_not_found" }) + "\\n");
          process.exitCode = 1;
          return;
        }
        if (tool === "orbit.task.update") {
          if (typeof input.status === "string") task.status = input.status;
          if (typeof input.execution_summary === "string") task.execution_summary = input.execution_summary;
          if (typeof input.comment === "string") task.comments = [...(task.comments ?? []), input.comment];
          writeFileSync(join(state, "tasks.json"), JSON.stringify(tasks));
        }
        process.stdout.write(JSON.stringify(task) + "\\n");
        return;
      }
      if (tool === "orbit.task.artifact.get") {
        let artifacts = {};
        try { artifacts = JSON.parse(readFileSync(join(state, "artifacts.json"), "utf8")); } catch {}
        const content = artifacts[input.id] && artifacts[input.id][input.path];
        if (typeof content !== "string") {
          process.stderr.write(JSON.stringify({ error: "artifact not found", code: "artifact_not_found" }) + "\\n");
          process.exitCode = 1;
          return;
        }
        process.stdout.write(JSON.stringify({ id: input.id, path: input.path, encoding: "utf8", content }) + "\\n");
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
      if (tool.startsWith("gateway.")) {
        let fixtures = {};
        try { fixtures = JSON.parse(readFileSync(join(state, "gateway.json"), "utf8")); } catch {}
        if (tool === "gateway.connection-put") {
          const listed = fixtures["gateway.connection-list"] ?? { connections: [], free_slots: 16 };
          const previous = listed.connections.find((item) => item.name === input.name);
          const saved = { ...input, slot: input.auth.scheme === "none" ? null : "conn_03", slot_state: previous?.slot_state ?? "unset" };
          fixtures["gateway.connection-list"] = { ...listed, connections: [...listed.connections.filter((item) => item.name !== input.name), saved] };
          writeFileSync(join(state, "gateway.json"), JSON.stringify(fixtures));
          process.stdout.write(JSON.stringify({
            connection: saved, created: previous === undefined, released_slot: fixtures.released_slot ?? null,
          }));
        } else {
          process.stdout.write(JSON.stringify(fixtures[tool] ?? {}));
        }
        return;
      }
            if (tool.startsWith("office.")) {
        let office = {};
        try { office = JSON.parse(readFileSync(join(state, "office.json"), "utf8")); } catch {}
        const refused = office.refuse && typeof office.refuse[tool] === "string" ? office.refuse[tool] : "";
        if (refused) {
          process.stderr.write(JSON.stringify({ error: "refused by fixture", code: refused }) + "\\n");
          process.exitCode = 1;
          return;
        }
        const failed = office.fail && typeof office.fail[tool] === "string" ? office.fail[tool] : "";
        if (failed) {
          process.stdout.write(JSON.stringify({ ok: false, error: { code: "fixture_failed", message: failed, retryable: false } }));
          return;
        }
        if (office.tools && Object.prototype.hasOwnProperty.call(office.tools, tool)) {
          process.stdout.write(JSON.stringify(office.tools[tool]));
          return;
        }
        if (tool === "office.config-set") {
          office.blocks = office.blocks || {};
          if (input.value === null) delete office.blocks[input.key];
          else office.blocks[input.key] = input.value;
          writeFileSync(join(state, "office.json"), JSON.stringify(office));
          process.stdout.write(JSON.stringify({ ok: true, output: { path: ".office/config.yaml", key: input.key } }));
          return;
        }
        if (tool === "office.private-route-check") {
          const block = (office.blocks && office.blocks.open_weights) || {};
          const label = block.label || "Acme";
          const kind = block.kind || "company";
          const model = block.model || "muse";
          const output = {
            path: ".office/config.yaml",
            label: label,
            kind: kind,
            route: "Private: " + label,
            model: model,
            reported_model: model,
            capabilities: ["text", "image"],
            routable: true,
            checks: [
              { capability: "text", accepted: true, elapsed_ms: 1, error: null, note: null },
              { capability: "image", accepted: true, elapsed_ms: 1, error: null, note: null },
              { capability: "audio", accepted: false, elapsed_ms: 1, error: "unsupported", note: "this server doesn't take audio, so private videos can't be transcribed" },
            ],
          };
          office.blocks = office.blocks || {};
          office.blocks.open_weights = Object.assign({}, block, { capabilities: output.capabilities, verified_at: "2026-10-26T00:00:00Z" });
          writeFileSync(join(state, "office.json"), JSON.stringify(office));
          process.stdout.write(JSON.stringify({ ok: true, output: output }));
          return;
        }
        if (tool === "office.status") {
          const block = office.blocks && office.blocks.open_weights;
          let openWeights = null;
          if (block) {
            openWeights = {
              endpoint: block.endpoint,
              model: block.model,
              kind: block.kind,
              label: block.label,
              capabilities: Object.prototype.hasOwnProperty.call(block, "capabilities") ? block.capabilities : null,
              verified_at: Object.prototype.hasOwnProperty.call(block, "verified_at") ? block.verified_at : null,
            };
          }
          process.stdout.write(JSON.stringify({ ok: true, output: {
            version: "0.1.0",
            secrets: { meta_api_key: "unset", open_weights_token: office.token || "unset" },
            library: {
              initialized: openWeights !== null,
              documents: 0,
              documents_by_kind: { source: 0, domain: 0, runbook: 0, artifact: 0 },
              pages_off_template: 0,
              template_invalid: [],
            },
            open_weights: openWeights,
          }}));
          return;
        }
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
