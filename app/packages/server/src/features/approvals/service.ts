/**
 * The Needs-you actions, through the Orbit client as the user.
 *
 * - Approve: `office.artifact-promote`, then the task walks to `done` with a
 *   comment naming who approved. No `verdict` object or `complete` flag: the
 *   installed app holds no operator authority. A refused promote changes
 *   nothing, so the task stays waiting for review.
 * - Send back: `review` to `backlog` with the user's comment.
 * - Accept: `office.proposal-accept` once, bound to the digest the user saw;
 *   a shared-library conflict also carries the user's `choice` (theirs or mine).
 * - Dismiss: the task is rejected; a gateway draft is never sent. A dismissed
 *   proposal then runs `office.proposal-reconcile`, which moves its evidence
 *   page to `proposals/dismissed/`; a failed move leaves the dismissal standing.
 * - Send: `gateway.http-send` with the draft id and the digest shown. A lost
 *   reply is `outcome_unknown`, never a failure, and is never retried.
 *
 * Orbit refusing the app's authority for any of these (`capability_denied`,
 * `policy_denied`) reaches the screen as the reason `not_permitted`: nothing
 * changed, and trying again will not help (docs/design/office/specs/proposals.md).
 *
 * Library drafts for someone else are never sent from here; only a gateway
 * draft the user approves goes out (docs/design/office/specs/gateway.md).
 */
import { createHash } from "node:crypto";

import type {
  AcceptAllResult,
  ApprovalDetail,
  ApproveResult,
  ConflictChoice,
  GatewaySendResult,
  TodayNeedKind,
} from "@orbit-office/contracts";

import { OrbitExitError, type OrbitToolClient } from "../../orbit/client.js";

export type ApprovalClient = Pick<OrbitToolClient, "callTool">;

export class ApprovalError extends Error {
  constructor(
    readonly status: number,
    readonly code: string,
    /** A tool's refusal code, passed on so the screen can say what happened. */
    readonly reason?: string,
  ) {
    super(code);
    this.name = "ApprovalError";
  }
}

const TASK_ID = /^[A-Za-z0-9._:-]{1,128}$/;
const SHA256 = /^[a-f0-9]{64}$/;
const CODE = /^[a-z][a-z0-9_]{0,63}$/;
/** Files a script-output task carries that are bookkeeping, not output. */
const BOOKKEEPING = new Set(["artifact-manifest.json", "script-run.json", "proposal.json", "gateway-draft.json"]);

interface Shown {
  id: string;
  title: string;
  status: string;
  tags: string[];
  summary: string;
  artifacts: string[];
}

export function isTaskKey(key: unknown): key is string {
  return typeof key === "string" && TASK_ID.test(key);
}

