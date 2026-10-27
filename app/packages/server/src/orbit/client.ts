/**
 * Orbit access: the app server talks to Orbit as the user, through
 * `orbit tool run <name> --json`, for both `orbit.*` and `office.*` tools
 * — including the tools hidden from MCP (`mcp_scope: none`) that only the
 * app may call. It never reads `.orbit/` directly and never uses Orbit's
 * dashboard HTTP API (docs/design/office/specs/web-app.md#authority).
 *
 * Concurrency (ARCHITECTURE.md, "Runtime concurrency"; STD-03 R2): at most
 * `maxConcurrent` `orbit` subprocesses run at once (default 4). Further
 * calls wait in a FIFO queue of at most `maxQueued` entries (default 32).
 * When that queue is full the call is refused with `queue_full` and is not
 * retained. A queued call still waiting after `queueWaitMs` (default: the
 * same value as `timeoutMs`) is removed and rejected with `queue_timeout`,
 * so abandoned work cannot wait indefinitely. Admitted calls keep FIFO
 * order. The execution timeout starts when a slot is acquired. Failures
 * are typed errors, never bare strings. `pollTool` adds a single-flight
 * cache so many browser tabs polling the same tool share one subprocess,
 * and a short-lived result is reused inside `pollCacheTtlMs`.
 *
 * Workspace binding: the installed app receives its workspace as the
 * `ORBIT_OFFICE_WORKSPACE` environment variable (install.sh service
 * templates; the absolute path recorded as `workspace_path`). Tools that
 * require an explicit workspace selector (see
 * {@link WORKSPACE_REQUIRING_TOOLS}: `orbit.routine.control`,
 * `orbit.auto_task.mint`, and auto-task listing and updates) get `workspace: <binding>` injected into their
 * input when the caller did not supply one. All other tools are sent unchanged, so callers
 * that rely on cwd discovery (for example to-dos calling
 * `orbit.task.list`) keep working. No code here reads `.orbit/`.
 *
 * Authority: the binding names the workspace and grants no operator
 * capability. No `ORBIT_OPERATOR` or other authority variable is put in
 * the service templates, because that would be a standing operator grant
 * to a login service. Until an Orbit-side decision delivers the operator
 * capability to the installed app by another mechanism,
 * `orbit.routine.control` returns `capability_denied` to the app; the
 * app instead manages auto-tasks using the permitted list and update tools.
 * The same applies to
 * `orbit.auto_task.mint`: `orbit tool show orbit.auto_task.mint` states
 * its `acknowledge_unconditional` acknowledgment "requires operator
 * authority", so {@link OrbitToolClient.mintAutoTask} does not succeed
 * without operator authority for an installed-workspace caller; Orbit's
 * `capability_denied` is reported as a typed `not_permitted` error and
 * no capability is granted here.
 */
import { spawn } from "node:child_process";

/** Machine-readable failure kind carried by every {@link OrbitError}. */
export type OrbitErrorCode =
  | "timeout"
  | "queue_timeout"
  | "queue_full"
  | "spawn_failed"
  | "exit_nonzero"
  | "invalid_output"
  | "output_too_large"
  | "not_permitted"
  | "unknown_auto_task";

/** Base class for every failure of the Orbit client. */
export class OrbitError extends Error {
  constructor(
    readonly code: OrbitErrorCode,
    message: string,
    options?: { cause?: unknown },
  ) {
    super(message, options);
    this.name = "OrbitError";
  }
}

/** A call exceeded its timeout and the subprocess was terminated. */
export class OrbitTimeoutError extends OrbitError {
  constructor(
    readonly toolName: string,
    readonly timeoutMs: number,
  ) {
    super("timeout", `orbit tool ${toolName} timed out after ${timeoutMs} ms`);
    this.name = "OrbitTimeoutError";
  }
}

/** A call waited in the queue until `queueWaitMs` and was cancelled before it ran. */
export class OrbitQueueTimeoutError extends OrbitError {
  constructor(
    readonly toolName: string,
    readonly queueWaitMs: number,
  ) {
    super(
      "queue_timeout",
      `orbit tool ${toolName} cancelled after waiting ${queueWaitMs} ms for a subprocess slot`,
    );
    this.name = "OrbitQueueTimeoutError";
  }
}

/**
 * The wait queue was already at `maxQueued`, so the call was refused.
 * Nothing was retained for it.
 */
export class OrbitQueueFullError extends OrbitError {
  constructor(
    readonly toolName: string,
    readonly maxQueued: number,
  ) {
    super("queue_full", `orbit tool ${toolName} refused: ${maxQueued} calls are already waiting`);
    this.name = "OrbitQueueFullError";
  }
}

