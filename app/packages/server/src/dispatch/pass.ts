/**
 * The dispatch pass: every 30 seconds, list `backlog` tasks tagged
 * `owner:agent` and submit them by id with `orbit run ship <ids>`
 * (review-only). Single-flight: a pass never starts while the last one is
 * running. The office workspace never runs an auto drain, so `owner:you`
 * to-dos are never submitted, and `proposed` tasks are never submitted —
 * only `backlog` tasks carrying `owner:agent` (and not `owner:you`) are
 * selected (docs/design/office/specs/web-app.md).
 */
import { spawn } from "node:child_process";

import type { OrbitToolClient } from "../orbit/client.js";

/** How often the pass runs. */
export const DISPATCH_INTERVAL_MS = 30_000;

/** The task fields the pass needs from `orbit.task.list`. */
export interface DispatchTask {
  id: string;
  status: string;
  tags?: readonly string[];
}

/** Outcome of one pass. `skipped` is true when a previous pass was running. */
export interface DispatchPassResult {
  skipped: boolean;
  submitted: string[];
}

export interface DispatchPassDeps {
  /** List candidate tasks (already status/tag filtered server-side where possible). */
  listTasks: () => Promise<readonly DispatchTask[]>;
  /** Submit the selected ids, e.g. `orbit run ship <ids>`. */
  ship: (ids: readonly string[]) => Promise<void>;
  /** Pass interval for `start()`. Default {@link DISPATCH_INTERVAL_MS}. */
  intervalMs?: number;
  /** Called when a scheduled pass fails. A manual `runOnce` still rejects. */
  onError?: (error: unknown) => void;
}

export interface Dispatcher {
  /** Run one pass now; resolves `skipped` instead of overlapping a running pass. */
  runOnce: () => Promise<DispatchPassResult>;
  /** Start the 30-second interval. Idempotent. */
  start: () => void;
  /** Stop the interval. A pass already running finishes. */
  stop: () => void;
  readonly running: boolean;
}

/**
 * Select submittable ids: `backlog` only, tagged `owner:agent`, never
 * `owner:you`. Anything else (`proposed`, `owner:you`, untagged) is left alone.
 */
export function selectDispatchIds(tasks: readonly DispatchTask[]): string[] {
  return tasks
    .filter((task) => task.status === "backlog")
    .filter((task) => task.tags?.includes("owner:agent") === true)
    .filter((task) => task.tags?.includes("owner:you") !== true)
    .map((task) => task.id);
}

/** Extract the task rows from an `orbit.task.list` envelope (or a bare array). */
export function tasksFromListResult(result: unknown): DispatchTask[] {
  const rows: unknown = Array.isArray(result)
    ? result
    : (result as { tasks?: unknown } | null)?.tasks;
  if (!Array.isArray(rows)) return [];
  const tasks: DispatchTask[] = [];
  for (const row of rows) {
    if (row === null || typeof row !== "object") continue;
    const record = row as Record<string, unknown>;
    if (typeof record.id !== "string" || typeof record.status !== "string") continue;
    const tags = Array.isArray(record.tags)
      ? record.tags.filter((tag): tag is string => typeof tag === "string")
      : undefined;
    tasks.push(tags === undefined ? { id: record.id, status: record.status } : { id: record.id, status: record.status, tags });
  }
  return tasks;
}

/** List `backlog` tasks tagged `owner:agent` through the Orbit client. */
export async function listBacklogAgentTasks(client: OrbitToolClient): Promise<DispatchTask[]> {
  const result = await client.callTool("orbit.task.list", {
    status: "backlog",
    tag: "owner:agent",
    fields: ["id", "status", "tags"],
    limit: 200,
  });
  return tasksFromListResult(result);
}

export interface ShipOptions {
  /** Executable to spawn. Resolved through PATH. Default `"orbit"`. */
  command?: string;
  env?: NodeJS.ProcessEnv;
  timeoutMs?: number;
}

