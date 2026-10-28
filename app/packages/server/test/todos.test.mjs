import assert from "node:assert/strict";
import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { createOrbitClient } from "../dist/orbit/client.js";
import { createTodoFeature, feature } from "../dist/features/todos/index.js";
import { CLOSED_FROM_TODOS, closePlan, todoColumns } from "../dist/features/todos/model.js";
import { closeTodo, loadTodos, scheduleTodo, TodoCloseError } from "../dist/features/todos/service.js";

const YOU = {
  id: "ORB-YOU-1",
  title: "Send the weekly follow-up",
  status: "backlog",
  terminal: false,
  tags: ["owner:you", "source:src-20261026-customer-call"],
  created_at: "2026-10-20T15:00:00Z",
  execution_summary: "",
  artifacts: [
    { path: "context/knowledge/runbooks/process-new-meetings.md" },
    { path: "context/knowledge/domain/pricing-notes.md" },
    { path: "context/sources/2026/10/src-20261026-customer-call/source.md" },
    { path: "context/artifacts/2026/10/art-20261021-qbr-template/deck.pdf" },
    { path: "output/draft.md" },
  ],
};

const YOU_LATER = {
  id: "ORB-YOU-2",
  title: "   ",
  status: "backlog",
  tags: ["owner:you", "source:ORB-99"],
  created_at: "2026-10-25T15:00:00Z",
  artifacts: [{ path: "context/knowledge/runbooks/receipt-checklist.md" }],
};

const AGENT = {
  id: "ORB-AG-1",
  title: "Draft the pricing reply",
  status: "in-progress",
  tags: ["owner:agent", "owner:office", "source:src-20261026-customer-call"],
  created_at: "2026-10-22T15:00:00Z",
  artifacts: [{ path: "context/knowledge/domain/pricing-notes.md" }],
};

const BOTH = {
  id: "ORB-BOTH",
  title: "Shared with you",
  status: "backlog",
  tags: ["owner:agent", "owner:you"],
  created_at: "2026-10-21T15:00:00Z",
  artifacts: [],
};

const DONE = {
  id: "ORB-DONE",
  title: "Already finished",
  status: "done",
  terminal: true,
  tags: ["owner:you"],
  created_at: "2026-10-01T15:00:00Z",
  artifacts: [],
};

function client(tasks, { failOn } = {}) {
  const calls = [];
  const rows = tasks.map((task) => ({ ...task, tags: [...(task.tags ?? [])] }));
  return {
    calls,
    async callTool(name, input) {
      calls.push({ name, input });
      if (failOn === calls.length) throw new Error("boom");
      if (name === "orbit.task.list") {
        const tag = input.tag;
        const matched = rows.filter((task) => tag === undefined || task.tags.includes(tag));
        return { tasks: matched, total: matched.length, truncated: false };
      }
      if (name === "orbit.task.show") {
        return rows.find((task) => task.id === input.id) ?? null;
      }
      if (name === "orbit.task.update") {
        const task = rows.find((item) => item.id === input.id);
        if (task && typeof input.status === "string") task.status = input.status;
        if (task && typeof input.execution_summary === "string") {
          task.execution_summary = input.execution_summary;
        }
        if (task && typeof input.not_before === "string") task.not_before = input.not_before === "" ? null : input.not_before;
        return task ?? null;
      }
      throw new Error(`unexpected ${name}`);
    },
    pollTool(name, input) {
      return this.callTool(name, input);
    },
    get activeCount() {
      return 0;
    },
    get queuedCount() {
      return 0;
    },
  };
}

function responseCapture() {
  return {
    status: 0,
    body: "",
    writeHead(code) {
      this.status = code;
    },
    end(payload = "") {
      this.body = payload;
    },
    json() {
      return JSON.parse(this.body);
    },
  };
}

function postRequest(body) {
  const encoded = Buffer.from(typeof body === "string" ? body : JSON.stringify(body));
  return {
    async *[Symbol.asyncIterator]() {
      yield encoded;
    },
  };
}

