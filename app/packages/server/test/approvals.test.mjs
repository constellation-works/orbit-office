import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { test } from "node:test";

import { createApprovalsFeature } from "../dist/features/approvals/index.js";
import { OrbitExitError, OrbitTimeoutError } from "../dist/orbit/client.js";

const PROPOSAL = JSON.stringify({ kind: "knowledge_add", markdown: "# Ingest service\n\nWho runs it and how." }, null, 2);
const PROPOSAL_SHA = createHash("sha256").update(PROPOSAL).digest("hex");
const DRAFT_SHA = "a".repeat(64);
const DRAFT = JSON.stringify({
  schema_version: 1, id: "gd-0123456789abcdef", connection: "acme-crm",
  request: { method: "POST", url: "https://api.acme.example/v2/notes", headers: { "Content-Type": "application/json" }, body: "{\"text\":\"Hello\"}" },
  sha256: DRAFT_SHA, created_at: "2026-10-27T00:00:00.000Z",
});

function tasks() {
  return [
    { id: "T-review", title: "Weekly brief", status: "review", tags: ["owner:agent"], execution_summary: "Drafted the brief.", artifacts: [{ path: "brief.md" }, { path: "artifact-manifest.json" }] },
    { id: "T-script", title: "Receipt catalog", status: "proposed", tags: ["script-output"], execution_summary: null, artifacts: [{ path: "receipts.csv" }, { path: "artifact-manifest.json" }, { path: "script-run.json" }] },
    { id: "T-page", title: "Ingest service", status: "proposed", tags: ["proposal", "proposal:knowledge_add", "onboarding"], artifacts: [{ path: "proposal.json" }] },
    { id: "T-page2", title: "Team rhythm", status: "proposed", tags: ["proposal", "proposal:knowledge_add", "onboarding"], artifacts: [{ path: "proposal.json" }] },
    { id: "T-page3", title: "Expenses", status: "proposed", tags: ["proposal", "proposal:knowledge_add", "onboarding"], artifacts: [{ path: "proposal.json" }] },
    { id: "T-draft", title: "Add a note to the account", status: "proposed", tags: ["gateway-draft", "proposal", "proposal:gateway_draft"], artifacts: [{ path: "gateway-draft.json" }] },
    { id: "T-mine", title: "Call the bank", status: "backlog", tags: ["owner:you"], artifacts: [] },
  ];
}

/** In-memory Orbit: tool replies and refusals per call, every call recorded in order. */
function fakeClient() {
  const state = { tasks: tasks(), calls: [], replies: {}, failures: {} };
  const artifacts = { "T-page": PROPOSAL, "T-page2": PROPOSAL, "T-page3": PROPOSAL, "T-draft": DRAFT };
  const refuse = (name, code) => new OrbitExitError(name, 1, null, JSON.stringify({ error: "refused", code }));
  state.client = {
    async callTool(name, input) {
      state.calls.push({ name, input: structuredClone(input) });
      const failure = state.failures[`${name} ${input.id ?? input.task ?? ""}`] ?? state.failures[name];
      if (typeof failure === "string") throw refuse(name, failure);
      if (failure instanceof Error) throw failure;
      if (state.replies[name] !== undefined) return structuredClone(state.replies[name]);
      const task = state.tasks.find((row) => row.id === (input.id ?? input.task));
      if (name === "orbit.task.show") {
        if (task === undefined) throw refuse(name, "task_not_found");
        return structuredClone(task);
      }
      if (name === "orbit.task.update") {
        if (input.status !== undefined) task.status = input.status;
        return structuredClone(task);
      }
      if (name === "orbit.task.artifact.get") return { id: input.id, path: input.path, encoding: "utf8", content: artifacts[input.id] };
      if (name === "office.artifact-promote") return { ok: true, output: { id: "art-20261027-brief", path: "artifacts/x", files: [], series: "brief", version: 1, supersedes: null } };
      if (name === "office.proposal-accept") {
        task.status = "done";
        return { ok: true, output: { id: input.id, accepted: true, change: { page: "dom-x", path: "knowledge/x.md" } } };
      }
      if (name === "office.proposal-reconcile") return { ok: true, output: { moved: [], unknown: [] } };
      if (name === "gateway.http-send") return { draft: input.draft, task_id: "T-draft", outcome: "sent", status: 201, task_closed: true };
      throw new Error(`unexpected tool ${name}`);
    },
  };
  return state;
}

