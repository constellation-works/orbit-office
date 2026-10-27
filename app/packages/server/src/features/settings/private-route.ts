/**
 * Private route: read the configured endpoint from `office.status` and save
 * it with `office.config-set`, then exactly one `office.private-route-check`.
 * A token, when the user typed one, goes only to
 * `orbit plugin secret set office open_weights_token` on stdin.
 */
import type { EndpointKind, PrivateRouteCheck, PrivateRouteCheckLine, PrivateRouteSaved, PrivateRouteStatus, PrivateRouteWrite, RouteCapability } from "@orbit-office/contracts";

import type { OrbitToolClient } from "../../orbit/client.js";
import { object, SettingsError } from "./gateway.js";
import { SecretError, writeOpenWeightsToken, type SecretOptions } from "./secret.js";

const KINDS: readonly EndpointKind[] = ["company", "provider", "own_hardware"];
const CAPABILITIES: readonly RouteCapability[] = ["text", "image", "audio"];
const BLOCK_KEYS = ["endpoint", "model", "kind", "label", "capabilities", "verified_at"];

export async function readPrivateRoute(client: OrbitToolClient): Promise<PrivateRouteStatus> {
  const output = officeOutput(await client.callTool("office.status", {}), "private_route_unavailable");
  return projectStatus(output);
}

/** Validate before any tool call. The stored label and token are not trimmed. */
export function privateRouteInput(value: unknown): PrivateRouteWrite {
  const keys = ["endpoint", "model", "kind", "label", "token"];
  if (!object(value) || Object.keys(value).some((key) => !keys.includes(key))) failInput();
  const endpoint = value.endpoint;
  const model = value.model;
  const kind = value.kind;
  const label = value.label;
  if (typeof endpoint !== "string" || endpoint.length === 0 || endpoint.length > 2048 || hasControl(endpoint)) failInput();
  if (typeof model !== "string" || model.length === 0 || model.length > 200 || /\s/u.test(model)) failInput();
  if (typeof kind !== "string" || !isKind(kind)) failInput();
  if (typeof label !== "string" || label.length === 0 || label.length > 80 || hasControl(label) || label.trim().length === 0) failInput();
  const input: PrivateRouteWrite = { endpoint, model, kind, label };
  if (value.token !== undefined) {
    const token = value.token;
    if (typeof token !== "string" || token.length === 0 || token.length > 4096 || token.trim().length === 0) failInput();
    input.token = token;
  }
  return input;
}

/**
 * `config-set` first. The secret write and the single check run only after
 * that call reports the block was written. Any later failure says the route
 * was saved and never includes the token or a tool diagnostic.
 */
export async function savePrivateRoute(
  client: OrbitToolClient,
  input: PrivateRouteWrite,
  secretOptions: SecretOptions = {},
): Promise<PrivateRouteSaved> {
  const written = officeOutput(await client.callTool("office.config-set", {
    key: "open_weights",
    value: { endpoint: input.endpoint, model: input.model, kind: input.kind, label: input.label },
  }), "private_route_unavailable");
  if (written.path !== ".office/config.yaml" || written.key !== "open_weights") {
    throw new SettingsError(502, "private_route_unavailable");
  }
  try {
    if (input.token !== undefined) await writeOpenWeightsToken(input.token, secretOptions);
    const checked = officeOutput(await client.callTool("office.private-route-check", {}), "private_route_check_failed");
    return {
      route: { endpoint: input.endpoint, model: input.model, kind: input.kind, label: input.label },
      check: projectCheck(checked, input.token),
    };
  } catch (error) {
    if (error instanceof SettingsError && error.details.route_saved === true) throw error;
    if (error instanceof SecretError) throw new SettingsError(502, error.code, { route_saved: true });
    if (error instanceof SettingsError) throw new SettingsError(error.status, error.code, { route_saved: true });
    throw new SettingsError(502, "private_route_unavailable", { route_saved: true });
  }
}

function failInput(): never {
  throw new SettingsError(400, "invalid_input");
}

function isKind(value: string): value is EndpointKind {
  return (KINDS as readonly string[]).includes(value);
}

function isCapability(value: unknown): value is RouteCapability {
  return typeof value === "string" && (CAPABILITIES as readonly string[]).includes(value);
}

