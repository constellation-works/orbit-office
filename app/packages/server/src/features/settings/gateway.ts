/** Closed HTTP input and projected gateway output. Never return raw tool errors. */
import type { ApiCall, ConnectionInput, ConnectionWrite, SettingsConnection, SignInPoll, SignInStarted } from "@orbit-office/contracts";

export class SettingsError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    readonly details: { route_saved?: true; field?: string; reason?: string } = {},
  ) { super(code); }
}

export function object(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function invalid(): never { throw new SettingsError(400, "invalid_input"); }
function strings(value: unknown): value is string[] {
  return Array.isArray(value) && value.every((item: unknown) => typeof item === "string");
}

const SCHEMES = ["bearer", "basic", "header", "none", "secret_path", "oauth2_device"];
const AUTH_TEXT = ["header", "username", "token_url", "device_url", "client_id"];
const NAME = /^[a-z][a-z0-9-]{0,47}$/;

function authValid(auth: unknown): boolean {
  return object(auth) && Object.keys(auth).every((key) => key === "scheme" || key === "scopes" || AUTH_TEXT.includes(key)) &&
    typeof auth.scheme === "string" && SCHEMES.includes(auth.scheme) &&
    AUTH_TEXT.every((key) => auth[key] === undefined || typeof auth[key] === "string") &&
    (auth.scopes === undefined || strings(auth.scopes));
}

/** Validate types and unknown keys here; gateway owns connection policy validation. */
export function connectionInput(value: unknown): ConnectionWrite {
  const keys = ["name", "base_url", "auth", "methods", "drafts", "timeout_seconds", "max_response_bytes", "disabled", "idempotency_header", "secret"];
  if (!object(value) || Object.keys(value).some((key) => !keys.includes(key)) ||
      typeof value.name !== "string" || typeof value.base_url !== "string" || !authValid(value.auth) ||
      !strings(value.methods) || !strings(value.drafts) || !Number.isSafeInteger(value.timeout_seconds) ||
      !Number.isSafeInteger(value.max_response_bytes) || typeof value.disabled !== "boolean" ||
      (value.idempotency_header !== undefined && typeof value.idempotency_header !== "string")) invalid();
  // An oauth2_device refresh token comes only from the sign-in, never from HTTP.
  const scheme = (value.auth as Record<string, unknown>).scheme;
  if (value.secret !== undefined && (typeof value.secret !== "string" || value.secret.length === 0 ||
      scheme === "none" || scheme === "oauth2_device")) invalid();
  return value as unknown as ConnectionWrite;
}

/** `{ name }` for the sign-in routes: a connection name and nothing else. */
export function signInInput(value: unknown): string {
  if (!object(value) || Object.keys(value).some((key) => key !== "name") ||
      typeof value.name !== "string" || !NAME.test(value.name)) invalid();
  return value.name;
}

/** Tool execution failures can include arbitrary diagnostics; only known codes cross HTTP. */
export function toolData(value: unknown): Record<string, unknown> {
  if (!object(value)) throw new SettingsError(502, "invalid_gateway_data");
  if (value.ok === false) {
    const code = object(value.error) ? value.error.code : undefined;
    if (code === "invalid_connection" || code === "invalid_argument") throw new SettingsError(400, "invalid_connection");
    if (code === "no_free_slot") throw new SettingsError(409, "no_free_slot");
    if (code === "unknown_connection") throw new SettingsError(404, "unknown_connection");
    if (code === "device_flow_missing") throw new SettingsError(409, "sign_in_not_started");
    if (code === "auth_failed") throw new SettingsError(502, "sign_in_failed");
    throw new SettingsError(502, "gateway_unavailable");
  }
  // Orbit's tool CLI returns the tool payload; tolerate the exec envelope as well.
  return value.ok === true && object(value.output) ? value.output : value;
}

export function connection(value: unknown): SettingsConnection {
  if (!object(value) || typeof value.slot_state !== "string" || !["set", "unset", "none"].includes(value.slot_state)) throw new SettingsError(502, "invalid_gateway_data");
  const projected: Record<string, unknown> = {};
  for (const key of ["name", "base_url", "auth", "methods", "drafts", "timeout_seconds", "max_response_bytes", "disabled"]) projected[key] = value[key];
  if (value.idempotency_header !== undefined) projected.idempotency_header = value.idempotency_header;
  // Project auth separately too: no accidental credential field can reach HTTP.
  if (!object(value.auth)) throw new SettingsError(502, "invalid_gateway_data");
  const auth: Record<string, unknown> = { scheme: value.auth.scheme };
  for (const key of [...AUTH_TEXT, "scopes"]) if (value.auth[key] !== undefined) auth[key] = value.auth[key];
  projected.auth = auth;
  let config: ConnectionInput;
  try { config = connectionInput(projected); } catch { throw new SettingsError(502, "invalid_gateway_data"); }
  const saved: SettingsConnection = { ...config, slot_state: value.slot_state as SettingsConnection["slot_state"] };
  if (value.auth_state !== undefined) {
    if (typeof value.auth_state !== "string" || !["signed_in", "signed_out", "expired"].includes(value.auth_state)) throw new SettingsError(502, "invalid_gateway_data");
    saved.auth_state = value.auth_state as NonNullable<SettingsConnection["auth_state"]>;
  }
  return saved;
}

function webAddress(value: unknown): value is string {
  if (typeof value !== "string") return false;
  try { return ["https:", "http:"].includes(new URL(value).protocol); } catch { return false; }
}

/** Only what the user needs to approve; a device code would never pass this projection. */
export function signInStarted(value: unknown, name: string): SignInStarted {
  if (!object(value) || value.connection !== name || typeof value.user_code !== "string" || value.user_code.length === 0 ||
      value.user_code.length > 64 || !webAddress(value.verification_uri) ||
      !Number.isSafeInteger(value.expires_in) || !Number.isSafeInteger(value.interval)) throw new SettingsError(502, "invalid_gateway_data");
  return { connection: name, user_code: value.user_code, verification_uri: value.verification_uri,
    expires_in: value.expires_in as number, interval: value.interval as number };
}

export function signInPoll(value: unknown, name: string): SignInPoll {
  if (!object(value) || value.connection !== name || typeof value.state !== "string" ||
      !["pending", "connected", "denied", "expired"].includes(value.state) ||
      (value.interval !== undefined && !Number.isSafeInteger(value.interval))) throw new SettingsError(502, "invalid_gateway_data");
  return { connection: name, state: value.state as SignInPoll["state"], ...(value.interval === undefined ? {} : { interval: value.interval as number }) };
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
