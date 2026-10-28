/** Read-only Today snapshot. Uploads and approval writes (features/approvals) have their own features. */
import type { SetupStatus, Today, TodayItem, TodayNeed, TodayNeedGroup } from "@orbit-office/contracts";

import type { Feature } from "../../http/types.js";
import { createOrbitClient, OrbitExitError } from "../../orbit/client.js";
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

/** The one card a shared-library sync's proposals share. */
export const SHARED_LIBRARY_GROUP: TodayNeedGroup = { key: "shared-library", title: "Changes from the shared library" };

/**
 * Onboarding proposals share one card, and so do the shared library's
 * incoming updates and conflicts. Bootstrap proposals are grouped by their
 * `subject:<name>` tag within a phase, or by the phase without one.
 */
function needGroup(task: Task, kind: TodayNeed["kind"]): TodayNeedGroup | null {
  if (kind !== "proposal") return null;
  if (task.tags.includes("shared-library")) return SHARED_LIBRARY_GROUP;
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

class SetupRefusal extends Error {
  constructor(readonly code: string) { super(code); }
}

function setupOutput(value: unknown): Record<string, unknown> {
  if (!record(value)) throw new Error("invalid response");
  if (value.ok === false) throw new SetupRefusal(record(value.error) && typeof value.error.code === "string" ? value.error.code : "unknown");
  const data = value.ok === true ? value.output : value;
  if (!record(data)) throw new Error("invalid response");
  return data;
}

function setupFailure(error: unknown): SetupStatus {
  const detail = error instanceof SetupRefusal ? error.code : error instanceof OrbitExitError ? error.stderr.toLowerCase() : "";
  if (/plugin.*(not enabled|disabled|not found)|unknown tool.*office|tool not found: office\.|tool.*office.*not found/.test(detail)) {
    return { state: "plugin_disabled", reason: "The Office plugin is not enabled for this workspace.", fix: "Run orbit-office init in your workspace." };
  }
  if (/workspace.*(not registered|unregistered|unknown|not found)|unknown workspace|no workspace|no workspaces registered/.test(detail)) {
    return { state: "workspace_unregistered", reason: "This workspace is not registered with Orbit.", fix: "Run orbit-office init in your workspace." };
  }
  if (detail === "git_missing") return { state: "unavailable", reason: "Git is not available to create the Library.", fix: "Install Git and run orbit-office init." };
  if (/^library_git_/.test(detail)) return { state: "unavailable", reason: "Git could not prepare the Library.", fix: "Check Git in your workspace and run orbit-office init." };
  if (/^index_/.test(detail)) return { state: "unavailable", reason: "Office could not prepare the Library index.", fix: "Run orbit-office init in your workspace." };
  if (/permission|access_denied|write_root|io_error|unsafe_path|library_dir_outside/.test(detail)) return { state: "unavailable", reason: "Office could not write the Library.", fix: "Check the workspace permissions and run orbit-office init." };
  return { state: "unavailable", reason: "Office could not set up the Library.", fix: "Run orbit-office init in your workspace, then try again." };
}

export interface TodayOptions {
  client: Pick<OrbitToolClient, "pollTool" | "callTool">;
  now?: () => Date;
}

/** Build the same HTTP feature used by the router, with an injectable Orbit port. */
export function createTodayFeature({ client, now = () => new Date() }: TodayOptions): Feature {
  return {
    name: "today",
    status: "built",
    routes: [
      {
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
            if (kind !== undefined) {
              const need: TodayNeed = { ...item(task), kind, group: needGroup(task, kind) };
              if (need.group === SHARED_LIBRARY_GROUP) need.shared = task.tags.includes("proposal:shared_conflict") ? "conflict" : "incoming";
              snapshot.needs_you.push(need);
            }
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
    },
      { method: "GET", path: "/api/setup", async handle(_request, response) {
        let status: SetupStatus;
        try {
          const data = setupOutput(await client.callTool("office.status", {}));
          if (!record(data.library) || typeof data.library.initialized !== "boolean") throw new Error("invalid status");
          status = { state: data.library.initialized ? "ready" : "missing" };
        } catch (error) { status = setupFailure(error); }
        response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
        response.end(JSON.stringify(status));
      } },
      { method: "POST", path: "/api/setup", async handle(_request, response) {
        try {
          const data = setupOutput(await client.callTool("office.init", {}));
          if (typeof data.already_initialized !== "boolean" || !Array.isArray(data.created) ||
              !data.created.every((path: unknown) => typeof path === "string")) throw new Error("invalid init result");
          response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          response.end(JSON.stringify({ created: data.created, already_initialized: data.already_initialized }));
        } catch (error) {
          response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
          response.end(JSON.stringify(setupFailure(error)));
        }
      } },
    ],
  };
}

// A shared client keeps identical browser polls single-flight across requests.
export const feature: Feature = createTodayFeature({ client: createOrbitClient() });
