/** Read-only Today snapshot. Uploads and approval writes (features/approvals) have their own features. */
import type { Today, TodayItem, TodayNeed, TodayNeedGroup } from "@orbit-office/contracts";

import type { Feature } from "../../http/types.js";
import { createOrbitClient } from "../../orbit/client.js";
import type { OrbitToolClient } from "../../orbit/client.js";

interface Task {
  id: string;
  title: string;
  status: string;
  tags: string[];
  description: string;
  execution_summary: string | null;
  history: unknown;
}

class TodayDataError extends Error {}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** A malformed response is unavailable data, never an empty section. */
function readList(value: unknown): { tasks: Task[]; truncated: boolean } {
  if (!record(value) || !Array.isArray(value.tasks) || typeof value.truncated !== "boolean") {
    throw new TodayDataError("invalid task listing");
  }
  const tasks = value.tasks.map((row: unknown): Task => {
    if (!record(row) || typeof row.id !== "string" || typeof row.title !== "string" ||
        typeof row.status !== "string" || !Array.isArray(row.tags) ||
        !row.tags.every((tag: unknown) => typeof tag === "string") ||
        typeof row.description !== "string" ||
        !(row.execution_summary === null || typeof row.execution_summary === "string")) {
      throw new TodayDataError("invalid task row");
    }
    return {
      id: row.id, title: row.title, status: row.status, tags: row.tags as string[],
      description: row.description, execution_summary: row.execution_summary, history: row.history,
    };
  });
  return { tasks, truncated: value.truncated };
}

function item(task: Task): TodayItem {
  return { key: task.id, title: task.title, background: task.description };
}

function needKind(task: Task): TodayNeed["kind"] | undefined {
  // Gateway drafts and script outputs follow the proposal record path, so they are matched first.
  if (task.status === "proposed" && task.tags.includes("gateway-draft")) return "gateway_draft";
  if (task.status === "proposed" && task.tags.includes("script-output")) return "script_output";
  if (task.status === "proposed" && task.tags.includes("proposal")) return "proposal";
  if (task.status === "review") return "approval";
  if (task.tags.includes("owner:you")) return "todo";
  return undefined;
}

const BOOTSTRAP_PHASES: Record<string, string> = {
  docs: "Pages from your documents",
  kt: "Pages from your recorded walkthroughs",
  team: "Pages about your team",
  process: "Pages about your processes and policies",
};

/** A tag value in plain words: `ingest-service` becomes "Ingest service". */
function words(value: string): string {
  const text = value.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim();
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * Onboarding proposals share one card. Bootstrap proposals are grouped by
 * their `subject:<name>` tag within a phase, or by the phase without one.
 */
function needGroup(task: Task, kind: TodayNeed["kind"]): TodayNeedGroup | null {
  if (kind !== "proposal") return null;
  if (task.tags.includes("onboarding")) return { key: "onboarding", title: "Set up your Library" };
  const phase = task.tags.find((tag) => tag.startsWith("bootstrap:"))?.slice("bootstrap:".length);
  if (phase === undefined || phase === "") return null;
  const subject = task.tags.find((tag) => tag.startsWith("subject:"))?.slice("subject:".length).trim();
  if (subject !== undefined && subject !== "") {
    return { key: `bootstrap:${phase}:${subject.toLowerCase()}`, title: words(subject) };
  }
  return { key: `bootstrap:${phase}`, title: BOOTSTRAP_PHASES[phase] ?? "New Library pages" };
}

function completedAt(task: Task): Date | undefined {
  if (!Array.isArray(task.history)) return undefined;
  const dates = task.history
    .filter((entry: unknown) => record(entry) && entry.to_status === "done")
    .map((entry: Record<string, unknown>) => typeof entry.at === "string" ? Date.parse(entry.at) : NaN);
  if (dates.length === 0 || dates.some((date) => !Number.isFinite(date))) return undefined;
  return new Date(Math.max(...dates));
}

export interface TodayOptions {
  client: Pick<OrbitToolClient, "pollTool">;
  now?: () => Date;
}

/** Build the same HTTP feature used by the router, with an injectable Orbit port. */
export function createTodayFeature({ client, now = () => new Date() }: TodayOptions): Feature {
  return {
    name: "today",
    status: "built",
    routes: [{
      method: "GET",
      path: "/api/today",
      async handle(_request, response) {
        // The snapshot is all-or-nothing: failed reads cannot masquerade as no work.
        try {
          const fields = ["id", "title", "status", "tags", "description", "execution_summary", "history"];
          const [active, done] = await Promise.all([
            client.pollTool("orbit.task.list", {
              status: ["proposed", "backlog", "in-progress", "review", "blocked", "someday"], fields, limit: 200,
            }).then(readList),
            client.pollTool("orbit.task.list", { status: "done", fields, limit: 200 }).then(readList),
          ]);
          const current = now();
          const start = new Date(current.getFullYear(), current.getMonth(), current.getDate());
          const snapshot: Today = {
            needs_you: [], agents_working_on: [], done_today: [],
            truncated: active.truncated || done.truncated, completion_dates_unavailable: 0,
          };
          for (const task of active.tasks) {
            if (!["proposed", "backlog", "in-progress", "review", "blocked", "someday"].includes(task.status)) continue;
            const kind = needKind(task);
            if (kind !== undefined) snapshot.needs_you.push({ ...item(task), kind, group: needGroup(task, kind) });
            if (task.status === "in-progress" && task.tags.includes("owner:agent") && !task.tags.includes("owner:you")) {
              snapshot.agents_working_on.push(item(task));
            }
          }
          const completed = done.tasks.filter((task) => task.status === "done").map((task) => ({ task, at: completedAt(task) }));
          snapshot.completion_dates_unavailable = completed.filter(({ at }) => at === undefined).length;
          snapshot.done_today = completed
            .filter(({ at }) => at !== undefined && at >= start && at <= current)
            .sort((a, b) => (b.at?.getTime() ?? 0) - (a.at?.getTime() ?? 0))
            .map(({ task }) => ({ ...item(task), result: task.execution_summary?.trim() || null }));
          const order = { proposal: 0, gateway_draft: 1, script_output: 2, approval: 3, todo: 4 };
          snapshot.needs_you.sort((a, b) => order[a.kind] - order[b.kind] ||
            (a.group?.title ?? "").localeCompare(b.group?.title ?? "") || a.title.localeCompare(b.title));
          snapshot.agents_working_on.sort((a, b) => a.title.localeCompare(b.title));
          response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          response.end(JSON.stringify(snapshot));
        } catch (error) {
          response.writeHead(error instanceof TodayDataError ? 502 : 503, {
            "content-type": "application/json", "cache-control": "no-store",
          });
          response.end(JSON.stringify({ error: error instanceof TodayDataError ? "invalid_today_data" : "today_unavailable" }));
        }
      },
    }],
  };
}

// A shared client keeps identical browser polls single-flight across requests.
export const feature: Feature = createTodayFeature({ client: createOrbitClient() });
