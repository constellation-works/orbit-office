import assert from "node:assert/strict";
import { test } from "node:test";
import { createReviewFeature } from "../dist/features/review/index.js";
import { OrbitNotPermittedError } from "../dist/orbit/client.js";

function fixture(count = 2) {
  const calls = [];
  const rows = Array.from({ length: count }, (_, i) => ({ id: `art-202610${String(i + 1).padStart(2, "0")}-review`, i }));
  return {
    calls,
    async callTool(name, input) {
      calls.push({ name, input });
      if (name === "office.search") return { ok: true, output: { hits: input.series === "review-quarterly" ? rows : [] } };
      const row = rows.find((item) => item.id === input.id);
      return { ok: true, output: { artifact: { id: row.id, kind: "review", series: "review-quarterly", title: `Review ${row.i}`, approved: { at: new Date(Date.UTC(2026, 0, row.i + 1)).toISOString() } }, body: "Outcome [src-20261001-call#L1]\nDecision [ORB-123]", truncated: false } };
    },
    async mintAutoTask(name) { calls.push({ name }); throw new OrbitNotPermittedError("orbit.auto_task.mint", name); },
  };
}
function capture() { return { writeHead(status) { this.status = status; }, end(body) { this.body = JSON.parse(body); } }; }
test("review routes search both series, read bodies, sort and explain refused regeneration", async () => {
  const client = fixture(); const feature = createReviewFeature(client);
  const get = feature.routes.find((route) => route.method === "GET"); const result = capture(); await get.handle({}, result);
  assert.equal(result.status, 200); assert.deepEqual(result.body.reviews.map((r) => r.title), ["Review 1", "Review 0"]);
  assert.match(result.body.reviews[0].body, /Decision/);
  assert.deepEqual(client.calls.slice(0, 2).map((c) => c.input), ["review-quarterly", "review-yearly"].map((series) => ({ query: series, kind: "artifact", series, limit: 100 })));
  assert.equal(client.calls.filter((c) => c.name === "office.artifact-show").length, 2);
  const post = feature.routes.find((route) => route.method === "POST"); const refusal = capture(); await post.handle({}, refusal);
  assert.equal(refusal.status, 403); assert.equal(refusal.body.error, "refused"); assert.equal(client.calls.at(-1).name, "office-review");
});
test("a full result is bounded and failures are not reported as an empty list", async () => {
  const client = fixture(100); const route = createReviewFeature(client).routes[0]; const result = capture(); await route.handle({}, result);
  assert.equal(result.body.reviews.length, 100); assert.equal(result.body.latest_100, true);
  client.callTool = async () => ({ ok: false }); const failure = capture(); await route.handle({}, failure); assert.equal(failure.status, 502);
});

test("regenerate invokes the real Orbit client against a fake executable", async (t) => {
  const { installFakeOrbit } = await import("./fake-orbit.mjs");
  const { createOrbitClient } = await import("../dist/orbit/client.js");
  const { readFile, rm } = await import("node:fs/promises");
  const { join } = await import("node:path");
  const fake = await installFakeOrbit();
  t.after(() => rm(fake.root, { recursive: true, force: true }));
  const client = createOrbitClient({ env: fake.env({ HOME: fake.root, ORBIT_OFFICE_WORKSPACE: fake.root }) });
  const result = capture(); await createReviewFeature(client).routes[1].handle({}, result);
  assert.equal(result.status, 200);
  const calls = (await readFile(join(fake.state, "invocations.log"), "utf8")).trim().split("\n").map(JSON.parse);
  assert.equal(calls[0][2], "orbit.auto_task.mint");
  assert.deepEqual(JSON.parse(calls[0][calls[0].indexOf("--input") + 1]), { name: "office-review", acknowledge_unconditional: true, workspace: fake.root });
});
