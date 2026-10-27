/** Source reads and capture; all Library access goes through Office tools. */
import { chmod, mkdtemp, open, realpath, rmdir, unlink } from "node:fs/promises";
import type { IncomingMessage, ServerResponse } from "node:http";
import { join } from "node:path";
import type { CapturedSource, SourceCitation, SourcePlan, SourceView } from "@orbit-office/contracts";
import type { Feature, Route } from "../../http/types.js";
import { createOrbitClient, type OrbitToolClient } from "../../orbit/client.js";

type Row = Record<string, unknown>;
class Refusal extends Error {
  constructor(readonly status: number, readonly code: string) { super(code); }
}
function object(value: unknown): Row {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Refusal(502, "invalid_output");
  return value as Row;
}
function output(value: unknown): Row {
  const row = object(value);
  if (row.ok === false) throw new Refusal(409, "source_refused");
  return row.ok === true ? object(row.output) : row;
}
function string(value: unknown): string {
  if (typeof value !== "string") throw new Refusal(400, "invalid_input");
  return value;
}
function id(value: unknown): string {
  const result = string(value);
  if (!/^src-[a-z0-9]+(?:-[a-z0-9]+)*$/.test(result)) throw new Refusal(400, "invalid_input");
  return result;
}
function closed(row: Row, keys: string[]): void {
  if (Object.keys(row).some(key => !keys.includes(key))) throw new Refusal(400, "invalid_input");
}
function digest(value: unknown): string {
  const result = string(value);
  if (!/^[a-f0-9]{64}$/.test(result)) throw new Refusal(400, "invalid_input");
  return result;
}
function phase(value: unknown): string {
  const result = string(value);
  if (!["docs", "kt", "team", "process"].includes(result)) throw new Refusal(400, "invalid_input");
  return result;
}
function reply(res: ServerResponse, status: number, value: unknown): void {
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  res.end(JSON.stringify(value));
}
async function body(req: IncomingMessage): Promise<Row> {
  if (req.headers["content-type"]?.split(";")[0] !== "application/json") throw new Refusal(415, "invalid_input");
  const chunks: Buffer[] = []; let bytes = 0;
  for await (const chunk of req) {
    const part = Buffer.from(chunk as Uint8Array); bytes += part.length;
    if (bytes > 1024 * 1024) throw new Refusal(413, "upload_too_large");
    chunks.push(part);
  }
  try { return object(JSON.parse(Buffer.concat(chunks).toString("utf8"))); }
  catch { throw new Refusal(400, "invalid_input"); }
}
const extensions = /\.(txt|md|vtt|html?|pdf|docx|pptx|png|jpe?g|wav|mp3|m4a|mp4|webm|mov)$/i;
export interface SourcesOptions {
  client: Pick<OrbitToolClient, "callTool">;
  /** Must be within the workspace granted to the plugin, never a browser path. */
  workspaceRoot?: string;
  maxUploadBytes?: number;
}
export function createSourcesFeature(options: SourcesOptions): Feature {
  const call = async (name: string, input: Row): Promise<Row> => output(await options.client.callTool(name, input));
  function route(method: "GET" | "POST", path: string, handle: Route["handle"]): Route {
    return { method, path, async handle(req, res) {
      try { req.setTimeout(30_000, () => { req.destroy(); }); await handle(req, res); }
      catch (error) { if (!res.headersSent) reply(res, error instanceof Refusal ? error.status : 502, { error: error instanceof Refusal ? error.code : "source_unavailable" }); else res.end(); }
    } };
  }
  async function capture(input: Row): Promise<CapturedSource> {
    const data = await call("office.capture", input);
    return { id: id(data.id), kind: string(data.kind), title: string(data.title) };
  }
  return { name: "sources", status: "built", routes: [
    route("GET", "/api/sources", async (req, res) => {
      const params = new URL(req.url ?? "/", "http://localhost").searchParams;
      const data = await call("office.source-show", { id: id(params.get("id")) });
      const provenance = object(data.derived_by);
      const routes = Object.values(provenance).filter(value => value !== "vtt-import").map(value => string(object(value).route));
      const privateRoute = routes.some(value => value.startsWith("private:"));
      let label = "Private";
      if (privateRoute) {
        const status = await call("office.status", {});
        const configured = status.open_weights === null ? null : object(status.open_weights);
        label = configured === null ? "Private: endpoint not configured" : `Private: ${string(configured.label)}${configured.kind === "provider" ? " (third party)" : ""}`;
      }
      if (!Array.isArray(data.cited_by) || !Array.isArray(data.chapters) || !Array.isArray(data.frames) || !Array.isArray(data.gotchas) || typeof data.task_citers_available !== "boolean" || typeof data.cited_by_truncated !== "boolean" || !["normal", "private"].includes(string(data.sensitivity))) throw new Refusal(502, "invalid_output");
      const view: SourceView = {
        id: id(data.id), kind: string(data.kind), title: string(data.title), text: string(data.text),
        sensitivity: data.sensitivity as SourceView["sensitivity"], media_type: typeof data.media_type === "string" ? data.media_type : null,
        route_labels: [...new Set(routes.map(value => value === "hosted" ? "Hosted" : label))],
        cited_by: data.cited_by as SourceView["cited_by"], chapters: data.chapters as SourceView["chapters"],
        frames: data.frames as SourceView["frames"], gotchas: data.gotchas as SourceView["gotchas"],
        task_citers_available: data.task_citers_available, cited_by_truncated: data.cited_by_truncated,
      };
      reply(res, 200, view);
    }),
    route("GET", "/api/sources/media", async (req, res) => {
      const params = new URL(req.url ?? "/", "http://localhost").searchParams;
      const frame = params.get("frame");
      if (frame !== null && !/^\d{4,10}\.png$/.test(frame)) throw new Refusal(400, "invalid_input");
      const data = await call("office.source-show", { id: id(params.get("id")), media: frame === null ? "original" : "frame", ...(frame === null ? {} : { frame }) });
      const media = object(data.media);
      if (media.too_large === true) throw new Refusal(413, "media_too_large");
      const mime = string(media.media_type);
      if (!/^(image\/(png|jpeg|webp)|audio\/(mpeg|mp4|wav|x-wav|webm)|video\/(mp4|webm|quicktime))$/.test(mime)) throw new Refusal(415, "media_unavailable");
      const bytes = Buffer.from(string(media.content_base64), "base64");
      const headers = { "content-type": mime, "cache-control": "no-store", "x-content-type-options": "nosniff", "accept-ranges": "bytes" };
      const range = req.headers.range;
      if (range !== undefined) {
        const match = /^bytes=(\d*)-(\d*)$/.exec(range);
        const start = match?.[1] ? Number(match[1]) : Math.max(0, bytes.length - Number(match?.[2]));
        const end = match?.[1] && match[2] ? Math.min(Number(match[2]), bytes.length - 1) : bytes.length - 1;
        if (!match || (!match[1] && !match[2]) || !Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || start > end || start >= bytes.length) {
          res.writeHead(416, { ...headers, "content-range": `bytes */${bytes.length}` }); res.end(); return;
        }
        const part = bytes.subarray(start, end + 1);
        res.writeHead(206, { ...headers, "content-length": part.length, "content-range": `bytes ${start}-${end}/${bytes.length}` }); res.end(part); return;
      }
      res.writeHead(200, { ...headers, "content-length": bytes.length }); res.end(bytes);
    }),
    route("POST", "/api/sources/upload", async (req, res) => {
      const params = new URL(req.url ?? "/", "http://localhost").searchParams;
      closed(Object.fromEntries(params), ["name", "phase", "private"]);
      const name = string(params.get("name"));
      if (!name || name.length > 200 || /[\\/\x00-\x1f]/.test(name) || !extensions.test(name)) throw new Refusal(400, "unsupported_file");
      const tags = params.has("phase") ? [`bootstrap:${phase(params.get("phase"))}`] : [];
      if (params.has("private") && params.get("private") !== "true") throw new Refusal(400, "invalid_input");
      if (req.headers["content-type"] !== "application/octet-stream") throw new Refusal(415, "invalid_input");
      const root = options.workspaceRoot ?? process.env.ORBIT_OFFICE_WORKSPACE;
      if (!root) throw new Refusal(503, "upload_root_unconfigured");
      const dir = await mkdtemp(join(await realpath(root), ".office-upload-"));
      const path = join(dir, name); let file; let captured: CapturedSource | undefined;
      let failure: unknown; let cleanupFailed = false;
      try {
        await chmod(dir, 0o700);
        file = await open(path, "wx", 0o600); let bytes = 0;
        for await (const chunk of req) {
          const part = Buffer.from(chunk as Uint8Array); bytes += part.length;
          if (bytes > (options.maxUploadBytes ?? 256 * 1024 * 1024)) throw new Refusal(413, "upload_too_large");
          await file.writeFile(part);
        }
        await file.close(); file = undefined;
        captured = await capture({ upload_path: path, title: name, captured_by: "upload", tags, ...(params.has("private") ? { sensitivity: "private" } : {}) });
      } catch (error) { failure = error; }
      finally {
        if (file) await file.close().catch(() => { cleanupFailed = true; });
        await unlink(path).catch(error => { if ((error as NodeJS.ErrnoException).code !== "ENOENT") cleanupFailed = true; });
        await rmdir(dir).catch(() => { cleanupFailed = true; });
      }
      if (cleanupFailed) throw new Refusal(502, "upload_cleanup_failed");
      if (failure !== undefined) throw failure;
      if (captured === undefined) throw new Refusal(502, "source_unavailable");
      reply(res, 200, captured);
    }),
    route("POST", "/api/sources/paste", async (req, res) => {
      const input = await body(req); closed(input, ["title", "text", "private"]);
      if (!string(input.title).trim() || !string(input.text).trim() || typeof input.private !== "boolean") throw new Refusal(400, "invalid_input");
      reply(res, 200, await capture({ title: input.title, text: input.text, kind: "doc", captured_by: "paste", ...(input.private ? { sensitivity: "private" } : {}) }));
    }),
    route("POST", "/api/sources/private", async (req, res) => {
      const input = await body(req); closed(input, ["id"]);
      await call("office.source-set-sensitivity", { id: id(input.id), sensitivity: "private" }); reply(res, 200, { ok: true });
    }),
    route("POST", "/api/sources/plan", async (req, res) => {
      const input = await body(req); closed(input, ["id", "phase", "private", "confirm", "digest"]);
      if ((input.id === undefined) === (input.phase === undefined) || typeof input.private !== "boolean" || (input.confirm !== undefined && input.confirm !== true) || (input.confirm === true) !== (input.digest !== undefined)) throw new Refusal(400, "invalid_input");
      const video = input.id !== undefined;
      const target = video ? { id: id(input.id) } : { phase: phase(input.phase) };
      const data = await call(video ? "office.video-plan" : "office.batch-plan", { ...target, ...(input.private ? { sensitivity: "private" } : {}), ...(input.confirm === true ? { confirm: true, digest: digest(input.digest) } : {}) });
      const plan = object(data.plan); const counts = object(plan.counts);
      if (Object.values(counts).some(value => typeof value !== "number" || !Number.isSafeInteger(value) || value < 0)) throw new Refusal(502, "invalid_output");
      const videos = video ? [plan] : Array.isArray(plan.videos) ? plan.videos.map(object) : [];
      const preview: SourcePlan = {
        digest: digest(plan.digest), ...(video ? { source: id(plan.id) } : { phase: phase(plan.phase) as NonNullable<SourcePlan["phase"]> }),
        private: video ? plan.sensitivity === "private" : plan.private === true,
        minutes: videos.reduce((sum, item) => sum + Number(item.duration_ms) / 60000, 0),
        frames: Number(counts.frames), calls: Object.values(counts).reduce<number>((sum, count) => sum + Number(count), 0),
        sources: video ? 1 : Array.isArray(plan.sources) ? plan.sources.length : 0,
        estimate: plan.estimate === null ? null : { amount: Number(object(plan.estimate).amount), currency: string(object(plan.estimate).currency) },
      };
      reply(res, 200, preview);
    }),
    route("GET", "/api/sources/citation", async (req, res) => {
      const params = new URL(req.url ?? "/", "http://localhost").searchParams;
      const kind = params.get("kind"); const key = string(params.get("id"));
      const tool = kind === "task" && /^[A-Za-z0-9-]+$/.test(key) ? "orbit.task.show" : kind === "artifact" && /^art-[a-z0-9-]+$/.test(key) ? "office.artifact-show" : ["domain", "runbook"].includes(kind ?? "") && /^(dom|rb)-[a-z0-9-]+$/.test(key) ? "office.knowledge-show" : null;
      if (!tool) throw new Refusal(400, "invalid_input");
      const data = await call(tool, { id: key }); const document = object(data.page ?? data.artifact ?? data.task ?? data);
      const citation: SourceCitation = { title: string(document.title), text: string(document.body ?? data.body ?? (typeof document.execution_summary === "string" && document.execution_summary.trim() ? document.execution_summary : undefined) ?? document.description ?? "No text available.") };
      reply(res, 200, citation);
    }),
  ] };
}
export const feature = createSourcesFeature({ client: createOrbitClient() });
