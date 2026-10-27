/** Typed access to the app server, with one error path. */
import type { Health, SettingsStatus } from "@orbit-office/contracts";

export class ApiError extends Error {
  constructor(readonly status: number, readonly code: string) {
    super(`app server answered ${status} (${code})`);
    this.name = "ApiError";
  }
}

/** Fetch JSON, translating network, HTTP and malformed response errors alike. */
export async function getJson<T>(path: string): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, { headers: { accept: "application/json" } });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => undefined);
    const code = typeof body === "object" && body !== null && "error" in body && typeof body.error === "string"
      ? body.error : "unknown";
    throw new ApiError(response.status, code);
  }
  try {
    return (await response.json()) as T;
  } catch {
    throw new ApiError(response.status, "invalid_json");
  }
}

/** Read the server health contract. */
export function getHealth(): Promise<Health> {
  return getJson<Health>("/healthz");
}

/** Read the settings contract: the agent CLI connection. */
export function getSettings(): Promise<SettingsStatus> {
  return getJson<SettingsStatus>("/api/settings");
}