/** The `orbit` executable could not be started (missing, not executable). */
export class OrbitSpawnError extends OrbitError {
  constructor(toolName: string, cause: unknown) {
    super("spawn_failed", `could not start orbit for tool ${toolName}`, { cause });
    this.name = "OrbitSpawnError";
  }
}

/** The `orbit` subprocess exited with a non-zero code or a signal. */
export class OrbitExitError extends OrbitError {
  constructor(
    readonly toolName: string,
    readonly exitCode: number | null,
    readonly signal: NodeJS.Signals | null,
    readonly stderr: string,
  ) {
    super(
      "exit_nonzero",
      `orbit tool ${toolName} failed (code ${exitCode ?? "none"}, signal ${signal ?? "none"}): ${stderr.slice(0, 500)}`,
    );
    this.name = "OrbitExitError";
  }
}

/**
 * Orbit refused `orbit.auto_task.mint` because the caller does not hold the
 * capability it requires (`capability_denied`). Minting unconditionally
 * requires operator authority (see `orbit tool show orbit.auto_task.mint`),
 * which the installed app does not hold, so a feature should show this in
 * plain words rather than retry.
 */
export class OrbitNotPermittedError extends OrbitError {
  constructor(
    readonly toolName: string,
    readonly autoTaskName: string,
    cause?: unknown,
  ) {
    super(
      "not_permitted",
      `not permitted to mint auto-task "${autoTaskName}": Orbit denied the capability this app does not hold`,
      cause === undefined ? undefined : { cause },
    );
    this.name = "OrbitNotPermittedError";
  }
}

/** No auto-task definition with the requested name exists in the workspace. */
export class OrbitUnknownAutoTaskError extends OrbitError {
  constructor(
    readonly autoTaskName: string,
    cause?: unknown,
  ) {
    super(
      "unknown_auto_task",
      `no auto-task named "${autoTaskName}" is defined in this workspace`,
      cause === undefined ? undefined : { cause },
    );
    this.name = "OrbitUnknownAutoTaskError";
  }
}

/** The subprocess answered, but its stdout was not valid JSON. */
export class OrbitInvalidOutputError extends OrbitError {
  constructor(toolName: string, cause: unknown) {
    super("invalid_output", `orbit tool ${toolName} returned invalid JSON`, { cause });
    this.name = "OrbitInvalidOutputError";
  }
}

export interface OrbitClientOptions {
  /** Executable to spawn. Resolved through PATH. Default `"orbit"`. */
  command?: string;
  /** Max concurrent subprocesses. Default 4. Must be at least 1. */
  maxConcurrent?: number;
  /**
   * Max calls waiting for a subprocess slot. Default 32. Must be a
   * non-negative integer. When every slot is busy and this many calls are
   * already waiting, the new call is refused with {@link OrbitQueueFullError}
   * and is not enqueued (STD-03 R2: refuse). Admitted calls stay FIFO.
   * `0` refuses every call that cannot start immediately.
   */
  maxQueued?: number;
  /**
   * Per-call execution timeout in milliseconds. It starts when a subprocess
   * slot is acquired, so queue wait does not consume it. Default 30_000.
   */
  timeoutMs?: number;
  /**
   * How long a call may wait in the queue before it is removed and rejected
   * with {@link OrbitQueueTimeoutError}. Default: `timeoutMs`. This bounds
   * abandoned work that has not yet started a subprocess.
   */
  queueWaitMs?: number;
  /** How long a `pollTool` result stays reusable. Default 2_000. `0` disables reuse. */
  pollCacheTtlMs?: number;
  /** Environment for the child. Default `process.env` (the call runs as the user). */
  env?: NodeJS.ProcessEnv;
  /**
   * Installed-workspace binding: the absolute workspace path delivered as
   * `ORBIT_OFFICE_WORKSPACE` by install.sh. Default: that variable from
   * `env` (or `process.env`). An explicit value overrides the environment;
   * an empty value is treated as unset. Used only for
   * {@link WORKSPACE_REQUIRING_TOOLS}; see the module docstring.
   */
  workspace?: string;
  /** Captured-output byte ceiling per stream. Default 10 MiB. */
  maxOutputBytes?: number;
}

