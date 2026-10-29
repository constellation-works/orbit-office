/**
 * Redacted route-failure diagnostics.
 *
 * One line per failed route call, naming only the route, the tool and a
 * typed failure kind — plus, when Orbit sent one, its machine-readable
 * refusal `code` and the child's exit code. Tool output, child stdout or
 * stderr text, request bodies, error messages and secret values are never
 * included: an `OrbitExitError` message embeds stderr, so only a JSON
 * refusal `code` validated against a closed shape is ever extracted.
 */
import { OrbitError, OrbitExitError } from "./client.js";

/** Orbit refusal codes are short lower-case identifiers; anything else is dropped. */
const SAFE_CODE = /^[a-z][a-z0-9_]{0,63}$/;

export interface FailureDiagnostic {
  /** Typed client kind (`timeout`, `spawn_failed`, `exit_nonzero`, …) or `orbit_error`/`unknown`. */
  kind: string;
  /** Orbit refusal `code`, only when it matches the closed safe shape. */
  code?: string;
  /** Child exit code for `exit_nonzero` failures. */
  exitCode?: number | null;
}

/** Extract a safe refusal code from an exec-envelope value (`{ok:false,error:{code}}`). */
export function safeCodeFromValue(value: unknown): string | undefined {
  if (value === null || typeof value !== "object" || Array.isArray(value)) return undefined;
  const record = value as Record<string, unknown>;
  const inner = record.error;
  const candidate =
    inner !== null && typeof inner === "object" && !Array.isArray(inner)
      ? (inner as Record<string, unknown>).code
      : record.code;
  return typeof candidate === "string" && SAFE_CODE.test(candidate) ? candidate : undefined;
}

/** Extract a safe refusal code from a child's stderr JSON lines, never its text. */
function safeCodeFromStderr(stderr: string): string | undefined {
  for (const line of stderr.split("\n").reverse()) {
    const trimmed = line.trim();
    if (!trimmed.startsWith("{")) continue;
    try {
      const code = safeCodeFromValue(JSON.parse(trimmed) as unknown);
      if (code !== undefined) return code;
    } catch {
      // Not a JSON refusal line; keep looking, never log the line itself.
    }
  }
  return undefined;
}

/** The typed diagnostic for any thrown failure. Never reads `error.message`. */
export function failureDiagnostic(error: unknown): FailureDiagnostic {
  if (error instanceof OrbitExitError) {
    const code = safeCodeFromStderr(error.stderr);
    return {
      kind: "exit_nonzero",
      ...(code !== undefined ? { code } : {}),
      exitCode: error.exitCode,
    };
  }
  if (error instanceof OrbitError) {
    return { kind: error.code };
  }
  // A SettingsError-shaped carrier (gateway.ts) may hold a preserved orbit code.
  if (error !== null && typeof error === "object") {
    const record = error as Record<string, unknown>;
    const carried = record.orbitCode;
    if (typeof carried === "string" && SAFE_CODE.test(carried)) {
      return { kind: "orbit_error", code: carried };
    }
    if (record.diagnostic !== null && typeof record.diagnostic === "object" &&
        typeof record.code === "string" && SAFE_CODE.test(record.code)) {
      return { kind: record.code };
    }
  }
  return { kind: "unknown" };
}

/** The safe Orbit refusal code carried by a failure, when one exists. */
export function safeOrbitCode(error: unknown): string | undefined {
  return failureDiagnostic(error).code;
}

function token(value: string): string {
  // Route and tool are internal constants; strip anything unexpected anyway.
  return /^[A-Za-z0-9_./:-]+$/.test(value) ? value : "unknown";
}

/** Format the single redacted diagnostic line for a failed route call. */
export function formatRouteFailure(route: string, tool: string, error: unknown): string {
  const diagnostic = failureDiagnostic(error);
  let line = `route_failure route=${token(route)} tool=${token(tool)} kind=${diagnostic.kind}`;
  if (diagnostic.code !== undefined) line += ` code=${diagnostic.code}`;
  if (diagnostic.exitCode !== undefined) line += ` exit_code=${diagnostic.exitCode ?? "none"}`;
  return line;
}

/**
 * Write exactly one redacted diagnostic line for a failed route call.
 * Goes to stderr (`console.error`), which the login service captures in
 * its log file. No tool output, request body or secret can appear in it.
 */
export function logRouteFailure(
  route: string,
  tool: string,
  error: unknown,
  write: (line: string) => void = (line) => console.error(line),
): void {
  write(formatRouteFailure(route, tool, error));
}
