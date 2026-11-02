/**
 * Settings › Models: every job the office runs, the Meta model it calls,
 * where it runs, and what the app can check now. The table mirrors
 * docs/design/office/specs/models-and-privacy.md; the checks are live:
 * the Meta Model API key's presence, the private route's last check, and
 * whether Orbit MCP is registered with the agent CLI. Never a secret value.
 */
import type { AgentCliStatus, ModelJob, ModelsView } from "@orbit-office/contracts";

import type { OrbitToolClient } from "../../orbit/client.js";
import { officeOutput } from "../../orbit/tasks.js";

type Check = ModelJob["check"];

interface Job {
  job: string;
  model: string;
  where: string;
  route: "hosted" | "private" | "crews";
}

/** models-and-privacy.md, "Use / Model / Where it runs". */
const JOBS: readonly Job[] = [
  { job: "Transcribe meetings", model: "Muse Voice Transcribe", where: "Meta Model API (api.meta.ai)", route: "hosted" },
  { job: "Extract decisions and to-dos", model: "muse-spark-1.3", where: "Meta Model API (api.meta.ai)", route: "hosted" },
  { job: "Careful work: consolidation, reviews", model: "muse-wonder", where: "Meta Model API (api.meta.ai)", route: "hosted" },
  { job: "Private sources", model: "Muse Spark Open", where: "Your endpoint (open weights)", route: "private" },
  { job: "Agent to-dos in Orbit", model: "Muse Code crews", where: "Your machine, through Orbit", route: "crews" },
];

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hostedCheck(status: Record<string, unknown>): Check {
  const secrets = status.secrets;
  const key = record(secrets) ? secrets.meta_api_key : undefined;
  return key === "set" ? { state: "passed", label: "API key set" } : { state: "attention", label: "API key missing" };
}

function privateCheck(status: Record<string, unknown>): { check: Check; where: string; model: string } {
  const route = status.open_weights;
  if (!record(route)) return { check: { state: "off", label: "Not set up" }, where: "Your endpoint (open weights)", model: "Muse Spark Open" };
  const label = typeof route.label === "string" && route.label !== "" ? route.label : "Your endpoint (open weights)";
  const model = typeof route.model === "string" && route.model !== "" ? route.model : "Muse Spark Open";
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
  const priv = privateCheck(status);
  const cli = agentCli();
  const jobs = JOBS.map((job): ModelJob => {
    switch (job.route) {
      case "hosted": return { job: job.job, model: job.model, where: job.where, check: hosted };
      case "private": return { job: job.job, model: priv.model, where: priv.where, check: priv.check };
      case "crews": return {
        job: job.job, model: job.model, where: `Your machine, through Orbit and ${cli.cli}`,
        check: cli.connected ? { state: "passed", label: "Connected" } : { state: "attention", label: "Not connected" },
      };
    }
  });
  return { jobs, checked_at: now().toISOString() };
}
