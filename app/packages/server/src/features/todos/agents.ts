/**
 * `GET /api/todos/agents`: agent to-dos filed from meetings into the
 * target workspace (`meetings.agent_workspace`, ORB-21336), with task id,
 * crew, state and PR, plus to-dos held back as yours because their matter
 * is contested (`owner:you` + `contested`, extract/filing.rs).
 */
import type { AgentTodo, AgentTodos, AgentTodoState } from "@orbit-office/contracts";
import type { ServerResponse } from "node:http";

import type { OrbitToolClient } from "../../orbit/client.js";
import { logRouteFailure } from "../../orbit/diagnostics.js";
import { sourceTag, taskPr, workspaceSettings } from "../../orbit/tasks.js";

const FIELDS = ["id", "title", "status", "tags", "crew", "created_at"];
const LIMIT = 200;
/** Delivery records read per snapshot; later rows show no PR. */
const PR_LOOKUPS = 24;

interface Row { id: string; title: string; status: string; tags: string[]; crew: string | null; created: number }

function rowsOf(listing: unknown): Row[] {
  const tasks = typeof listing === "object" && listing !== null && "tasks" in listing && Array.isArray(listing.tasks)
    ? listing.tasks as unknown[] : Array.isArray(listing) ? listing : [];
  return tasks.flatMap((value) => {
    if (typeof value !== "object" || value === null) return [];
    const task = value as Record<string, unknown>;
    if (typeof task.id !== "string" || typeof task.title !== "string" || typeof task.status !== "string") return [];
    const tags = Array.isArray(task.tags) ? task.tags.filter((tag): tag is string => typeof tag === "string") : [];
    const created = typeof task.created_at === "string" ? Date.parse(task.created_at) : NaN;
    return [{
      id: task.id, title: task.title.trim(), status: task.status, tags,
      crew: typeof task.crew === "string" && task.crew !== "" ? task.crew : null,
      created: Number.isFinite(created) ? created : 0,
    }];
  });
}

function state(status: string): AgentTodoState | null {
  switch (status) {
    case "proposed": case "backlog": case "someday": case "blocked": return "queued";
    case "in-progress": return "working";
    case "review": return "review";
    case "done": return "done";
    default: return null;
  }
}

/** Shown first: work in flight, then queued, held, finished. */
const ORDER: Record<AgentTodoState, number> = { working: 0, review: 1, queued: 2, held: 3, merged: 4, done: 5 };

export async function loadAgentTodos(client: Pick<OrbitToolClient, "callTool" | "pollTool">): Promise<AgentTodos> {
  const { agentWorkspace, followed } = workspaceSettings(await client.callTool("office.status", {}));
  const heldListing = await client.callTool("orbit.task.list", { tag: "contested", fields: FIELDS, limit: LIMIT });
  const todos: AgentTodo[] = rowsOf(heldListing)
    .filter((row) => row.tags.includes("owner:you") && state(row.status) !== null && row.status !== "done")
    .map((row) => ({ id: row.id, title: row.title, workspace: "office", state: "held", crew: null, pr: null, source: sourceTag(row.tags) }));
  let targetUnavailable = false;
  if (agentWorkspace !== null) {
    try {
      const listing = await client.callTool("orbit.task.list", { workspace: agentWorkspace, tag: "owner:agent", fields: FIELDS, limit: LIMIT });
      const filed = rowsOf(listing).filter((row) => sourceTag(row.tags) !== null && state(row.status) !== null)
        .sort((a, b) => b.created - a.created);
      let lookups = 0;
      for (const row of filed) {
        const current = state(row.status) as AgentTodoState;
        const pr = current !== "queued" && lookups++ < PR_LOOKUPS ? await taskPr(client, row.id, agentWorkspace) : null;
        todos.push({
          id: row.id, title: row.title, workspace: agentWorkspace,
          state: current === "done" && pr?.merged === true ? "merged" : current,
          crew: row.crew, pr, source: sourceTag(row.tags),
        });
      }
    } catch (error) {
      logRouteFailure("/api/todos/agents", "orbit.task.list", error);
      targetUnavailable = true;
    }
  }
  todos.sort((a, b) => ORDER[a.state] - ORDER[b.state]);
  return { target_workspace: agentWorkspace, followed, todos, target_unavailable: targetUnavailable };
}

export async function handleAgentTodos(client: Pick<OrbitToolClient, "callTool" | "pollTool">, response: ServerResponse): Promise<void> {
  try {
    const body = await loadAgentTodos(client);
    response.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
    response.end(JSON.stringify(body));
  } catch (error) {
    logRouteFailure("/api/todos/agents", "office.status", error);
    response.writeHead(503, { "content-type": "application/json", "cache-control": "no-store" });
    response.end(JSON.stringify({ error: "unavailable" }));
  }
}
