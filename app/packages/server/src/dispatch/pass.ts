/**
 * The dispatch pass: every 30 seconds, list `backlog` tasks tagged
 * `owner:agent` and submit them by id with `orbit run ship <ids>`
 * (review-only). Single-flight: a pass never starts while the last one is
 * running. The office workspace never runs an auto drain, so `owner:you`
 * to-dos are never submitted, and `proposed` tasks are never submitted —
 * only `backlog` tasks carrying `owner:agent` (and not `owner:you`) are
 * selected (docs/design/office/specs/web-app.md). A task whose `not_before`
 * is still in the future waits: Orbit would refuse it as `task_scheduled`,
 * so the pass leaves it out instead of logging that refusal every 30
 * seconds, and ships it on its first pass at or after the time
 * (docs/design/office/specs/calendar.md, "Scheduled tasks").
 */
import { spawn } from "node:child_process";

import { OrbitError, type OrbitToolClient } from "../orbit/client.js";

/** How often the pass runs. */
export const DISPATCH_INTERVAL_MS = 30_000;

/** The task fields the pass needs from `orbit.task.list`. */
export interface DispatchTask {
  id: string;
  status: string;
  tags?: readonly string[];
  /** The scheduled start (RFC 3339); absent or null when the task has none. */
  not_before?: string | null;
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
  /** The pass's clock, read once per pass. Default the system clock. */
  now?: () => Date;
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
 * `owner:you`, and never before its `not_before`. Anything else (`proposed`,
 * `owner:you`, untagged, scheduled later than `now`) is left alone.
 */
export function selectDispatchIds(tasks: readonly DispatchTask[], now: Date = new Date()): string[] {
  return tasks
    .filter((task) => task.status === "backlog")
    .filter((task) => task.tags?.includes("owner:agent") === true)
    .filter((task) => task.tags?.includes("owner:you") !== true)
    .filter((task) => task.not_before === undefined || task.not_before === null || Date.parse(task.not_before) <= now.getTime())
    .map((task) => task.id);
}

/**
 * Extract the task rows from an `orbit.task.list` envelope (or a bare array).
 * A listing the pass cannot read is an `invalid_output` failure, never an
 * empty pass: a scheduled pass reports it through `onError`, and nothing is
 * submitted (STD-02 §R29). A valid empty listing is still `[]`.
 */
export function tasksFromListResult(result: unknown): DispatchTask[] {
  const rows: unknown = Array.isArray(result)
    ? result
    : (result as { tasks?: unknown } | null)?.tasks;
  if (!Array.isArray(rows)) throw invalidListing("a listing without a tasks array");
  return rows.map((row: unknown, index) => taskFromRow(row, index));
}

/**
 * One listing row. `id` and `status` are required strings; `tags`, when
 * present, is an array of strings; `not_before`, when present and not null,
 * is a readable time. A time the pass cannot read fails the listing rather
 * than shipping a task Orbit is holding.
 */
function taskFromRow(row: unknown, index: number): DispatchTask {
  const record = row !== null && typeof row === "object" ? (row as Record<string, unknown>) : undefined;
  if (record === undefined || typeof record.id !== "string" || typeof record.status !== "string") {
    throw invalidListing(`row ${index} without a string id and status`);
  }
  const task: DispatchTask = { id: record.id, status: record.status };
  const tags = record.tags;
  if (tags !== undefined) {
    if (!Array.isArray(tags) || !tags.every((tag): tag is string => typeof tag === "string")) {
      throw invalidListing(`row ${index} with tags that are not an array of strings`);
    }
    task.tags = tags;
  }
  const notBefore = record.not_before;
  if (notBefore !== undefined && notBefore !== null) {
    if (typeof notBefore !== "string" || Number.isNaN(Date.parse(notBefore))) {
      throw invalidListing(`row ${index} with a not_before that is not a time`);
    }
    task.not_before = notBefore;
  }
  return task;
}

function invalidListing(detail: string): OrbitError {
  return new OrbitError("invalid_output", `orbit.task.list returned ${detail}`);
}

/** List `backlog` tasks tagged `owner:agent`, with their scheduled start, through the Orbit client. */
export async function listBacklogAgentTasks(client: OrbitToolClient): Promise<DispatchTask[]> {
  const result = await client.callTool("orbit.task.list", {
    status: "backlog",
    tag: "owner:agent",
    fields: ["id", "status", "tags", "not_before"],
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

/**
 * Max stderr bytes retained for ship diagnostics (STD-03 R15).
 * The failure message shows only a short preview of these bytes; the
 * ceiling bounds retained and concatenated memory on every path,
 * including successful long-running dispatches.
 */
export const MAX_SHIP_STDERR_BYTES = 8 * 1024;

/** Characters of captured stderr shown in the failure message. */
const SHIP_STDERR_PREVIEW_CHARS = 500;

/** Marker appended when captured stderr was truncated. */
const SHIP_STDERR_TRUNCATED_MARK = "... [truncated]";

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
    let leaderExited = false;
    let settled = false;
    const timer = setTimeout(() => {
      timedOut = true;
      // After `exit` the leader is reaped: never signal (STD-03 R14), but
      // still settle — a descendant holding a pipe must not hang the pass.
      if (leaderExited) {
        settleTimedOut();
        return;
      }
      killTree("SIGTERM");
      // Escalate if the leader is still live after the grace period (STD-03 R12).
      killer = setTimeout(() => {
        killer = undefined;
        killTree("SIGKILL");
        if (leaderExited) settleTimedOut();
      }, 250);
      killer.unref();
    }, timeoutMs);
    timer.unref();
    const clearTimers = (): void => {
      clearTimeout(timer);
      if (killer !== undefined) clearTimeout(killer);
    };
    const finish = (fn: () => void): void => {
      if (settled) return;
      settled = true;
      clearTimers();
      fn();
    };
    const settleTimedOut = (): void => {
      finish(() => reject(new Error(`orbit run ship timed out after ${timeoutMs} ms`)));
    };
    let stderrBytes = 0;
    let stderrTruncated = false;
    const stderrChunks: Buffer[] = [];
    child.stderr.on("data", (chunk: Buffer | string) => {
      // Drain every chunk so the child never blocks on a full pipe, but
      // retain the first MAX_SHIP_STDERR_BYTES bytes, including a partial
      // final chunk when it crosses the ceiling (STD-03 R15).
      if (stderrTruncated) return;
      const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
      const remaining = MAX_SHIP_STDERR_BYTES - stderrBytes;
      const retainedBytes = Math.min(bytes.length, remaining);
      if (retainedBytes > 0) stderrChunks.push(bytes.subarray(0, retainedBytes));
      stderrBytes += retainedBytes;
      if (retainedBytes < bytes.length) stderrTruncated = true;
    });
    child.stdout.on("data", () => undefined);
    child.on("error", (error) => {
      finish(() => reject(error));
    });
    child.on("exit", () => {
      // Keep the deadline armed (see the timer above); if it already fired,
      // bound the remaining wait for `close` by the same 250 ms grace.
      leaderExited = true;
      if (timedOut && killer === undefined) {
        killer = setTimeout(() => {
          killer = undefined;
          settleTimedOut();
        }, 250);
        killer.unref();
      }
    });
    child.on("close", (code, signal) => {
      finish(() => {
        if (timedOut) {
          reject(new Error(`orbit run ship timed out after ${timeoutMs} ms`));
          return;
        }
        if (code !== 0) {
          // Bounded: stderrChunks holds at most MAX_SHIP_STDERR_BYTES, so this
          // concat never touches unbounded data (STD-03 R15).
          const captured = Buffer.concat(stderrChunks).toString("utf8").trim();
          const preview = captured.slice(0, SHIP_STDERR_PREVIEW_CHARS);
          const wasCut = stderrTruncated || preview.length < captured.length;
          const suffix = wasCut ? ` ${SHIP_STDERR_TRUNCATED_MARK}` : "";
          reject(new Error(`orbit run ship failed (code ${code ?? "none"}, signal ${signal ?? "none"}): ${preview}${suffix}`));
          return;
        }
        resolve();
      });
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
      const ids = selectDispatchIds(tasks, deps.now?.() ?? new Date());
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
  /** The pass's clock. Default the system clock. */
  now?: () => Date;
}

/** Create the production dispatcher: Orbit client for listing, `orbit run ship` for submitting. */
export function createDispatcher(options: CreateDispatcherOptions): Dispatcher {
  return createDispatchPass({
    listTasks: () => listBacklogAgentTasks(options.client),
    ship: (ids) => shipTasks(ids, options),
    ...(options.intervalMs === undefined ? {} : { intervalMs: options.intervalMs }),
    ...(options.onError === undefined ? {} : { onError: options.onError }),
    ...(options.now === undefined ? {} : { now: options.now }),
  });
}