async function serve(t, state) {
  const feature = createApprovalsFeature({ client: state.client, approver: () => "Dana" });
  assert.equal(feature.status, "built");
  const server = createServer((request, response) => {
    const path = (request.url ?? "/").split("?")[0];
    const route = feature.routes.find((item) => item.method === request.method && item.path === path);
    if (route) void route.handle(request, response);
    else { response.writeHead(404); response.end(); }
  });
  t.after(() => new Promise((resolve) => server.close(resolve)));
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const base = `http://127.0.0.1:${server.address().port}`;
  return {
    async post(path, body) {
      const response = await fetch(base + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
      return { status: response.status, body: await response.json() };
    },
    async get(path) {
      const response = await fetch(base + path);
      return { status: response.status, body: await response.json() };
    },
  };
}

const names = (state, prefix = "") => state.calls.map(({ name }) => name).filter((name) => name.startsWith(prefix));

test("approving reviewed work promotes first, then moves it to done with who approved", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  const result = await api.post("/api/approvals/approve", { key: "T-review" });
  assert.equal(result.status, 200);
  assert.deepEqual(result.body, { approved: true, closed: true });
  assert.deepEqual(names(state), ["orbit.task.show", "office.artifact-promote", "orbit.task.update"]);
  assert.deepEqual(state.calls[1].input, { task: "T-review" });
  const update = state.calls[2].input;
  assert.equal(update.status, "done");
  assert.match(update.comment, /Approved by Dana/);
  assert.equal(update.verdict, undefined);
  assert.equal(update.complete, undefined);
  assert.equal(update.execution_summary, undefined, "an existing summary is kept");
});

test("a refused promote leaves the task in review and says why", async (t) => {
  const state = fakeClient();
  state.failures["office.artifact-promote"] = "task_has_no_artifacts";
  const api = await serve(t, state);
  const result = await api.post("/api/approvals/approve", { key: "T-review" });
  assert.equal(result.status, 409);
  assert.deepEqual(result.body, { error: "promote_refused", reason: "task_has_no_artifacts" });
  assert.deepEqual(names(state, "orbit.task.update"), []);
  assert.equal(state.tasks[0].status, "review");

  state.failures["office.artifact-promote"] = new OrbitTimeoutError("office.artifact-promote", 30_000);
  const timeout = await api.post("/api/approvals/approve", { key: "T-review" });
  assert.equal(timeout.status, 502);
  assert.equal(timeout.body.error, "unavailable");
  assert.equal(state.tasks[0].status, "review");
});

test("a closing failure after promote reports the output kept and the task still open", async (t) => {
  const state = fakeClient();
  state.failures["orbit.task.update"] = "transition_refused";
  const api = await serve(t, state);
  const result = await api.post("/api/approvals/approve", { key: "T-review" });
  assert.deepEqual(result.body, { approved: true, closed: false });
});

test("sending back moves reviewed work to backlog with the comment; it needs a comment", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  assert.equal((await api.post("/api/approvals/send-back", { key: "T-review", comment: "  " })).status, 400);
  const result = await api.post("/api/approvals/send-back", { key: "T-review", comment: "Add the budget table." });
  assert.equal(result.status, 200);
  const update = state.calls.find(({ name }) => name === "orbit.task.update").input;
  assert.equal(update.status, "backlog");
  assert.match(update.comment, /Sent back by Dana.*Add the budget table\./);
  assert.deepEqual(names(state, "office."), []);
});

test("a proposal's detail is the reviewed text and the digest of its exact bytes", async (t) => {
  const api = await serve(t, fakeClient());
  const result = await api.get("/api/approvals/detail?key=T-page");
  assert.equal(result.status, 200);
  assert.deepEqual(result.body, { kind: "proposal", key: "T-page", preview: "# Ingest service\n\nWho runs it and how.", sha256: PROPOSAL_SHA, shareable: false, conflict: false });
  assert.equal((await api.get("/api/approvals/detail?key=T-mine")).status, 409, "a personal to-do has no approval detail");
  assert.equal((await api.get("/api/approvals/detail?key=../x")).status, 400);
  assert.equal((await api.get("/api/approvals/detail?key=T-page&extra=1")).status, 400);
});

