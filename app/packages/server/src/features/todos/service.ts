/**
 * Read, close and schedule to-dos through the Orbit client.
 * Listing is uncached (`callTool`): a close or a schedule must see what it
 * just wrote.
 */
import type { TodoList } from "@orbit-office/contracts";

import type { OrbitToolClient } from "../../orbit/client.js";
import { closePlan, isInstant, isSchedulable, isTaskId, parseShownTask, todoColumns } from "./model.js";

export type TodoErrorCode =
  | "invalid_input"
  | "not_found"
  | "not_yours"
  | "not_closable"
  | "not_schedulable"
  | "unavailable";

export class TodoCloseError extends Error {
  constructor(
    readonly status: number,
    readonly code: TodoErrorCode,
  ) {
    super(code);
    this.name = "TodoCloseError";
  }
}

const LIST_LIMIT = 200;
const LIST_FIELDS = ["id", "title", "status", "tags", "terminal", "artifacts", "created_at", "not_before"];

/** Open to-dos for the You and Agents columns. */
export async function loadTodos(client: OrbitToolClient): Promise<TodoList> {
  const [yours, agents] = await Promise.all([
    listTagged(client, "owner:you"),
    listTagged(client, "owner:agent"),
  ]);
  return todoColumns(yours.rows, agents.rows, yours.more || agents.more);
}

/**
 * Move one of the user's to-dos to done.
 * An agent to-do is refused and no update is sent.
 */
export async function closeTodo(client: OrbitToolClient, id: string): Promise<void> {
  if (!isTaskId(id)) throw new TodoCloseError(400, "invalid_input");
  const shown = await orbitCall(client, "orbit.task.show", {
    id,
    fields: ["id", "tags", "status", "terminal", "execution_summary"],
  });
  const task = parseShownTask(shown);
  if (task === null || task.id !== id) throw new TodoCloseError(404, "not_found");
  if (!task.tags.includes("owner:you")) throw new TodoCloseError(403, "not_yours");
  const plan = closePlan(task.status, task.summary.trim() !== "");
  if (plan === null) throw new TodoCloseError(409, "not_closable");
  for (const step of plan) {
    const input: Record<string, unknown> = { id, status: step.status };
    if (step.execution_summary !== undefined) input.execution_summary = step.execution_summary;
    await orbitCall(client, "orbit.task.update", input);
  }
}

/**
 * Set an open to-do's scheduled start, or clear it with null, through
 * `orbit.task.update` acting as the user. Yours or an agent's alike; a
 * to-do something has already started, or a task that is not a to-do, is
 * refused and no update is sent.
 */
export async function scheduleTodo(client: OrbitToolClient, id: string, notBefore: string | null): Promise<string | null> {
  if (!isTaskId(id)) throw new TodoCloseError(400, "invalid_input");
  if (notBefore !== null && !isInstant(notBefore)) throw new TodoCloseError(400, "invalid_input");
  const shown = await orbitCall(client, "orbit.task.show", {
    id,
    fields: ["id", "tags", "status", "terminal"],
  });
  const task = parseShownTask(shown);
  if (task === null || task.id !== id) throw new TodoCloseError(404, "not_found");
  if (!task.tags.includes("owner:you") && !task.tags.includes("owner:agent")) throw new TodoCloseError(404, "not_found");
  if (task.terminal || !isSchedulable(task.status)) throw new TodoCloseError(409, "not_schedulable");
  // Orbit clears the field on an empty string; null would leave it unchanged.
  await orbitCall(client, "orbit.task.update", { id, not_before: notBefore ?? "" });
  return notBefore;
}

async function listTagged(
  client: OrbitToolClient,
  tag: string,
): Promise<{ rows: unknown[]; more: boolean }> {
  const result = await client.callTool("orbit.task.list", {
    tag,
    fields: LIST_FIELDS,
    limit: LIST_LIMIT,
  });
  return rowsFromList(result);
}

function rowsFromList(result: unknown): { rows: unknown[]; more: boolean } {
  if (Array.isArray(result)) return { rows: result, more: false };
  if (result !== null && typeof result === "object" && "tasks" in result) {
    const record = result as { tasks?: unknown; truncated?: unknown };
    if (!Array.isArray(record.tasks)) {
      throw new TodoCloseError(502, "unavailable");
    }
    return { rows: record.tasks, more: record.truncated === true };
  }
  throw new TodoCloseError(502, "unavailable");
}

async function orbitCall(
  client: OrbitToolClient,
  name: string,
  input: Record<string, unknown>,
): Promise<unknown> {
  try {
    return await client.callTool(name, input);
  } catch {
    throw new TodoCloseError(502, "unavailable");
  }
}
