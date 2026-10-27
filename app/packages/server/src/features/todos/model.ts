/**
 * Turn Orbit task rows into the To-dos columns.
 *
 * Labels are plain words. Task ids, tag names, artifact paths, crews,
 * runs and raw statuses never become visible text.
 *
 * `done` is reachable only from `review`, so closing a to-do walks the
 * legal lifecycle instead of asking Orbit for a jump it will refuse.
 */
import type { TodoItem, TodoList } from "@orbit-office/contracts";

/** Written on the move to done when the to-do has no execution summary yet. */
export const CLOSED_FROM_TODOS = "Closed from To-dos.";

export interface CloseStep {
  status: "backlog" | "in-progress" | "review" | "done";
  execution_summary?: string;
}

const ACRONYMS = new Set(["api", "pdf", "qbr", "url"]);
const SOURCE_ID = /^src-\d{8}-(.+)$/;
const ARTIFACT_ID = /^art-\d{8}-(.+)$/;
const TASK_ID = /^[A-Za-z0-9._:-]{1,128}$/;

interface ParsedTask {
  id: string;
  title: string;
  status: string;
  tags: string[];
  terminal: boolean;
  createdAt: string | null;
  summary: string;
  artifacts: string[];
}

/** True for an Orbit task id the close route is willing to pass through. */
export function isTaskId(id: string): boolean {
  return TASK_ID.test(id);
}

/**
 * Updates that move `status` to done, in order.
 * An empty list means the to-do is already done. `null` means this
 * status cannot be closed.
 */
export function closePlan(status: string, hasSummary: boolean): CloseStep[] | null {
  const done: CloseStep = hasSummary
    ? { status: "done" }
    : { status: "done", execution_summary: CLOSED_FROM_TODOS };
  switch (status) {
    case "done":
      return [];
    case "review":
      return [done];
    case "in-progress":
      return [{ status: "review" }, done];
    case "backlog":
      return [{ status: "in-progress" }, { status: "review" }, done];
    case "proposed":
    case "someday":
    case "blocked":
      return [{ status: "backlog" }, { status: "in-progress" }, { status: "review" }, done];
    default:
      return null;
  }
}

/** Split open to-dos into the You and Agents columns. */
export function todoColumns(
  youRows: readonly unknown[],
  agentRows: readonly unknown[],
  more: boolean,
): TodoList {
  const tasks: ParsedTask[] = [];
  const seen = new Set<string>();
  for (const row of [...youRows, ...agentRows]) {
    const task = parseTask(row);
    if (task === null || seen.has(task.id)) continue;
    seen.add(task.id);
    tasks.push(task);
  }
  const you = tasks
    .filter((task) => isOpen(task) && task.tags.includes("owner:you"))
    .sort(byCreated);
  const agents = tasks
    .filter((task) => isOpen(task) && !task.tags.includes("owner:you") && task.tags.includes("owner:agent"))
    .sort(byCreated);
  return {
    you: you.map((task) => toItem(task, true)),
    agents: agents.map((task) => toItem(task, false)),
    more,
  };
}

/** Fields the close path needs from an `orbit.task.show` payload. */
export interface ShownTask {
  id: string;
  status: string;
  tags: readonly string[];
  summary: string;
}

/** Read the fields the close path needs from an `orbit.task.show` payload. */
export function parseShownTask(row: unknown): ShownTask | null {
  const task = parseTask(row);
  if (task === null) return null;
  return { id: task.id, status: task.status, tags: task.tags, summary: task.summary };
}

function parseTask(row: unknown): ParsedTask | null {
  if (row === null || typeof row !== "object") return null;
  const record = row as Record<string, unknown>;
  if (typeof record.id !== "string" || !isTaskId(record.id)) return null;
  const title = typeof record.title === "string" ? record.title.trim() : "";
  const status = typeof record.status === "string" ? record.status : "";
  const settled = status === "done" || status === "archived" || status === "rejected";
  return {
    id: record.id,
    title: title === "" ? "Untitled to-do" : title,
    status,
    tags: stringList(record.tags),
    terminal: record.terminal === true || settled,
    createdAt: typeof record.created_at === "string" ? record.created_at : null,
    summary: typeof record.execution_summary === "string" ? record.execution_summary : "",
    artifacts: artifactPaths(record.artifacts),
  };
}

function stringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.filter((item): item is string => typeof item === "string");
}

function artifactPaths(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  const paths: string[] = [];
  for (const item of value) {
    if (typeof item === "string") {
      paths.push(item);
      continue;
    }
    if (item !== null && typeof item === "object" && "path" in item && typeof item.path === "string") {
      paths.push(item.path);
    }
  }
  return paths;
}

function isOpen(task: ParsedTask): boolean {
  return !task.terminal;
}

function byCreated(a: ParsedTask, b: ParsedTask): number {
  if (a.createdAt === null && b.createdAt === null) return 0;
  if (a.createdAt === null) return 1;
  if (b.createdAt === null) return -1;
  if (a.createdAt < b.createdAt) return -1;
  if (a.createdAt > b.createdAt) return 1;
  return 0;
}

function toItem(task: ParsedTask, closable: boolean): TodoItem {
  return {
    id: task.id,
    title: task.title,
    source: sourceLabel(task),
    knowledge: knowledgeLabels(task),
    closable,
  };
}

function sourceLabel(task: ParsedTask): string | null {
  for (const tag of task.tags) {
    if (!tag.startsWith("source:")) continue;
    const id = tag.slice("source:".length);
    return labelFromSourceId(id) ?? "A recorded source";
  }
  for (const path of task.artifacts) {
    if (!path.includes("/sources/")) continue;
    const id = /(?:^|\/)(src-\d{8}-[^/]+)(?:\/|$)/.exec(path)?.[1];
    if (id === undefined) continue;
    const label = labelFromSourceId(id);
    if (label !== null) return label;
  }
  return null;
}

function labelFromSourceId(id: string): string | null {
  const slug = SOURCE_ID.exec(id)?.[1];
  if (slug === undefined) return null;
  const label = humanizeSlug(slug);
  if (label === "" || label.includes(id)) return null;
  return label;
}

function knowledgeLabels(task: ParsedTask): string[] {
  const labels: string[] = [];
  const seen = new Set<string>();
  for (const path of task.artifacts) {
    const label = knowledgeLabel(path);
    if (label === null || seen.has(label)) continue;
    seen.add(label);
    labels.push(label);
  }
  return labels;
}

function knowledgeLabel(path: string): string | null {
  if (!path.startsWith("context/") || path.includes("/sources/")) return null;
  const artifactId = /(?:^|\/)(art-\d{8}-[^/]+)(?:\/|$)/.exec(path)?.[1];
  if (artifactId !== undefined) {
    const slug = ARTIFACT_ID.exec(artifactId)?.[1];
    if (slug !== undefined) {
      const label = humanizeSlug(slug);
      if (label !== "" && !label.includes(artifactId)) return label;
    }
  }
  const file = path.split("/").pop() ?? "";
  const stem = file.replace(/\.[^.]+$/, "");
  if (stem === "" || stem === "source" || stem === "artifact") return null;
  const label = humanizeSlug(stem);
  if (label === "" || label.includes(path)) return null;
  return label;
}

function humanizeSlug(slug: string): string {
  return slug
    .split(/[-_]+/)
    .filter((word) => word !== "")
    .map((word) => {
      const lower = word.toLowerCase();
      if (ACRONYMS.has(lower)) return lower.toUpperCase();
      return lower.slice(0, 1).toUpperCase() + lower.slice(1);
    })
    .join(" ");
}
