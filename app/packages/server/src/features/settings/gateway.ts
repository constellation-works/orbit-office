/** Closed HTTP input and projected gateway output. Never return raw tool errors. */
import type { ApiCall, ConnectionInput, ConnectionWrite, SettingsConnection } from "@orbit-office/contracts";

export class SettingsError extends Error {
  constructor(readonly status: number, readonly code: string) { super(code); }
}

export function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function invalid(): never { throw new SettingsError(400, "invalid_input"); }
function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item: unknown) => typeof item === "string");
}

/** Validate types and unknown keys here; gateway owns connection policy validation. */
export function connectionInput(value: unknown): ConnectionWrite {
  const keys = ["name", "base_url", "auth", "methods", "drafts", "timeout_seconds", "max_response_bytes", "disabled", "secret"];
  if (!object(value) || Object.keys(value).some((key) => !keys.includes(key)) ||
      typeof value.name !== "string" || typeof value.base_url !== "string" ||
      !object(value.auth) || Object.keys(value.auth).some((key) => !["scheme", "header", "username"].includes(key)) ||
      typeof value.auth.scheme !== "string" || !["bearer", "basic", "header", "none"].includes(value.auth.scheme) ||
      (value.auth.header !== undefined && typeof value.auth.header !== "string") ||
      (value.auth.username !== undefined && typeof value.auth.username !== "string") ||
      !strings(value.methods) || !strings(value.drafts) || !Number.isSafeInteger(value.timeout_seconds) ||
      !Number.isSafeInteger(value.max_response_bytes) || typeof value.disabled !== "boolean" ||
      (value.secret !== undefined && (typeof value.secret !== "string" || value.secret.length === 0 || value.auth.scheme === "none"))) invalid();
  return value as unknown as ConnectionWrite;
}

/** Tool execution failures can include arbitrary diagnostics; only known codes cross HTTP. */
export function toolData(value: unknown): Record<string, unknown> {
  if (!object(value)) throw new SettingsError(502, "invalid_gateway_data");
  if (value.ok === false) {
    const code = object(value.error) ? value.error.code : undefined;
    if (code === "invalid_connection" || code === "invalid_argument") throw new SettingsError(400, "invalid_connection");
    if (code === "no_free_slot") throw new SettingsError(409, "no_free_slot");
    throw new SettingsError(502, "gateway_unavailable");
  }
  // Orbit's tool CLI returns the tool payload; tolerate the exec envelope as well.
  return value.ok === true && object(value.output) ? value.output : value;
}

export function connection(value: unknown): SettingsConnection {
  if (!object(value) || typeof value.slot_state !== "string" || !["set", "unset", "none"].includes(value.slot_state)) throw new SettingsError(502, "invalid_gateway_data");
  const projected: Record<string, unknown> = {};
  for (const key of ["name", "base_url", "auth", "methods", "drafts", "timeout_seconds", "max_response_bytes", "disabled"]) projected[key] = value[key];
  // Project auth separately too: no accidental credential field can reach HTTP.
  if (!object(value.auth)) throw new SettingsError(502, "invalid_gateway_data");
  projected.auth = { scheme: value.auth.scheme,
    ...(value.auth.header === undefined ? {} : { header: value.auth.header }),
    ...(value.auth.username === undefined ? {} : { username: value.auth.username }) };
  let config: ConnectionInput;
  try { config = connectionInput(projected); } catch { throw new SettingsError(502, "invalid_gateway_data"); }
  return { ...config, slot_state: value.slot_state as SettingsConnection["slot_state"] };
}

export function count(value: unknown): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new SettingsError(502, "invalid_gateway_data");
  return value;
}

export function auditRow(value: unknown): ApiCall {
  if (!object(value) || typeof value.time !== "string" || !Number.isFinite(Date.parse(value.time)) ||
      typeof value.tool !== "string" || typeof value.outcome !== "string" || !object(value.caller) ||
      !["connection", "method", "path"].every((key) => value[key] === null || typeof value[key] === "string") ||
      !(value.status === null || (typeof value.status === "number" && Number.isInteger(value.status)))) {
    throw new SettingsError(502, "invalid_gateway_data");
  }
  const caller: ApiCall["caller"] = {};
  for (const key of ["agent", "model"] as const) {
    if (typeof value.caller[key] === "string") caller[key] = value.caller[key];
  }
  return { time: value.time, tool: value.tool, caller, connection: value.connection as string | null,
    method: value.method as string | null, path: value.path as string | null, status: value.status as number | null,
    bytes: count(value.bytes), duration_ms: count(value.duration_ms), outcome: value.outcome };
}
