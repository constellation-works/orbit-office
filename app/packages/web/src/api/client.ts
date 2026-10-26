/**
 * Typed fetch helpers for the app server's HTTP API. Screens call these,
 * never `fetch` directly, so errors are handled in one place.
 */

/** A non-2xx response from the app server. */
export class ApiError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
  ) {
    super(`app server answered ${status} (${code})`);
  }
}

/** GET a JSON resource from the app server. */
export async function getJson<T>(path: string): Promise<T> {
  const response = await fetch(path, { headers: { accept: "application/json" } });
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as { error?: unknown };
    throw new ApiError(response.status, typeof body.error === "string" ? body.error : "unknown");
  }
  return (await response.json()) as T;
}
