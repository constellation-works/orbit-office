/** Typed access to the app server, with one error path. */
import type { ConnectionSaved, ConnectionWrite, Health, PrivateRouteSaved, PrivateRouteStatus, PrivateRouteWrite, SettingsApiCalls, SettingsConnections, SettingsStatus } from "@orbit-office/contracts";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly routeSaved = false,
  ) {
    super(`app server answered ${status} (${code})`);
    this.name = "ApiError";
  }
}

/** Fetch JSON, translating network, HTTP and malformed response errors alike. */
export async function getJson<T>(path: string): Promise<T> {
  return requestJson<T>(path);
}

async function requestJson<T>(path: string, body?: unknown): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, body === undefined
      ? { headers: { accept: "application/json" } }
      : { method: "POST", headers: { accept: "application/json", "content-type": "application/json" }, body: JSON.stringify(body) });
  } catch {
    throw new ApiError(0, "network_error");
  }
  if (!response.ok) {
    const body: unknown = await response.json().catch(() => undefined);
    const code = typeof body === "object" && body !== null && "error" in body && typeof body.error === "string"
      ? body.error : "unknown";
    const routeSaved = typeof body === "object" && body !== null && "route_saved" in body && body.route_saved === true;
    throw new ApiError(response.status, code, routeSaved);
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

export function getConnections(): Promise<SettingsConnections> {
  return getJson<SettingsConnections>("/api/settings/connections");
}

export function putConnection(input: ConnectionWrite): Promise<ConnectionSaved> {
  return requestJson<ConnectionSaved>("/api/settings/connections/put", input);
}

export function getApiCalls(): Promise<SettingsApiCalls> {
  return getJson<SettingsApiCalls>("/api/settings/api-calls");
}

/** The configured private endpoint, or null. Never includes a token value. */
export function getPrivateRoute(): Promise<PrivateRouteStatus> {
  return getJson<PrivateRouteStatus>("/api/settings/private-route");
}

/** Save the endpoint block, then read the check. Omit `token` to leave it. */
export function savePrivateRoute(input: PrivateRouteWrite): Promise<PrivateRouteSaved> {
  return requestJson<PrivateRouteSaved>("/api/settings/private-route", input);
}
