/** The only credential writer. No output from this child is captured or logged. */
import { spawn } from "node:child_process";

export class SecretError extends Error {
  constructor(readonly code: "secret_not_saved" | "secret_cleanup_failed" | "secret_timeout") {
    super(code);
  }
}

export interface SecretOptions {
  command?: string;
  env?: NodeJS.ProcessEnv;
  timeoutMs?: number;
}

/** Fixed slot names only, never arbitrary command arguments from a response. */
export function isSlot(slot: unknown): slot is string {
  return typeof slot === "string" && /^conn_(0[1-9]|1[0-6])$/.test(slot);
}

/** The private-route token. The name is fixed; the value travels on stdin only. */
export function writeOpenWeightsToken(value: string, options: SecretOptions = {}): Promise<void> {
  if (value.length === 0) return Promise.reject(new SecretError("secret_not_saved"));
  return runSecret("set", "office", "open_weights_token", value, "secret_not_saved", options);
}

export async function writeSecret(
  verb: "set" | "rm", slot: string, value: string | undefined, options: SecretOptions = {},
): Promise<void> {
  const failure = verb === "set" ? "secret_not_saved" : "secret_cleanup_failed";
  if (!isSlot(slot)) throw new SecretError(failure);
  await runSecret(verb, "gateway", slot, value, failure, options);
}

async function runSecret(
  verb: "set" | "rm",
  scope: string,
  name: string,
  value: string | undefined,
  failure: SecretError["code"],
  options: SecretOptions,
): Promise<void> {
  const timeoutMs = options.timeoutMs ?? 10_000;
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) throw new SecretError(failure);
  await new Promise<void>((resolve, reject) => {
    const child = spawn(options.command ?? "orbit", ["plugin", "secret", verb, scope, name], {
      env: options.env ?? process.env,
      // Discard output at the OS boundary, even if Orbit echoes a credential.
      stdio: ["pipe", "ignore", "ignore"],
      detached: process.platform !== "win32",
    });
    let timedOut = false;
    let writeFailed = false;
    let killer: ReturnType<typeof setTimeout> | undefined;
    const kill = (signal: NodeJS.Signals): void => {
      // Node reaps before emitting exit. Never signal a potentially reused PID.
      if (child.exitCode !== null || child.signalCode !== null) return;
      try {
        if (process.platform !== "win32" && child.pid !== undefined) process.kill(-child.pid, signal);
        else child.kill(signal);
      } catch { /* Already gone. The close event settles the operation. */ }
    };
    const timer = setTimeout(() => {
      timedOut = true;
      kill("SIGTERM");
      killer = setTimeout(() => kill("SIGKILL"), 250);
    }, timeoutMs);
    const clearTimers = (): void => {
      clearTimeout(timer);
      if (killer !== undefined) clearTimeout(killer);
    };
    child.on("exit", clearTimers);
    child.stdin.on("error", () => { writeFailed = true; });
    child.on("error", () => { writeFailed = true; });
    child.on("close", (code) => {
      clearTimers();
      if (timedOut) reject(new SecretError("secret_timeout"));
      else if (code !== 0 || writeFailed) reject(new SecretError(failure));
      else resolve();
    });
    // No newline is added: the bytes the user entered are the bytes sent.
    child.stdin.end(value);
  });
}