export function isDigest(value: unknown): value is string {
  return typeof value === "string" && SHA256.test(value);
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** Orbit's refusals of the caller's authority; the screen says `not_permitted`. */
const PERMISSION = new Set(["capability_denied", "policy_denied"]);

/**
 * A tool's own refusal code, from an exec envelope or Orbit's stderr line.
 * Orbit refusing the app's authority is `not_permitted`, whichever tool it was.
 */
function refusal(error: unknown): string | undefined {
  if (error instanceof ToolRefusal) return PERMISSION.has(error.code) ? "not_permitted" : error.code;
  if (!(error instanceof OrbitExitError)) return undefined;
  for (const line of error.stderr.split("\n").reverse()) {
    try {
      const parsed: unknown = JSON.parse(line);
      if (!record(parsed)) continue;
      const code = record(parsed.error) ? parsed.error.code : parsed.code;
      if (typeof code === "string" && PERMISSION.has(code)) return "not_permitted";
      if (typeof code === "string") return CODE.test(code) ? code : "refused";
    } catch {
      // Not a JSON line.
    }
  }
  return undefined;
}

class ToolRefusal extends Error {
  constructor(readonly code: string) {
    super(code);
  }
}

/** Call one tool and unwrap the exec envelope; a refusal is a {@link ToolRefusal}. */
async function call(client: ApprovalClient, name: string, input: Record<string, unknown>): Promise<unknown> {
  const value = await client.callTool(name, input);
  if (record(value) && value.ok === false) {
    const code = record(value.error) ? value.error.code : undefined;
    throw new ToolRefusal(typeof code === "string" && CODE.test(code) ? code : "refused");
  }
  return record(value) && value.ok === true && "output" in value ? value.output : value;
}

/** Call a tool for a step the user asked for; any failure is `failure` with the tool's reason. */
async function step(
  client: ApprovalClient,
  name: string,
  input: Record<string, unknown>,
  failure: string,
): Promise<unknown> {
  try {
    return await call(client, name, input);
  } catch (error) {
    const reason = refusal(error);
    throw reason === undefined ? new ApprovalError(502, "unavailable") : new ApprovalError(409, failure, reason);
  }
}

async function show(client: ApprovalClient, key: string): Promise<Shown> {
  let value: unknown;
  try {
    value = await call(client, "orbit.task.show", {
      id: key,
      fields: ["id", "title", "status", "tags", "execution_summary", "artifacts"],
    });
  } catch (error) {
    const reason = refusal(error);
    if (reason === "task_not_found" || reason === "unknown_task_prefix") throw new ApprovalError(404, "not_found");
    throw new ApprovalError(502, "unavailable");
  }
  const task = record(value) && record(value.task) ? value.task : value;
  if (!record(task) || task.id !== key || typeof task.status !== "string" || !Array.isArray(task.tags)) {
    throw new ApprovalError(502, "unavailable");
  }
  const artifacts = Array.isArray(task.artifacts) ? task.artifacts : [];
  return {
    id: key,
    title: typeof task.title === "string" ? task.title : "",
    status: task.status,
    tags: task.tags.filter((tag: unknown): tag is string => typeof tag === "string"),
    summary: typeof task.execution_summary === "string" ? task.execution_summary : "",
    artifacts: artifacts
      .map((item: unknown) => (typeof item === "string" ? item : record(item) && typeof item.path === "string" ? item.path : ""))
      .filter((path: string) => path !== ""),
  };
}

/** The same classification as Today, so an action only applies to what the card showed. */
function kindOf(task: Shown): TodayNeedKind | undefined {
  if (task.status === "proposed" && task.tags.includes("gateway-draft")) return "gateway_draft";
  if (task.status === "proposed" && task.tags.includes("script-output")) return "script_output";
  if (task.status === "proposed" && task.tags.includes("proposal")) return "proposal";
  if (task.status === "review") return "approval";
  return undefined;
}

async function actionable(client: ApprovalClient, key: string, kinds: readonly TodayNeedKind[]): Promise<Shown & { kind: TodayNeedKind }> {
  const task = await show(client, key);
  const kind = kindOf(task);
  if (kind === undefined || !kinds.includes(kind)) throw new ApprovalError(409, "not_actionable");
  return { ...task, kind };
}

/** One stored artifact's bytes: Orbit returns UTF-8 text as `content`, anything else as base64. */
async function artifactBytes(client: ApprovalClient, key: string, path: string): Promise<Buffer> {
  let value: unknown;
  try {
    value = await call(client, "orbit.task.artifact.get", { id: key, path });
  } catch {
    throw new ApprovalError(502, "unavailable");
  }
  const artifact = record(value) && record(value.artifact) ? value.artifact : value;
  if (record(artifact) && typeof artifact.content === "string") return Buffer.from(artifact.content, "utf8");
  if (record(artifact) && typeof artifact.content_base64 === "string") return Buffer.from(artifact.content_base64, "base64");
  throw new ApprovalError(502, "unavailable");
}

function json(bytes: Buffer): Record<string, unknown> {
  try {
    const parsed: unknown = JSON.parse(bytes.toString("utf8"));
    if (record(parsed)) return parsed;
  } catch {
    // Reported below.
  }
  throw new ApprovalError(502, "unavailable");
}

function fileName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

/** What a proposal changes, as text the user can read before accepting. */
function proposalPreview(payload: Record<string, unknown>): string {
  if (typeof payload.markdown === "string" && payload.markdown.trim() !== "") return payload.markdown;
  const page = record(payload.page) ? payload.page : undefined;
  if (page !== undefined && typeof page.body === "string" && page.body.trim() !== "") return page.body;
  if (payload.kind === "auto_task" && record(payload.definition)) {
    const name = typeof payload.definition.name === "string" ? payload.definition.name : "a recurring task";
    return `Sets up a recurring task: ${name}.`;
  }
  if (payload.kind === "retire") return "Retires something nobody uses any more.";
  return "The suggested change is described above.";
}

/** Everything the card shows before the user acts. */
export async function loadDetail(client: ApprovalClient, key: string): Promise<ApprovalDetail> {
  const task = await actionable(client, key, ["proposal", "approval", "script_output", "gateway_draft"]);
  switch (task.kind) {
    case "proposal": {
      const bytes = await artifactBytes(client, key, "proposal.json");
      const payload = json(bytes);
      return {
        kind: "proposal",
        key,
        preview: proposalPreview(payload),
        sha256: createHash("sha256").update(bytes).digest("hex"),
        shareable: payload.shareable === true,
        conflict: payload.kind === "shared_conflict",
      };
    }
    case "approval":
    case "script_output":
      return {
        kind: task.kind,
        key,
        files: task.artifacts
          .filter((path) => !BOOKKEEPING.has(path) && !path.toLowerCase().startsWith("review-") && !path.includes("/"))
          .map(fileName),
      };
    case "gateway_draft": {
      const draft = await gatewayDraft(client, key);
      return { kind: "gateway_draft", key, ...draft.shown };
    }
    case "todo":
      throw new ApprovalError(409, "not_actionable");
  }
}

interface GatewayDraft {
  id: string;
  shown: { connection: string; method: string; url: string; body: string | null; sha256: string };
}

async function gatewayDraft(client: ApprovalClient, key: string): Promise<GatewayDraft> {
  const document = json(await artifactBytes(client, key, "gateway-draft.json"));
  const request = record(document.request) ? document.request : undefined;
  if (typeof document.id !== "string" || request === undefined || typeof request.method !== "string" ||
      typeof request.url !== "string" || !(request.body === null || typeof request.body === "string") ||
      typeof document.sha256 !== "string" || !SHA256.test(document.sha256)) {
    throw new ApprovalError(502, "unavailable");
  }
  return {
    id: document.id,
    shown: {
      connection: typeof document.connection === "string" ? document.connection : "",
      method: request.method,
      url: request.url,
      body: request.body,
      sha256: document.sha256,
    },
  };
}

/** Status updates that take a task from `status` to done; `done` needs a summary and `review` first. */
function closeSteps(status: string): string[] | null {
  switch (status) {
    case "done":
      return [];
    case "review":
      return ["done"];
    case "in-progress":
      return ["review", "done"];
    case "backlog":
      return ["in-progress", "review", "done"];
    case "proposed":
      return ["backlog", "in-progress", "review", "done"];
    default:
      return null;
  }
}

/** Approve: promote first; only a promoted output closes the task. */
export async function approve(client: ApprovalClient, key: string, approver: string): Promise<ApproveResult> {
  const task = await actionable(client, key, ["approval", "script_output"]);
  await step(client, "office.artifact-promote", { task: key }, "promote_refused");
  const steps = closeSteps(task.status);
  if (steps === null) return { approved: true, closed: false };
  const comment = `Approved by ${approver} in the office app; the output was added to the Library.`;
  try {
    if (steps.length === 0) await call(client, "orbit.task.update", { id: key, comment });
    for (const status of steps) {
      const input: Record<string, unknown> = { id: key, status };
      if (status === "done") {
        if (task.summary.trim() === "") input.execution_summary = `Approved by ${approver}; output added to the Library.`;
        input.comment = comment;
      }
      await call(client, "orbit.task.update", input);
    }
  } catch {
    // The promote stands; the user sees that the close is still pending.
    return { approved: true, closed: false };
  }
  return { approved: true, closed: true };
}

export async function sendBack(client: ApprovalClient, key: string, comment: string, approver: string): Promise<void> {
  await actionable(client, key, ["approval"]);
  await step(client, "orbit.task.update", {
    id: key,
    status: "backlog",
    comment: `Sent back by ${approver} from the office app: ${comment}`,
  }, "send_back_refused");
}

export async function dismiss(client: ApprovalClient, key: string, reason: string, approver: string): Promise<void> {
  const task = await actionable(client, key, ["proposal", "script_output", "gateway_draft"]);
  const why = reason === "" ? "" : ` Reason: ${reason}`;
  await step(client, "orbit.task.update", {
    id: key,
    status: "rejected",
    comment: `Dismissed by ${approver} in the office app.${why}`,
  }, "dismiss_refused");
  if (task.kind !== "proposal") return;
  // The rejection is the record. Moving the evidence page to dismissed/ follows
  // it; if that fails, the next reconcile moves it, so the dismissal stands.
  try {
    await call(client, "office.proposal-reconcile", {});
  } catch {
    // Reported by the next reconcile, not to the user who dismissed.
  }
}

export function isChoice(value: unknown): value is ConflictChoice {
  return value === "theirs" || value === "mine";
}

/**
 * Exactly one `office.proposal-accept` call; the tool checks the task and
 * digest, and requires `choice` for a shared-library conflict only.
 */
export async function accept(client: ApprovalClient, key: string, sha256: string, choice?: ConflictChoice): Promise<void> {
  await step(client, "office.proposal-accept", choice === undefined ? { id: key, sha256 } : { id: key, sha256, choice }, "accept_refused");
}

/** Each in turn; a refusal is reported and the rest still run, so earlier accepts stand. */
export async function acceptAll(
  client: ApprovalClient,
  proposals: ReadonlyArray<{ key: string; sha256: string }>,
): Promise<AcceptAllResult> {
  const results: AcceptAllResult["results"] = [];
  for (const { key, sha256 } of proposals) {
    try {
      await accept(client, key, sha256);
      results.push({ key, accepted: true });
    } catch (error) {
      const code = error instanceof ApprovalError ? error.reason ?? error.code : "unavailable";
      results.push({ key, accepted: false, error: code });
    }
  }
  return { results };
}

export async function send(client: ApprovalClient, key: string, sha256: string): Promise<GatewaySendResult> {
  await actionable(client, key, ["gateway_draft"]);
  const draft = await gatewayDraft(client, key);
  const unknown: GatewaySendResult = { outcome: "outcome_unknown", status: null, task_closed: false };
  let value: unknown;
  try {
    value = await call(client, "gateway.http-send", { draft: draft.id, sha256 });
  } catch (error) {
    // A refusal sent nothing. Anything else (a timeout, a lost reply) may
    // have gone out, so it is never reported as a failure or retried.
    const reason = refusal(error);
    if (reason !== undefined && !reason.includes("outcome_unknown")) throw new ApprovalError(409, "send_refused", reason);
    return unknown;
  }
  if (!record(value) || (value.outcome !== "sent" && value.outcome !== "outcome_unknown")) return unknown;
  return {
    outcome: value.outcome,
    status: typeof value.status === "number" ? value.status : null,
    task_closed: value.task_closed === true,
  };
}
