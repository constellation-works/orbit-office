/**
 * Settings › Models: every job the office runs, the model it calls, where
 * it runs, and what the app can check now. The hosted rows come from
 * `office.status`'s `hosted` block (the configured `models.endpoint` and
 * models, Meta's Model API and the Muse models by default); the table
 * mirrors docs/design/office/specs/models-and-privacy.md. The checks are
 * live: the hosted API key's presence, the private route's last check,
 * and whether Orbit MCP is registered with the agent CLI. Never a secret
 * value.
 */
import type { AgentCliStatus, ModelJob, ModelsView } from "@orbit-office/contracts";

import type { OrbitToolClient } from "../../orbit/client.js";
import { officeOutput } from "../../orbit/tasks.js";

type Check = ModelJob["check"];

type HostedModel = "transcribe" | "standard" | "careful";

interface Job {
  job: string;
  route: "hosted" | "private" | "crews";
  /** The hosted model this job names; hosted rows only. */
  hosted?: HostedModel;
  /** Shown when nothing configured overrides it. */
  model: string;
}

/** models-and-privacy.md, "Use / Model / Where it runs". */
const JOBS: readonly Job[] = [
  { job: "Transcribe meetings", route: "hosted", hosted: "transcribe", model: "muse-voice-transcribe-1.0" },
  { job: "Extract decisions and to-dos", route: "hosted", hosted: "standard", model: "muse-spark-1.3" },
  { job: "Careful work: consolidation, reviews", route: "hosted", hosted: "careful", model: "muse-wonder" },
  { job: "Private sources", route: "private", model: "Open-weights model" },
  { job: "Agent to-dos in Orbit", route: "crews", model: "crews" },
];

/** The hosted endpoint while `models.endpoint` is unset. */
const DEFAULT_HOSTED_ENDPOINT = "https://api.meta.ai/v1";

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function nonEmpty(value: unknown): string | undefined {
  return typeof value === "string" && value !== "" ? value : undefined;
}

/** "Meta Model API (api.meta.ai)" for Meta's endpoint, else its host. */
function hostedWhere(endpoint: string): string {
  let host: string;
  try {
    host = new URL(endpoint).hostname.replace(/^\[|\]$/g, "");
  } catch {
    return "Hosted model API";
  }
  return host === "api.meta.ai" ? "Meta Model API (api.meta.ai)" : `Hosted model API (${host})`;
}

function hostedRoute(status: Record<string, unknown>): { where: string; models: Record<HostedModel, string | undefined> } {
  const hosted = record(status.hosted) ? status.hosted : {};
  return {
    where: hostedWhere(nonEmpty(hosted.endpoint) ?? DEFAULT_HOSTED_ENDPOINT),
    models: { transcribe: nonEmpty(hosted.transcribe), standard: nonEmpty(hosted.standard), careful: nonEmpty(hosted.careful) },
  };
}

function hostedCheck(status: Record<string, unknown>): Check {
  const secrets = status.secrets;
  const key = record(secrets) ? secrets.meta_api_key : undefined;
  return key === "set" ? { state: "passed", label: "API key set" } : { state: "attention", label: "API key missing" };
}

function privateCheck(status: Record<string, unknown>): { check: Check; where: string; model: string } {
  const route = status.open_weights;
  const fallback = JOBS.find((job) => job.route === "private")?.model ?? "";
  if (!record(route)) return { check: { state: "off", label: "Not set up" }, where: "Your endpoint (open weights)", model: fallback };
  const label = nonEmpty(route.label) ?? "Your endpoint (open weights)";
  const model = nonEmpty(route.model) ?? fallback;
  const verified = typeof route.verified_at === "string" && Number.isFinite(Date.parse(route.verified_at));
  return {
    check: verified ? { state: "passed", label: "Route checked" } : { state: "attention", label: "Not checked yet" },
    where: label, model,
  };
}

export async function readModels(
  client: Pick<OrbitToolClient, "callTool">,
  agentCli: () => AgentCliStatus,
  now: () => Date = () => new Date(),
): Promise<ModelsView> {
  const status = officeOutput(await client.callTool("office.status", {}));
  const hosted = hostedCheck(status);
  const route = hostedRoute(status);
  const priv = privateCheck(status);
  const cli = agentCli();
  const jobs = JOBS.map((job): ModelJob => {
    switch (job.route) {
      case "hosted": return {
        job: job.job, model: (job.hosted === undefined ? undefined : route.models[job.hosted]) ?? job.model,
        where: route.where, check: hosted,
      };
      case "private": return { job: job.job, model: priv.model, where: priv.where, check: priv.check };
      case "crews": return {
        job: job.job, model: `${cli.cli} ${job.model}`, where: `Your machine, through Orbit and ${cli.cli}`,
        check: cli.connected ? { state: "passed", label: "Connected" } : { state: "attention", label: "Not connected" },
      };
    }
  });
  return { jobs, checked_at: now().toISOString() };
}
