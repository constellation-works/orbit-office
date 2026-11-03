import assert from "node:assert/strict";
import { test } from "node:test";

import { readModels } from "../dist/features/settings/models.js";

const NOW = () => new Date("2026-11-02T09:00:00Z");
const CLI = () => ({ cli: "Muse Code", connected: true });

function client(status) {
  return { callTool: async (tool) => { assert.equal(tool, "office.status"); return { ok: true, output: status }; } };
}

function hostedRows(view) {
  return view.jobs.slice(0, 3).map(({ model, where }) => ({ model, where }));
}

test("readModels shows the default Meta route and Muse models when no hosted block is reported", async () => {
  const view = await readModels(client({ secrets: { meta_api_key: "set" }, open_weights: null }), CLI, NOW);
  assert.deepEqual(hostedRows(view), [
    { model: "muse-voice-transcribe-1.0", where: "Meta Model API (api.meta.ai)" },
    { model: "muse-spark-1.3", where: "Meta Model API (api.meta.ai)" },
    { model: "muse-wonder", where: "Meta Model API (api.meta.ai)" },
  ]);
  assert.deepEqual(view.jobs[0].check, { state: "passed", label: "API key set" });
  assert.equal(view.jobs[4].model, "Muse Code crews");
  assert.equal(view.checked_at, "2026-11-02T09:00:00.000Z");
});

test("readModels names the configured hosted endpoint and models", async () => {
  const view = await readModels(client({
    secrets: { meta_api_key: "unset" },
    hosted: { endpoint: "https://api.example-llm.com/v1", standard: "acme-chat-large", careful: "acme-reasoner", transcribe: "acme-whisper" },
    open_weights: null,
  }), CLI, NOW);
  assert.deepEqual(hostedRows(view), [
    { model: "acme-whisper", where: "Hosted model API (api.example-llm.com)" },
    { model: "acme-chat-large", where: "Hosted model API (api.example-llm.com)" },
    { model: "acme-reasoner", where: "Hosted model API (api.example-llm.com)" },
  ]);
  assert.deepEqual(view.jobs[1].check, { state: "attention", label: "API key missing" });
  assert.deepEqual(view.jobs[3], { job: "Private sources", model: "Open-weights model", where: "Your endpoint (open weights)", check: { state: "off", label: "Not set up" } });
});