export interface OrbitToolClient {
  /** Call one tool by name, e.g. `orbit.task.list` or `office.status`. Never cached. */
  callTool(name: string, input: Record<string, unknown>): Promise<unknown>;
  /**
   * Mint one task now from the named auto-task definition via
   * `orbit.auto_task.mint`, with the installed workspace binding and
   * `acknowledge_unconditional: true`, under the same supervision, timeout
   * and output bounds as every other call. Rejects with
   * {@link OrbitUnknownAutoTaskError} when no such definition exists and
   * {@link OrbitNotPermittedError} (`not_permitted`) when Orbit reports
   * `capability_denied`, which is the expected outcome for the installed
   * app until Orbit grants it operator authority by another mechanism.
   */
  mintAutoTask(name: string): Promise<unknown>;
  /**
   * Poll one tool: concurrent identical polls share one subprocess, and a
   * fresh result is reused for `pollCacheTtlMs` so many tabs share one call.
   */
  pollTool(name: string, input: Record<string, unknown>): Promise<unknown>;
  /** Number of subprocesses currently running. */
  readonly activeCount: number;
  /** Number of calls waiting for a subprocess slot. */
  readonly queuedCount: number;
}

const DEFAULT_MAX_CONCURRENT = 4;
/** Finite wait-queue capacity. Full callers are refused, not enqueued. */
const DEFAULT_MAX_QUEUED = 32;
const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_POLL_CACHE_TTL_MS = 2_000;
const DEFAULT_MAX_OUTPUT_BYTES = 10 * 1024 * 1024;

/** Environment variable carrying the installed-workspace binding. */
export const WORKSPACE_ENV = "ORBIT_OFFICE_WORKSPACE";

/**
 * Tools that require an explicit `workspace` selector in their input.
 * Only these receive the installed-workspace binding; every other tool
 * is sent exactly as the caller wrote it.
 */
export const WORKSPACE_REQUIRING_TOOLS: ReadonlySet<string> = new Set([
  "orbit.routine.control",
  "orbit.auto_task.mint",
  "orbit.auto_task.list",
  "orbit.auto_task.update",
]);

/** Stable cache key: tool name plus JSON with recursively sorted keys. */
function cacheKey(name: string, input: Record<string, unknown>): string {
  return `${name}\n${stableStringify(input)}`;
}

function stableStringify(value: unknown): string {
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value) ?? "undefined";
  }
  if (Array.isArray(value)) {
    return `[${value.map((item) => stableStringify(item)).join(",")}]`;
  }
  const entries = Object.entries(value as Record<string, unknown>)
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([key, val]) => `${JSON.stringify(key)}:${stableStringify(val)}`);
  return `{${entries.join(",")}}`;
}

interface QueuedCall {
  /** Start this admitted call. Clears its queue-wait deadline. */
  run: () => void;
}

/** Extract Orbit's `{error, code}` refusal from a subprocess stderr line. */
function orbitRefusal(stderr: string): { code: string; message: string } | null {
  try {
    const parsed: unknown = JSON.parse(stderr.trim());
    if (parsed !== null && typeof parsed === "object") {
      const record = parsed as Record<string, unknown>;
      if (typeof record.code === "string") {
        return { code: record.code, message: typeof record.error === "string" ? record.error : "" };
      }
    }
  } catch {
    // Not a JSON refusal; the caller keeps the original exit error.
  }
  return null;
}

/** Map an `orbit.auto_task.mint` failure to its typed error, or keep it. */
function mapMintError(name: string, error: unknown): unknown {
  if (!(error instanceof OrbitExitError)) return error;
  const refusal = orbitRefusal(error.stderr);
  const code = refusal?.code ?? "";
  const message = (refusal?.message ?? error.stderr).toLowerCase();
  if (code === "capability_denied" || code === "policy_denied" || message.includes("capability denied")) {
    return new OrbitNotPermittedError(error.toolName, name, error);
  }
  if (
    code === "auto_task_not_found" ||
    code === "not_found" ||
    code === "unknown_auto_task" ||
    code.includes("not_found") ||
    message.includes("unknown auto-task") ||
    message.includes("auto-task not found") ||
    message.includes("no auto-task")
  ) {
    return new OrbitUnknownAutoTaskError(name, error);
  }
  return error;
}

