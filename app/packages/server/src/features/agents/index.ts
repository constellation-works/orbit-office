/** Read-only Agent work snapshot: the Orbit tasks agents are carrying, across the office workspace and the followed workspaces. */
import type { AgentWork, AgentWorkTask } from "@orbit-office/contracts";

import type { Feature } from "../../http/types.js";
import { createOrbitClient } from "../../orbit/client.js";
import type { OrbitToolClient } from "../../orbit/client.js";
import { logRouteFailure, safeOrbitCode } from "../../orbit/diagnostics.js";

/** Workspace name for the office workspace, which has no selector. */
export const OFFICE_WORKSPACE = "office";

const FIELDS = ["id", "title", "status", "type", "priority", "crew", "updated_at", "history"];
const ACTIVE_STATUSES = ["in-progress", "review", "backlog"];

interface TaskRow {
  id: string;
  title: string;
  status: string;
  type: string;
  priority: string;
  crew: string | null;
  updated_at: string;
  history: unknown;
}

class AgentsDataError extends Error {}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Unwrap an `office.*` exec envelope (`{ok:true,output}`), or a bare output. */
function officeOutput(value: unknown): Record<string, unknown> {
  if (!record(value)) throw new AgentsDataError("invalid office status");
  if (value.ok === false) throw new AgentsDataError("office status refused");
  const data = value.ok === true ? value.output : value;
  if (!record(data)) throw new AgentsDataError("invalid office status");
  return data;
}

/** The followed selectors from `office.status`; an uninitialized Library follows none. */
function followedWorkspaces(value: unknown): string[] {
  const data = officeOutput(value);
  if (data.brief === null || data.brief === undefined) return [];
  if (!record(data.brief) || !Array.isArray(data.brief.task_workspaces) ||
      !data.brief.task_workspaces.every((item: unknown) => typeof item === "string")) {
    throw new AgentsDataError("invalid office status");
  }
  return [...data.brief.task_workspaces];
}

function readList(value: unknown): TaskRow[] {
  if (!record(value) || !Array.isArray(value.tasks)) throw new AgentsDataError("invalid task listing");
  return value.tasks.map((row: unknown): TaskRow => {
    if (!record(row) || typeof row.id !== "string" || typeof row.title !== "string" ||
        typeof row.status !== "string" || typeof row.updated_at !== "string" ||
        !Number.isFinite(Date.parse(row.updated_at))) {
      throw new AgentsDataError("invalid task row");
    }
    return {
      id: row.id,
      title: row.title,
      status: row.status,
      type: typeof row.type === "string" ? row.type : "",
      priority: typeof row.priority === "string" ? row.priority : "",
      crew: typeof row.crew === "string" ? row.crew : null,
      updated_at: row.updated_at,
      history: row.history,
    };
  });
}

function completedAt(task: TaskRow): Date | undefined {
  if (!Array.isArray(task.history)) return undefined;
  const dates = task.history
    .filter((entry: unknown) => record(entry) && entry.to_status === "done")
    .map((entry: Record<string, unknown>) => typeof entry.at === "string" ? Date.parse(entry.at) : NaN);
  if (dates.length === 0 || dates.some((date) => !Number.isFinite(date))) return undefined;
  return new Date(Math.max(...dates));
}

function toTask(row: TaskRow, workspace: string): AgentWorkTask {
  return {
    id: row.id, title: row.title, workspace,
    type: row.type, priority: row.priority, crew: row.crew, updated_at: row.updated_at,
  };
}

const byNewest = (a: AgentWorkTask, b: AgentWorkTask): number =>
  Date.parse(b.updated_at) - Date.parse(a.updated_at) || a.id.localeCompare(b.id);

export interface AgentsOptions {
  client: Pick<OrbitToolClient, "pollTool" | "callTool">;
  now?: () => Date;
}

/** Build the same HTTP feature used by the router, with an injectable Orbit port. */
export function createAgentsFeature({ client, now = () => new Date() }: AgentsOptions): Feature {
  return {
    name: "agents",
    status: "built",
    routes: [{
      method: "GET",
      path: "/api/agents",
      async handle(_request, response) {
        try {
          const followed = followedWorkspaces(await client.callTool("office.status", {}));
          const selectors: Array<string | null> = [null, ...followed];
          const current = now();
          const start = new Date(current.getFullYear(), current.getMonth(), current.getDate());
          const snapshot: AgentWork = {
            working_now: [], in_review: [], queued: [], done_today: [],
            workspaces: selectors.map((selector) => selector ?? OFFICE_WORKSPACE),
            unavailable: [], updated_at: current.toISOString(),
          };
          // Each workspace is read on its own: one failure is reported by
          // name and never fails the whole snapshot (the brief's
          // `workspace_unavailable` pattern).
          await Promise.all(selectors.map(async (selector) => {
            const workspace = selector ?? OFFICE_WORKSPACE;
            const extra = selector === null ? {} : { workspace: selector };
            try {
              const [active, done] = await Promise.all([
                client.pollTool("orbit.task.list", { status: ACTIVE_STATUSES, fields: FIELDS, limit: 200, ...extra }).then(readList),
                client.pollTool("orbit.task.list", { status: "done", fields: FIELDS, limit: 200, ...extra }).then(readList),
              ]);
              for (const row of active) {
                if (row.status === "in-progress") snapshot.working_now.push(toTask(row, workspace));
                else if (row.status === "review") snapshot.in_review.push(toTask(row, workspace));
                else if (row.status === "backlog") snapshot.queued.push(toTask(row, workspace));
              }
              for (const row of done) {
                if (row.status !== "done") continue;
                const at = completedAt(row);
                if (at !== undefined && at >= start && at <= current) snapshot.done_today.push(toTask(row, workspace));
              }
            } catch {
              snapshot.unavailable.push(workspace);
            }
          }));
          snapshot.working_now.sort(byNewest);
          snapshot.in_review.sort(byNewest);
          snapshot.queued.sort(byNewest);
          snapshot.done_today.sort(byNewest);
          snapshot.unavailable.sort((a, b) => snapshot.workspaces.indexOf(a) - snapshot.workspaces.indexOf(b));
          response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          response.end(JSON.stringify(snapshot));
        } catch (error) {
          logRouteFailure("/api/agents", error instanceof AgentsDataError ? "office.status" : "orbit.task.list", error);
          const orbitCode = error instanceof AgentsDataError ? undefined : safeOrbitCode(error);
          response.writeHead(error instanceof AgentsDataError ? 502 : 503, {
            "content-type": "application/json", "cache-control": "no-store",
          });
          response.end(JSON.stringify({
            error: error instanceof AgentsDataError ? "invalid_agents_data" : "agents_unavailable",
            ...(orbitCode !== undefined ? { code: orbitCode } : {}),
          }));
        }
      },
    }],
  };
}

// A shared client keeps identical browser polls single-flight across requests.
export const feature: Feature = createAgentsFeature({ client: createOrbitClient() });