function hasControl(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code <= 0x1f || code === 0x7f) return true;
  }
  return false;
}

/** Tool failures stay on this side of HTTP. Known validation codes are input errors. */
function officeOutput(value: unknown, unavailable: string): Record<string, unknown> {
  if (!object(value)) throw new SettingsError(502, unavailable);
  if (value.ok === false) {
    const code = object(value.error) ? value.error.code : undefined;
    if (code === "invalid_library_setting" || code === "invalid_argument") throw new SettingsError(400, "invalid_input");
    throw new SettingsError(502, unavailable);
  }
  const output = value.ok === true ? value.output : value;
  if (!object(output)) throw new SettingsError(502, unavailable);
  return output;
}

function projectStatus(output: Record<string, unknown>): PrivateRouteStatus {
  if (!object(output.secrets)) throw new SettingsError(502, "private_route_unavailable");
  const token = output.secrets.open_weights_token;
  if (token !== "set" && token !== "unset") throw new SettingsError(502, "private_route_unavailable");
  return { route: projectBlock(output.open_weights), token };
}

function projectBlock(value: unknown): PrivateRouteStatus["route"] {
  if (value === null) return null;
  if (!object(value) || Object.keys(value).some((key) => !BLOCK_KEYS.includes(key))) {
    throw new SettingsError(502, "private_route_unavailable");
  }
  const { endpoint, model, kind, label, capabilities, verified_at } = value;
  if (typeof endpoint !== "string" || typeof model !== "string" || typeof kind !== "string" || !isKind(kind) || typeof label !== "string") {
    throw new SettingsError(502, "private_route_unavailable");
  }
  return {
    endpoint,
    model,
    kind,
    label,
    capabilities: projectCapabilities(capabilities),
    verified_at: projectVerifiedAt(verified_at),
  };
}

function projectCapabilities(value: unknown): RouteCapability[] | null {
  if (value === null) return null;
  if (!Array.isArray(value) || value.some((item) => !isCapability(item))) {
    throw new SettingsError(502, "private_route_unavailable");
  }
  const caps = value.filter(isCapability);
  if (new Set(caps).size !== caps.length) throw new SettingsError(502, "private_route_unavailable");
  return caps;
}

/** A timestamp the check wrote, or null. Anything else is not returned. */
function projectVerifiedAt(value: unknown): string | null {
  if (value === null) return null;
  if (typeof value === "string" && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}/u.test(value)) return value;
  throw new SettingsError(502, "private_route_unavailable");
}

function projectCheck(output: Record<string, unknown>, token: string | undefined): PrivateRouteCheck {
  const { label, kind, model, routable, capabilities, checks } = output;
  if (typeof label !== "string" || typeof kind !== "string" || !isKind(kind) || typeof model !== "string" ||
      typeof routable !== "boolean" || !Array.isArray(capabilities) || !Array.isArray(checks)) {
    throw new SettingsError(502, "private_route_check_failed");
  }
  if (capabilities.some((item) => !isCapability(item))) throw new SettingsError(502, "private_route_check_failed");
  const lines = checks.map(projectLine);
  const echoed = [output.reported_model, ...lines.map((line) => line.note)];
  if (token !== undefined && token.length > 0 && echoed.some((part) => typeof part === "string" && part.includes(token))) {
    throw new SettingsError(502, "private_route_check_failed", { route_saved: true });
  }
  return {
    label,
    kind,
    model,
    routable,
    capabilities: capabilities.filter(isCapability),
    checks: lines,
  };
}

function projectLine(value: unknown): PrivateRouteCheckLine {
  if (!object(value) || !isCapability(value.capability) || typeof value.accepted !== "boolean") {
    throw new SettingsError(502, "private_route_check_failed");
  }
  const elapsed = value.elapsed_ms;
  if (!(elapsed === null || (typeof elapsed === "number" && Number.isSafeInteger(elapsed) && elapsed >= 0))) {
    throw new SettingsError(502, "private_route_check_failed");
  }
  if (!(value.note === null || typeof value.note === "string")) throw new SettingsError(502, "private_route_check_failed");
  return { capability: value.capability, accepted: value.accepted, elapsed_ms: elapsed, note: value.note };
}