test("accepting a proposal calls office.proposal-accept exactly once with the digest shown", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  const result = await api.post("/api/approvals/accept", { key: "T-page", sha256: PROPOSAL_SHA });
  assert.deepEqual(result, { status: 200, body: { ok: true } });
  assert.deepEqual(state.calls.map(({ name, input }) => [name, input]), [["office.proposal-accept", { id: "T-page", sha256: PROPOSAL_SHA }]]);

  state.calls.length = 0;
  state.failures["office.proposal-accept"] = "proposal_payload_changed";
  const refused = await api.post("/api/approvals/accept", { key: "T-page2", sha256: PROPOSAL_SHA });
  assert.deepEqual(refused, { status: 409, body: { error: "accept_refused", reason: "proposal_payload_changed" } });
  assert.equal(state.calls.length, 1, "a refusal is never retried");
  assert.equal((await api.post("/api/approvals/accept", { key: "T-page", sha256: "nothex" })).status, 400);
  assert.equal((await api.post("/api/approvals/accept", { key: "T-page", sha256: PROPOSAL_SHA, force: true })).status, 400, "unknown keys are refused");
});

test("accept all accepts each in turn; one refusal leaves the others accepted and is named", async (t) => {
  const state = fakeClient();
  state.failures["office.proposal-accept T-page2"] = "proposal_closed";
  const api = await serve(t, state);
  const proposals = ["T-page", "T-page2", "T-page3"].map((key) => ({ key, sha256: PROPOSAL_SHA }));
  const result = await api.post("/api/approvals/accept-all", { proposals });
  assert.equal(result.status, 200);
  assert.deepEqual(result.body.results, [
    { key: "T-page", accepted: true },
    { key: "T-page2", accepted: false, error: "proposal_closed" },
    { key: "T-page3", accepted: true },
  ]);
  assert.deepEqual(state.calls.map(({ input }) => input.id), ["T-page", "T-page2", "T-page3"]);
  assert.deepEqual(state.tasks.filter((row) => row.id.startsWith("T-page")).map((row) => row.status), ["done", "proposed", "done"]);
  assert.equal((await api.post("/api/approvals/accept-all", { proposals: [proposals[0], proposals[0]] })).status, 400);
  assert.equal((await api.post("/api/approvals/accept-all", { proposals: [] })).status, 400);
});

test("Orbit refusing the app's authority is not_permitted on every user-approval route, and nothing changes", async (t) => {
  for (const code of ["capability_denied", "policy_denied"]) {
    const state = fakeClient();
    for (const name of ["office.proposal-accept", "orbit.task.update", "gateway.http-send", "office.artifact-promote"]) state.failures[name] = code;
    const api = await serve(t, state);
    assert.deepEqual(await api.post("/api/approvals/accept", { key: "T-page", sha256: PROPOSAL_SHA }), { status: 409, body: { error: "accept_refused", reason: "not_permitted" } }, code);
    const all = await api.post("/api/approvals/accept-all", { proposals: ["T-page", "T-page2"].map((key) => ({ key, sha256: PROPOSAL_SHA })) });
    assert.deepEqual(all.body.results, [{ key: "T-page", accepted: false, error: "not_permitted" }, { key: "T-page2", accepted: false, error: "not_permitted" }]);
    assert.deepEqual(await api.post("/api/approvals/dismiss", { key: "T-page3" }), { status: 409, body: { error: "dismiss_refused", reason: "not_permitted" } });
    assert.deepEqual(await api.post("/api/approvals/send", { key: "T-draft", sha256: DRAFT_SHA }), { status: 409, body: { error: "send_refused", reason: "not_permitted" } });
    assert.deepEqual(await api.post("/api/approvals/approve", { key: "T-review" }), { status: 409, body: { error: "promote_refused", reason: "not_permitted" } });
    assert.deepEqual(await api.post("/api/approvals/send-back", { key: "T-review", comment: "More detail." }), { status: 409, body: { error: "send_back_refused", reason: "not_permitted" } });
    assert.deepEqual(state.tasks.map((row) => row.status), tasks().map((row) => row.status), "a refused authority changes no task");
    assert.equal(names(state, "office.proposal-reconcile").length, 0);
  }
});

test("dismissing a proposal records the rejection with the reason, then files its page under dismissed", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  const result = await api.post("/api/approvals/dismiss", { key: "T-page", reason: "Not our team." });
  assert.equal(result.status, 200);
  assert.deepEqual(names(state), ["orbit.task.show", "orbit.task.update", "office.proposal-reconcile"]);
  const update = state.calls[1].input;
  assert.equal(update.status, "rejected");
  assert.match(update.comment, /Dismissed by Dana.*Not our team\./);
  assert.deepEqual(state.calls[2].input, {});
  assert.equal((await api.post("/api/approvals/dismiss", { key: "T-review" })).status, 409, "reviewed work is sent back, not dismissed");
});