/** Submit ids with `orbit run ship <ids>` (review-only: no `--complete`). */
export function shipTasks(ids: readonly string[], options: ShipOptions = {}): Promise<void> {
  if (ids.length === 0) return Promise.resolve();
  const command = options.command ?? "orbit";
  const timeoutMs = options.timeoutMs ?? 120_000;
  return new Promise<void>((resolve, reject) => {
    const child = spawn(command, ["run", "ship", ...ids], {
      env: options.env ?? process.env,
      stdio: ["ignore", "pipe", "pipe"],
      detached: process.platform !== "win32",
    });
    // Node reaps the leader before emitting `exit`. The group id is not safe to signal after that (STD-03 R14).
    let killer: ReturnType<typeof setTimeout> | undefined;
    const killTree = (signal: NodeJS.Signals): void => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      try {
        if (process.platform !== "win32" && child.pid !== undefined) {
          process.kill(-child.pid, signal);
        } else {
          child.kill(signal);
        }
      } catch {
        // Already gone.
      }
    };
    let timedOut = false;
    const timer = setTimeout(() => {
      timedOut = true;
      killTree("SIGTERM");
      // Escalate if the leader is still live after the grace period (STD-03 R12).
      killer = setTimeout(() => killTree("SIGKILL"), 250);
      killer.unref();
    }, timeoutMs);
    timer.unref();
    const clearTimers = (): void => {
      clearTimeout(timer);
      if (killer !== undefined) clearTimeout(killer);
    };
    const stderrChunks: Buffer[] = [];
    child.stderr.on("data", (chunk: Buffer) => stderrChunks.push(chunk));
    child.stdout.on("data", () => undefined);
    child.on("error", (error) => {
      clearTimers();
      reject(error);
    });
    child.on("exit", clearTimers);
    child.on("close", (code, signal) => {
      clearTimers();
      if (timedOut) {
        reject(new Error(`orbit run ship timed out after ${timeoutMs} ms`));
        return;
      }
      if (code !== 0) {
        const stderr = Buffer.concat(stderrChunks).toString("utf8").trim().slice(0, 500);
        reject(new Error(`orbit run ship failed (code ${code ?? "none"}, signal ${signal ?? "none"}): ${stderr}`));
        return;
      }
      resolve();
    });
  });
}

/** Create a single-flight dispatch pass from explicit dependencies. */
export function createDispatchPass(deps: DispatchPassDeps): Dispatcher {
  let running = false;
  let timer: NodeJS.Timeout | undefined;

  async function runOnce(): Promise<DispatchPassResult> {
    if (running) return { skipped: true, submitted: [] };
    running = true;
    try {
      const tasks = await deps.listTasks();
      const ids = selectDispatchIds(tasks);
      if (ids.length > 0) await deps.ship(ids);
      return { skipped: false, submitted: ids };
    } finally {
      running = false;
    }
  }

  return {
    runOnce,
    start() {
      if (timer !== undefined) return;
      timer = setInterval(() => {
        void runOnce().catch((error: unknown) => deps.onError?.(error));
      }, deps.intervalMs ?? DISPATCH_INTERVAL_MS);
      timer.unref();
    },
    stop() {
      if (timer !== undefined) clearInterval(timer);
      timer = undefined;
    },
    get running() {
      return running;
    },
  };
}

export interface CreateDispatcherOptions extends ShipOptions {
  client: OrbitToolClient;
  /** Pass interval. Default {@link DISPATCH_INTERVAL_MS}; `start()` uses it. */
  intervalMs?: number;
  onError?: (error: unknown) => void;
}

/** Create the production dispatcher: Orbit client for listing, `orbit run ship` for submitting. */
export function createDispatcher(options: CreateDispatcherOptions): Dispatcher {
  return createDispatchPass({
    listTasks: () => listBacklogAgentTasks(options.client),
    ship: (ids) => shipTasks(ids, options),
    ...(options.intervalMs === undefined ? {} : { intervalMs: options.intervalMs }),
    ...(options.onError === undefined ? {} : { onError: options.onError }),
  });
}