test("columns put your to-dos and the agents' in plain words", () => {
  const list = todoColumns([YOU, YOU_LATER, BOTH, DONE], [AGENT, BOTH, DONE], false);
  assert.deepEqual(
    list.you.map((item) => item.title),
    ["Send the weekly follow-up", "Shared with you", "Untitled to-do"],
  );
  assert.deepEqual(list.agents.map((item) => item.id), ["ORB-AG-1"]);
  assert.equal(list.you.every((item) => item.closable), true);
  assert.equal(list.agents.every((item) => item.closable), false);
  assert.equal(list.you[0].source, "Customer Call");
  assert.deepEqual(list.you[0].knowledge, [
    "Process New Meetings",
    "Pricing Notes",
    "QBR Template",
  ]);
  assert.equal(list.you[2].source, "A recorded source");
  const visible = JSON.stringify({
    source: list.you[0].source,
    knowledge: list.you[0].knowledge,
    titles: list.you.map((item) => item.title),
  });
  assert.equal(visible.includes("src-20261026-customer-call"), false);
  assert.equal(visible.includes("context/"), false);
  assert.equal(visible.includes("output/draft.md"), false);
  assert.equal(visible.includes("ORB-99"), false);
  assert.equal(visible.includes("owner:"), false);
  assert.equal(visible.includes("backlog"), false);
});

test("a to-do carries its scheduled time and whether it can still be scheduled", () => {
  const list = todoColumns(
    [{ ...YOU, not_before: "2026-10-30T09:00:00Z" }],
    [AGENT, { ...AGENT, id: "ORB-AG-2", status: "proposed", not_before: null }],
    false,
  );
  assert.equal(list.you[0].scheduled, "2026-10-30T09:00:00Z");
  assert.equal(list.you[0].schedulable, true);
  assert.deepEqual(list.agents.map((item) => [item.id, item.scheduled, item.schedulable]), [
    ["ORB-AG-1", null, false],
    ["ORB-AG-2", null, true],
  ], "an agent to-do already running keeps its time; one not yet approved can take one");
});

test("scheduling sets and clears not_before on yours and an agent's, and refuses what has started", async () => {
  const waiting = { ...AGENT, id: "ORB-AG-3", status: "backlog" };
  const orbit = client([YOU, AGENT, waiting, DONE, { ...YOU, id: "ORB-PLAIN", tags: ["office"] }]);
  assert.equal(await scheduleTodo(orbit, waiting.id, "2026-10-30T09:00:00+01:00"), "2026-10-30T09:00:00+01:00");
  assert.equal(await scheduleTodo(orbit, YOU.id, "2026-10-30T08:00:00Z"), "2026-10-30T08:00:00Z");
  assert.equal(await scheduleTodo(orbit, YOU.id, null), null);
  const updates = orbit.calls.filter((call) => call.name === "orbit.task.update").map((call) => call.input);
  assert.deepEqual(updates, [
    { id: waiting.id, not_before: "2026-10-30T09:00:00+01:00" },
    { id: YOU.id, not_before: "2026-10-30T08:00:00Z" },
    { id: YOU.id, not_before: "" },
  ], "a clear is the empty string Orbit reads as clear; no status moves");

  const refusals = [
    [AGENT.id, "2026-10-30T09:00:00Z", "not_schedulable"],
    [DONE.id, "2026-10-30T09:00:00Z", "not_schedulable"],
    ["ORB-PLAIN", "2026-10-30T09:00:00Z", "not_found"],
    ["ORB-NOPE", "2026-10-30T09:00:00Z", "not_found"],
    [YOU.id, "Friday at nine", "invalid_input"],
    [YOU.id, "2026-10-30T09:00", "invalid_input"],
    [YOU.id, "2026-02-30T09:00:00Z", "invalid_input"],
    ["../etc", "2026-10-30T09:00:00Z", "invalid_input"],
  ];
  for (const [id, at, code] of refusals) {
    await assert.rejects(scheduleTodo(orbit, id, at), (error) => error instanceof TodoCloseError && error.code === code, `${id} ${at}`);
  }
  assert.equal(orbit.calls.filter((call) => call.name === "orbit.task.update").length, 3, "a refusal sends no update");
});

test("closing walks a backlog to-do to done and refuses an agent to-do", async () => {
  assert.deepEqual(closePlan("backlog", false), [
    { status: "in-progress" },
    { status: "review" },
    { status: "done", execution_summary: CLOSED_FROM_TODOS },
  ]);
  assert.deepEqual(closePlan("review", true), [{ status: "done" }]);

  const yours = client([YOU]);
  await closeTodo(yours, YOU.id);
  assert.deepEqual(
    yours.calls.map((call) => call.name),
    ["orbit.task.show", "orbit.task.update", "orbit.task.update", "orbit.task.update"],
  );
  assert.deepEqual(
    yours.calls.slice(1).map((call) => call.input),
    [
      { id: YOU.id, status: "in-progress" },
      { id: YOU.id, status: "review" },
      { id: YOU.id, status: "done", execution_summary: CLOSED_FROM_TODOS },
    ],
  );

  const reviewed = client([{ ...YOU, status: "review", execution_summary: "Already written." }]);
  await closeTodo(reviewed, YOU.id);
  assert.deepEqual(reviewed.calls.map((call) => call.input.status), [undefined, "done"]);
  assert.equal("execution_summary" in reviewed.calls[1].input, false);

  const agent = client([AGENT]);
  await assert.rejects(closeTodo(agent, AGENT.id), (error) => {
    assert.ok(error instanceof TodoCloseError);
    assert.equal(error.code, "not_yours");
    return true;
  });
  assert.deepEqual(agent.calls.map((call) => call.name), ["orbit.task.show"]);
});