test("a failed page move after dismissing still reports the dismissal", async (t) => {
  const state = fakeClient();
  state.failures["office.proposal-reconcile"] = "proposal_history_incomplete";
  const api = await serve(t, state);
  assert.equal((await api.post("/api/approvals/dismiss", { key: "T-page2" })).status, 200);
  assert.equal(state.tasks.find((row) => row.id === "T-page2").status, "rejected");
  assert.deepEqual(names(state, "office."), ["office.proposal-reconcile"], "never retried");
});

test("a proposal's review preview comes from its task by id, wherever its page is filed", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  const result = await api.get("/api/approvals/detail?key=T-page");
  assert.equal(result.body.preview, "# Ingest service\n\nWho runs it and how.");
  assert.deepEqual(state.calls.map(({ name, input }) => [name, input.id, input.path]), [
    ["orbit.task.show", "T-page", undefined],
    ["orbit.task.artifact.get", "T-page", "proposal.json"],
  ], "no Library page path is read");
});

test("script output shows its files; approve promotes it and walks it closed; dismiss rejects it", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  const detail = await api.get("/api/approvals/detail?key=T-script");
  assert.deepEqual(detail.body, { kind: "script_output", key: "T-script", files: ["receipts.csv"] });
  const approved = await api.post("/api/approvals/approve", { key: "T-script" });
  assert.deepEqual(approved.body, { approved: true, closed: true });
  const steps = state.calls.filter(({ name }) => name !== "orbit.task.show" && name !== "orbit.task.artifact.get");
  assert.deepEqual(steps.map(({ name, input }) => name === "orbit.task.update" ? input.status : name),
    ["office.artifact-promote", "backlog", "in-progress", "review", "done"]);
  assert.ok(steps.at(-1).input.execution_summary.includes("Dana"), "done needs a summary when there is none");

  const other = fakeClient();
  const otherApi = await serve(t, other);
  assert.equal((await otherApi.post("/api/approvals/dismiss", { key: "T-script" })).status, 200);
  assert.equal(other.tasks[1].status, "rejected");
  assert.deepEqual(names(other, "office."), []);
});

test("a gateway draft shows method, URL and body; approve sends with the draft id and the digest shown", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  const detail = await api.get("/api/approvals/detail?key=T-draft");
  assert.deepEqual(detail.body, {
    kind: "gateway_draft", key: "T-draft", connection: "acme-crm", method: "POST",
    url: "https://api.acme.example/v2/notes", body: "{\"text\":\"Hello\"}", sha256: DRAFT_SHA,
  });
  const sent = await api.post("/api/approvals/send", { key: "T-draft", sha256: DRAFT_SHA });
  assert.deepEqual(sent, { status: 200, body: { outcome: "sent", status: 201, task_closed: true } });
  const sends = state.calls.filter(({ name }) => name === "gateway.http-send");
  assert.deepEqual(sends.map(({ input }) => input), [{ draft: "gd-0123456789abcdef", sha256: DRAFT_SHA }]);
  assert.deepEqual(names(state, "office."), [], "a draft is never promoted or accepted");
});

test("a lost gateway reply is outcome unknown, never a failure, and is not retried", async (t) => {
  const state = fakeClient();
  state.replies["gateway.http-send"] = { draft: "gd-0123456789abcdef", task_id: "T-draft", outcome: "outcome_unknown", status: null, task_closed: false, message: "check" };
  const api = await serve(t, state);
  const reported = await api.post("/api/approvals/send", { key: "T-draft", sha256: DRAFT_SHA });
  assert.deepEqual(reported, { status: 200, body: { outcome: "outcome_unknown", status: null, task_closed: false } });

  delete state.replies["gateway.http-send"];
  state.failures["gateway.http-send"] = new OrbitTimeoutError("gateway.http-send", 30_000);
  const timedOut = await api.post("/api/approvals/send", { key: "T-draft", sha256: DRAFT_SHA });
  assert.deepEqual(timedOut, { status: 200, body: { outcome: "outcome_unknown", status: null, task_closed: false } });
  assert.equal(names(state, "gateway.").length, 2, "one call per approval");

  state.failures["gateway.http-send"] = "draft_changed";
  const refused = await api.post("/api/approvals/send", { key: "T-draft", sha256: DRAFT_SHA });
  assert.deepEqual(refused, { status: 409, body: { error: "send_refused", reason: "draft_changed" } });
});

test("dismissing a gateway draft rejects the task and never sends", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  assert.equal((await api.post("/api/approvals/dismiss", { key: "T-draft" })).status, 200);
  assert.equal(state.tasks.find((row) => row.id === "T-draft").status, "rejected");
  assert.deepEqual(names(state, "gateway."), []);
  assert.equal((await api.post("/api/approvals/send", { key: "T-draft", sha256: DRAFT_SHA })).status, 409, "a dismissed draft is not sendable from the app");
  assert.deepEqual(names(state, "gateway."), []);
});

