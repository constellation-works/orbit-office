/**
 * Static file serving for the web UI: `web/static` (the app shell) first,
 * then `web/dist` (the compiled UI). `/` serves the shell's `index.html`.
 * Paths are contained in one of the roots on the physical path (STD-05
 * R6/R7): a spelled or symlinked escape is refused, never served.
 */
import { createReadStream } from "node:fs";
import { realpath, stat } from "node:fs/promises";
import type { IncomingMessage, ServerResponse } from "node:http";
import { extname, resolve, sep } from "node:path";

const CONTENT_TYPES: Readonly<Record<string, string>> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".gif": "image/gif",
  ".ico": "image/x-icon",
  ".webp": "image/webp",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".txt": "text/plain; charset=utf-8",
  ".map": "application/json",
  ".webmanifest": "application/manifest+json",
};

function inside(root: string, path: string): boolean {
  return path === root || path.startsWith(root + sep);
}

/** Resolve `pathname` inside `root`, or undefined when it escapes or is absent. */
async function resolveInside(root: string, pathname: string): Promise<string | undefined> {
  let realRoot: string;
  try {
    realRoot = await realpath(root);
  } catch {
    return undefined;
  }
  const candidate = resolve(realRoot, `.${pathname}`);
  if (!inside(realRoot, candidate)) return undefined;
  let realCandidate: string;
  try {
    realCandidate = await realpath(candidate);
  } catch {
    return undefined;
  }
  if (!inside(realRoot, realCandidate)) return undefined;
  const info = await stat(realCandidate).catch(() => undefined);
  if (info === undefined) return undefined;
  if (info.isDirectory()) {
    const index = resolve(realCandidate, "index.html");
    const realIndex = await realpath(index).catch(() => undefined);
    if (realIndex === undefined || !inside(realRoot, realIndex)) return undefined;
    const indexInfo = await stat(realIndex).catch(() => undefined);
    return indexInfo?.isFile() === true ? realIndex : undefined;
  }
  return info.isFile() ? realCandidate : undefined;
}

/**
 * Serve one GET/HEAD request from `roots` in order. Returns false when no
 * root holds the file, so the caller can answer 404.
 */
export async function serveStatic(
  request: IncomingMessage,
  response: ServerResponse,
  roots: readonly string[],
  pathname: string,
): Promise<boolean> {
  if (request.method !== "GET" && request.method !== "HEAD") return false;
  let decoded: string;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return false;
  }
  if (decoded.includes("\0")) return false;
  const wanted = decoded === "/" ? "/index.html" : decoded;
  for (const root of roots) {
    const file = await resolveInside(root, wanted);
    if (file === undefined) continue;
    const info = await stat(file);
    const type = CONTENT_TYPES[extname(file).toLowerCase()] ?? "application/octet-stream";
    response.writeHead(200, {
      "content-type": type,
      "content-length": info.size,
      "cache-control": "no-cache",
      "x-content-type-options": "nosniff",
    });
    if (request.method === "HEAD") {
      response.end();
      return true;
    }
    await new Promise<void>((resolveStream, rejectStream) => {
      const stream = createReadStream(file);
      stream.on("error", rejectStream);
      response.on("finish", resolveStream);
      response.on("close", resolveStream);
      stream.pipe(response);
    });
    return true;
  }
  return false;
}