test("a failed step does not send the later updates", async () => {
  const orbit = client([YOU], { failOn: 3 });
  await assert.rejects(closeTodo(orbit, YOU.id), (error) => error instanceof TodoCloseError && error.code === "unavailable");
  assert.deepEqual(orbit.calls.map((call) => call.name), [
    "orbit.task.show",
    "orbit.task.update",
    "orbit.task.update",
  ]);
  assert.equal(orbit.calls.at(-1).input.status, "review");
});

test("the routes list and close through the Orbit client", async () => {
  assert.equal(feature.status, "built");
  assert.equal(feature.name, "todos");
  const orbit = client([YOU, AGENT]);
  const routes = createTodoFeature(orbit).routes;
  const list = routes.find((route) => route.method === "GET");
  const close = routes.find((route) => route.method === "POST");
  const listed = responseCapture();
  await list.handle({}, listed);
  const body = listed.json();
  assert.equal(listed.status, 200);
  assert.equal(body.you[0].closable, true);
  assert.equal(body.agents[0].closable, false);
  assert.equal(body.you[0].source, "Customer Call");

  const refused = responseCapture();
  await close.handle(postRequest({ id: AGENT.id }), refused);
  assert.equal(refused.status, 403);
  assert.equal(refused.json().error, "not_yours");
  assert.equal(orbit.calls.some((call) => call.name === "orbit.task.update"), false);

  const closed = responseCapture();
  await close.handle(postRequest({ id: YOU.id }), closed);
  assert.equal(closed.status, 200);
  assert.equal(closed.json().ok, true);
  const updates = orbit.calls.filter((call) => call.name === "orbit.task.update").map((call) => call.input.status);
  assert.deepEqual(updates, ["in-progress", "review", "done"]);

  const bad = responseCapture();
  await close.handle(postRequest("{"), bad);
  assert.equal(bad.status, 400);
  assert.equal(bad.json().error, "invalid_input");
});

test("the schedule route sets and clears a time, refusing unknown keys and started to-dos", async () => {
  const orbit = client([YOU, AGENT]);
  const schedule = createTodoFeature(orbit).routes.find((route) => route.path === "/api/todos/schedule");
  assert.equal(schedule.method, "POST");
  const set = responseCapture();
  await schedule.handle(postRequest({ id: YOU.id, not_before: "2026-10-30T09:00:00Z" }), set);
  assert.equal(set.status, 200);
  assert.deepEqual(set.json(), { ok: true, scheduled: "2026-10-30T09:00:00Z" });
  const cleared = responseCapture();
  await schedule.handle(postRequest({ id: YOU.id, not_before: null }), cleared);
  assert.deepEqual(cleared.json(), { ok: true, scheduled: null });

  for (const [body, status, code] of [
    [{ id: YOU.id, not_before: "2026-10-30T09:00:00Z", status: "done" }, 400, "invalid_input"],
    [{ id: YOU.id }, 400, "invalid_input"],
    [{ id: YOU.id, not_before: 1_793_000_000 }, 400, "invalid_input"],
    [{ id: AGENT.id, not_before: "2026-10-30T09:00:00Z" }, 409, "not_schedulable"],
  ]) {
    const refused = responseCapture();
    await schedule.handle(postRequest(body), refused);
    assert.equal(refused.status, status, JSON.stringify(body));
    assert.equal(refused.json().error, code);
  }
  const updates = orbit.calls.filter((call) => call.name === "orbit.task.update").map((call) => call.input);
  assert.deepEqual(updates, [{ id: YOU.id, not_before: "2026-10-30T09:00:00Z" }, { id: YOU.id, not_before: "" }]);
});