test("malformed bodies and unknown tasks are refused before any write", async (t) => {
  const state = fakeClient();
  const api = await serve(t, state);
  assert.equal((await api.post("/api/approvals/approve", { key: "T-review", edited: true })).status, 400);
  assert.equal((await api.post("/api/approvals/approve", { key: "" })).status, 400);
  assert.equal((await api.post("/api/approvals/approve", { key: "T-none" })).status, 404);
  assert.equal((await api.post("/api/approvals/approve", { key: "T-mine" })).status, 409);
  assert.deepEqual(names(state, "office."), []);
  assert.deepEqual(names(state, "orbit.task.update"), []);
});

const SHARED_UPDATE = JSON.stringify({ kind: "knowledge_update", page: "dom-client-acme", markdown: "# Acme\n\nBob's update.", shareable: true });
const SHARED_CONFLICT = JSON.stringify({ kind: "shared_conflict", page: "dom-client-acme", shared_path: "knowledge/clients/dom-client-acme.md", shared_blob: "b".repeat(40), markdown: "# Acme\n\nTheirs.", own_sha256: "c".repeat(64) });

function sharedClient() {
  const state = fakeClient();
  state.tasks.push(
    { id: "T-update", title: "Acme", status: "proposed", tags: ["proposal", "proposal:knowledge_update", "shared-library"], artifacts: [{ path: "proposal.json" }] },
    { id: "T-conflict", title: "Acme", status: "proposed", tags: ["proposal", "proposal:shared_conflict", "shared-library"], artifacts: [{ path: "proposal.json" }] },
  );
  const base = state.client.callTool;
  state.client.callTool = async (name, input) => {
    if (name === "orbit.task.artifact.get" && input.id === "T-update") { state.calls.push({ name, input }); return { id: input.id, path: input.path, encoding: "utf8", content: SHARED_UPDATE }; }
    if (name === "orbit.task.artifact.get" && input.id === "T-conflict") { state.calls.push({ name, input }); return { id: input.id, path: input.path, encoding: "utf8", content: SHARED_CONFLICT }; }
    return base(name, input);
  };
  return state;
}

test("a proposal carrying shareable says so, and a shared conflict is marked as one", async (t) => {
  const api = await serve(t, sharedClient());
  const update = await api.get("/api/approvals/detail?key=T-update");
  assert.equal(update.status, 200);
  assert.equal(update.body.shareable, true);
  assert.equal(update.body.conflict, false);
  assert.equal(update.body.sha256, createHash("sha256").update(SHARED_UPDATE).digest("hex"));
  const conflict = await api.get("/api/approvals/detail?key=T-conflict");
  assert.equal(conflict.body.conflict, true);
  assert.equal(conflict.body.shareable, false);
});

test("Use theirs and Keep mine call office.proposal-accept with the chosen resolution; a bad choice is refused first", async (t) => {
  const state = sharedClient();
  const api = await serve(t, state);
  const sha256 = createHash("sha256").update(SHARED_CONFLICT).digest("hex");
  assert.deepEqual(await api.post("/api/approvals/accept", { key: "T-conflict", sha256, choice: "theirs" }), { status: 200, body: { ok: true } });
  assert.deepEqual(state.calls.at(-1), { name: "office.proposal-accept", input: { id: "T-conflict", sha256, choice: "theirs" } });
  state.tasks.find((task) => task.id === "T-conflict").status = "proposed";
  assert.deepEqual(await api.post("/api/approvals/accept", { key: "T-conflict", sha256, choice: "mine" }), { status: 200, body: { ok: true } });
  assert.deepEqual(state.calls.at(-1), { name: "office.proposal-accept", input: { id: "T-conflict", sha256, choice: "mine" } });

  state.calls.length = 0;
  for (const choice of ["both", "", null, 1]) {
    assert.equal((await api.post("/api/approvals/accept", { key: "T-conflict", sha256, choice })).status, 400, String(choice));
  }
  assert.deepEqual(state.calls, []);

  state.failures["office.proposal-accept"] = "invalid_argument";
  const refused = await api.post("/api/approvals/accept", { key: "T-conflict", sha256 });
  assert.deepEqual(refused, { status: 409, body: { error: "accept_refused", reason: "invalid_argument" } });
  assert.deepEqual(state.calls.map(({ input }) => input), [{ id: "T-conflict", sha256 }], "no choice is sent when none was given");
});
