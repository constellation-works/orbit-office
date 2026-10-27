/**
 * Loopback HTTP guards (STD-05 R16/R17): a loopback bind is not
 * authentication. Every route, including `/healthz`, validates the `Host`
 * header against an approved loopback authority and refuses a missing or
 * unparsable `Host`. Any request carrying an `Origin`, and every
 * state-changing request, must carry a loopback `http` Origin that
 * matches the validated `Host`.
 */
import type { IncomingMessage } from "node:http";

const STATE_CHANGING = new Set(["POST", "PUT", "PATCH", "DELETE"]);

/** True for `localhost`, any `127.0.0.0/8` address, or `::1`. */
export function isLoopbackHostname(hostname: string): boolean {
  const host = hostname.toLowerCase().replace(/\.$/, "");
  if (host === "localhost" || host.endsWith(".localhost")) return true;
  if (host === "::1") return true;
  const parts = host.split(".");
  if (parts.length === 4 && parts[0] === "127") {
    return parts.every((part) => /^\d{1,3}$/.test(part) && Number(part) <= 255);
  }
  return false;
}

/** Parse a `Host` header into hostname/port, or undefined when unparsable. */
export function parseHostHeader(header: string | undefined): { hostname: string; host: string } | undefined {
  if (header === undefined) return undefined;
  const host = header.trim().toLowerCase();
  if (host === "" || /[\s/@\\]/.test(host)) return undefined;
  let hostname: string;
  if (host.startsWith("[")) {
    const close = host.indexOf("]");
    if (close < 0) return undefined;
    hostname = host.slice(1, close);
    const rest = host.slice(close + 1);
    if (rest !== "" && !/^:\d{1,5}$/.test(rest)) return undefined;
  } else {
    const colon = host.lastIndexOf(":");
    if (colon >= 0) {
      if (!/^\d{1,5}$/.test(host.slice(colon + 1))) return undefined;
      hostname = host.slice(0, colon);
      if (hostname.includes(":")) return undefined; // Bare IPv6 without brackets.
    } else {
      hostname = host;
    }
  }
  if (hostname === "") return undefined;
  return { hostname, host };
}

/** Check one request's Host/Origin. Returns the validated host when allowed. */
export function checkLoopbackRequest(
  request: Pick<IncomingMessage, "method" | "headers">,
): { ok: true; host: string } | { ok: false } {
  const parsed = parseHostHeader(request.headers.host);
  if (parsed === undefined || !isLoopbackHostname(parsed.hostname)) return { ok: false };

  const origin = request.headers.origin;
  const method = (request.method ?? "GET").toUpperCase();
  if (origin !== undefined) {
    let url: URL;
    try {
      url = new URL(origin);
    } catch {
      return { ok: false };
    }
    if (url.protocol !== "http:" || url.username !== "" || url.password !== "") return { ok: false };
    // `URL.hostname` strips IPv6 brackets; `parsed.host` keeps the raw
    // authority. Compare bare hostname plus effective port instead.
    const originHostname = url.hostname.toLowerCase().replace(/^\[|\]$/g, "");
    if (!isLoopbackHostname(originHostname)) return { ok: false };
    const requestedPort = parsed.host.startsWith("[")
      ? (parsed.host.split("]:")[1] ?? "")
      : (parsed.host.split(":")[1] ?? "");
    if (originHostname !== parsed.hostname || url.port !== requestedPort) return { ok: false };
    return { ok: true, host: parsed.host };
  }
  if (STATE_CHANGING.has(method)) return { ok: false };
  return { ok: true, host: parsed.host };
}