test("the Orbit client subprocess moves an owner:you to-do to done", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "todos-orbit-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const bin = join(root, "bin");
  const state = join(root, "state");
  const { mkdir } = await import("node:fs/promises");
  await mkdir(bin, { recursive: true });
  await mkdir(state, { recursive: true });
  const tasks = [
    { ...YOU, artifacts: YOU.artifacts },
    { ...AGENT, artifacts: AGENT.artifacts },
  ];
  await writeFile(join(state, "tasks.json"), JSON.stringify(tasks));
  await writeFile(join(bin, "orbit"), FAKE_ORBIT);
  await chmod(join(bin, "orbit"), 0o755);
  const env = {
    ...process.env,
    PATH: `${bin}:${process.env.PATH ?? ""}`,
    FAKE_ORBIT_STATE: state,
  };
  const orbit = createOrbitClient({ env, timeoutMs: 10_000 });

  const listed = await loadTodos(orbit);
  assert.equal(listed.you[0].closable, true);
  assert.equal(listed.you[0].source, "Customer Call");
  assert.equal(listed.agents[0].closable, false);
  assert.deepEqual(listed.agents[0].knowledge, ["Pricing Notes"]);

  await closeTodo(orbit, YOU.id);
  const stored = JSON.parse(await readFile(join(state, "tasks.json"), "utf8"));
  assert.equal(stored.find((task) => task.id === YOU.id).status, "done");
  assert.equal(stored.find((task) => task.id === AGENT.id).status, "in-progress");

  const invocations = (await readFile(join(state, "invocations.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line));
  const updates = invocations
    .filter((args) => args[2] === "orbit.task.update")
    .map((args) => JSON.parse(args[args.indexOf("--input") + 1]));
  assert.deepEqual(updates.map((input) => input.status), ["in-progress", "review", "done"]);
  assert.equal(updates.at(-1).execution_summary, CLOSED_FROM_TODOS);
  assert.equal(updates.every((input) => input.id === YOU.id), true);

  await assert.rejects(closeTodo(orbit, AGENT.id), (error) => error instanceof TodoCloseError && error.code === "not_yours");
  const afterRefuse = (await readFile(join(state, "invocations.log"), "utf8"))
    .split("\n")
    .filter((line) => line !== "")
    .map((line) => JSON.parse(line))
    .filter((args) => args[2] === "orbit.task.update");
  assert.equal(afterRefuse.length, updates.length);

  // Schedule and clear an agent to-do that has not started, through the same subprocess.
  const waiting = JSON.parse(await readFile(join(state, "tasks.json"), "utf8"));
  waiting.push({ ...AGENT, id: "ORB-AG-WAIT", status: "backlog", not_before: null });
  await writeFile(join(state, "tasks.json"), JSON.stringify(waiting));
  await scheduleTodo(orbit, "ORB-AG-WAIT", "2026-10-30T09:00:00Z");
  const scheduled = await loadTodos(orbit);
  assert.equal(scheduled.agents.find((item) => item.id === "ORB-AG-WAIT").scheduled, "2026-10-30T09:00:00Z");
  await scheduleTodo(orbit, "ORB-AG-WAIT", null);
  const unscheduled = await loadTodos(orbit);
  assert.equal(unscheduled.agents.find((item) => item.id === "ORB-AG-WAIT").scheduled, null);
});

const FAKE_ORBIT = `#!/usr/bin/env node
import { appendFileSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const state = process.env.FAKE_ORBIT_STATE ?? ".";
const args = process.argv.slice(2);
appendFileSync(join(state, "invocations.log"), JSON.stringify(args) + "\\n");
if (args[0] !== "tool" || args[1] !== "run") {
  process.stderr.write("unknown\\n");
  process.exit(2);
}
const tool = args[2] ?? "";
const input = JSON.parse(args[args.indexOf("--input") + 1] ?? "{}");
const tasks = JSON.parse(readFileSync(join(state, "tasks.json"), "utf8"));
if (tool === "orbit.task.list") {
  const tag = typeof input.tag === "string" ? input.tag : "";
  const rows = tag === "" ? tasks : tasks.filter((task) => (task.tags ?? []).includes(tag));
  process.stdout.write(JSON.stringify({ tasks: rows, total: rows.length, truncated: false }) + "\\n");
  process.exit(0);
}
if (tool === "orbit.task.show") {
  process.stdout.write(JSON.stringify(tasks.find((task) => task.id === input.id) ?? null) + "\\n");
  process.exit(0);
}
if (tool === "orbit.task.update") {
  const task = tasks.find((item) => item.id === input.id);
  if (!task) {
    process.stderr.write("missing\\n");
    process.exit(1);
  }
  if (typeof input.status === "string") task.status = input.status;
  if (input.status === "done") task.terminal = true;
  if (typeof input.execution_summary === "string") task.execution_summary = input.execution_summary;
  if (typeof input.not_before === "string") task.not_before = input.not_before === "" ? null : input.not_before;
  writeFileSync(join(state, "tasks.json"), JSON.stringify(tasks));
  process.stdout.write(JSON.stringify(task) + "\\n");
  process.exit(0);
}
process.stderr.write("unknown tool\\n");
process.exit(2);
`;