/** The `orbit tool run --json` client. See the module docstring. */
export function createOrbitClient(options: OrbitClientOptions = {}): OrbitToolClient {
  const command = options.command ?? "orbit";
  const maxConcurrent = options.maxConcurrent ?? DEFAULT_MAX_CONCURRENT;
  const maxQueued = options.maxQueued ?? DEFAULT_MAX_QUEUED;
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const queueWaitMs = options.queueWaitMs ?? timeoutMs;
  const pollCacheTtlMs = options.pollCacheTtlMs ?? DEFAULT_POLL_CACHE_TTL_MS;
  const env = options.env ?? process.env;
  const rawWorkspace = options.workspace ?? env[WORKSPACE_ENV];
  const workspaceBinding = rawWorkspace !== undefined && rawWorkspace.trim() !== "" ? rawWorkspace : undefined;
  const maxOutputBytes = options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES;
  if (!Number.isInteger(maxConcurrent) || maxConcurrent < 1) {
    throw new RangeError("maxConcurrent must be a positive integer");
  }
  if (!Number.isInteger(maxQueued) || maxQueued < 0) {
    throw new RangeError("maxQueued must be a non-negative integer");
  }
  if (!(timeoutMs > 0)) {
    throw new RangeError("timeoutMs must be positive");
  }
  if (!Number.isFinite(queueWaitMs) || !(queueWaitMs > 0)) {
    throw new RangeError("queueWaitMs must be a positive finite number");
  }

  let active = 0;
  // STD-03 R2: bounded queue. When `queue.length` is already `maxQueued`,
  // the producer is refused (`queue_full`) and no entry is retained.
  // Waiters stay in insertion order. A waiter still present after
  // `queueWaitMs` is removed and rejected (`queue_timeout`).
  const queue: QueuedCall[] = [];
  const inflightPolls = new Map<string, Promise<unknown>>();
  const pollCache = new Map<string, { value: unknown; expiresAt: number }>();

  function pump(): void {
    while (active < maxConcurrent && queue.length > 0) {
      const next = queue.shift();
      if (next === undefined) break;
      next.run();
    }
  }

  function release(): void {
    active -= 1;
    pump();
  }

  function begin(
    name: string,
    input: Record<string, unknown>,
    resolve: (value: unknown) => void,
    reject: (reason: unknown) => void,
  ): void {
    active += 1;
    execute(name, input).then(
      (value) => {
        release();
        resolve(value);
      },
      (error: unknown) => {
        release();
        reject(error instanceof Error ? error : new Error(String(error)));
      },
    );
  }

  /**
   * Admit one call. A free slot and an empty queue starts it now. Otherwise
   * it joins the FIFO queue, or is refused when that queue is already full.
   */
  function runSubprocess(name: string, input: Record<string, unknown>): Promise<unknown> {
    return new Promise<unknown>((resolve, reject) => {
      if (active < maxConcurrent && queue.length === 0) {
        begin(name, input, resolve, reject);
        return;
      }
      if (queue.length >= maxQueued) {
        reject(new OrbitQueueFullError(name, maxQueued));
        return;
      }

      let settled = false;
      const waiting: { entry: QueuedCall | null } = { entry: null };
      const timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        const entry = waiting.entry;
        if (entry !== null) {
          const index = queue.indexOf(entry);
          if (index >= 0) queue.splice(index, 1);
        }
        reject(new OrbitQueueTimeoutError(name, queueWaitMs));
      }, queueWaitMs);
      timer.unref();
      const entry: QueuedCall = {
        run: () => {
          if (settled) return;
          settled = true;
          clearTimeout(timer);
          begin(name, input, resolve, reject);
        },
      };
      waiting.entry = entry;
      queue.push(entry);
      pump();
    });
  }

  function execute(name: string, input: Record<string, unknown>): Promise<unknown> {
    return new Promise<unknown>((resolve, reject) => {
      const child = spawn(command, ["tool", "run", name, "--json", "--input", JSON.stringify(input)], {
        env,
        stdio: ["ignore", "pipe", "pipe"],
        // Own process group so a timeout can signal the whole tree (STD-03 R11/R12).
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
          // Already gone; the close handler settles the call.
        }
      };
      let settled = false;
      let leaderExited = false;
      let timedOut = false;
      let outputTooLarge = false;
      let spawnError: unknown;
      let closed = false;
      const stdoutChunks: Buffer[] = [];
      const stderrChunks: Buffer[] = [];
      let stdoutBytes = 0;
      let stderrBytes = 0;

      const timer = setTimeout(() => {
        timedOut = true;
        // After `exit` the leader is reaped: never signal (STD-03 R14), but
        // still settle — a descendant holding a pipe must not hang the call.
        if (leaderExited) {
          settleBounded();
          return;
        }
        killTree("SIGTERM");
        // Escalate if the leader is still live after the grace period (STD-03 R12).
        killer = setTimeout(() => {
          killer = undefined;
          killTree("SIGKILL");
          if (leaderExited) settleBounded();
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

      /** Settle without waiting for `close`, mirroring its precedence. No signal is sent. */
      const settleBounded = (): void => {
        finish(() => {
          if (outputTooLarge) {
            reject(new OrbitError("output_too_large", `orbit tool ${name} exceeded the output limit`));
            return;
          }
          if (spawnError !== undefined) {
            reject(new OrbitSpawnError(name, spawnError));
            return;
          }
          reject(new OrbitTimeoutError(name, timeoutMs));
        });
      };

      child.stdout.on("data", (chunk: Buffer) => {
        if (outputTooLarge) return;
        stdoutBytes += chunk.length;
        if (stdoutBytes > maxOutputBytes) {
          // Settle only on `close`: the slot must stay occupied until the
          // killed child has terminated and its streams have closed.
          outputTooLarge = true;
          killTree("SIGKILL");
          return;
        }
        stdoutChunks.push(chunk);
      });
      child.stderr.on("data", (chunk: Buffer) => {
        stderrBytes += chunk.length;
        if (stderrBytes <= maxOutputBytes) stderrChunks.push(chunk);
      });
      child.on("error", (error) => {
        if (spawnError === undefined) spawnError = error;
        // Node emits `close` after a spawn `error`; settle there so the
        // slot is not released before the child is reaped. If `close`
        // already fired, settle now.
        if (closed) {
          finish(() => reject(new OrbitSpawnError(name, error)));
        }
      });
      child.on("exit", () => {
        // `exit` means the leader is reaped but its pipes may stay open via a
        // descendant. Keep the deadline armed (it settles without signalling);
        // if it already fired, bound the remaining wait for `close` by the
        // same 250 ms grace instead of waiting indefinitely.
        leaderExited = true;
        if ((timedOut || outputTooLarge) && killer === undefined) {
          killer = setTimeout(() => {
            killer = undefined;
            settleBounded();
          }, 250);
          killer.unref();
        }
      });
      child.on("close", (code, signal) => {
        closed = true;
        finish(() => {
          if (outputTooLarge) {
            reject(new OrbitError("output_too_large", `orbit tool ${name} exceeded the output limit`));
            return;
          }
          if (spawnError !== undefined) {
            reject(new OrbitSpawnError(name, spawnError));
            return;
          }
          if (timedOut) {
            reject(new OrbitTimeoutError(name, timeoutMs));
            return;
          }
          if (code !== 0) {
            reject(
              new OrbitExitError(
                name,
                code,
                signal,
                Buffer.concat(stderrChunks).toString("utf8").trim(),
              ),
            );
            return;
          }
          const text = Buffer.concat(stdoutChunks).toString("utf8").trim();
          if (text === "") {
            resolve(null);
            return;
          }
          try {
            resolve(JSON.parse(text) as unknown);
          } catch (cause) {
            reject(new OrbitInvalidOutputError(name, cause));
          }
        });
      });
    });
  }

  /** Inject the binding before any cache key is built from the input. */
  function withWorkspace(name: string, input: Record<string, unknown>): Record<string, unknown> {
    if (!WORKSPACE_REQUIRING_TOOLS.has(name)) return input;
    if (workspaceBinding === undefined) return input;
    if (input.workspace !== undefined) return input;
    return { ...input, workspace: workspaceBinding };
  }

  return {
    callTool(name, input) {
      return runSubprocess(name, withWorkspace(name, input));
    },
    async mintAutoTask(name) {
      if (typeof name !== "string" || name.trim() === "") {
        throw new RangeError("mintAutoTask name must be a non-empty string");
      }
      try {
        return await runSubprocess(
          "orbit.auto_task.mint",
          withWorkspace("orbit.auto_task.mint", { name, acknowledge_unconditional: true }),
        );
      } catch (error) {
        throw mapMintError(name, error);
      }
    },
    pollTool(name, input) {
      const effective = withWorkspace(name, input);
      const key = cacheKey(name, effective);
      const cached = pollCache.get(key);
      if (cached !== undefined && cached.expiresAt > Date.now()) {
        return Promise.resolve(cached.value);
      }
      const inflight = inflightPolls.get(key);
      if (inflight !== undefined) return inflight;
      const promise = runSubprocess(name, effective).then(
        (value) => {
          inflightPolls.delete(key);
          if (pollCacheTtlMs > 0) {
            pollCache.set(key, { value, expiresAt: Date.now() + pollCacheTtlMs });
          }
          return value;
        },
        (error: unknown) => {
          inflightPolls.delete(key);
          throw error;
        },
      );
      inflightPolls.set(key, promise);
      return promise;
    },
    get activeCount() {
      return active;
    },
    get queuedCount() {
      return queue.length;
    },
  };
}
