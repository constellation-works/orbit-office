/**
 * Shared library: read the `shared` block from `office.status`, save it with
 * `office.config-set`, run `office.library-sync` for **Sync now**, and turn on
 * the hourly `office-shared-sync` routine when the user accepts the offer
 * (docs/design/office/specs/shared-library.md, "App").
 *
 * A refused setting names only its field, never the value. A sync failure is
 * its refusal code; git diagnostics and remote details stay on the tool side.
 * The latest sync the app ran is kept in memory for Settings and Today.
 */
import type {
  SharedLibraryBlock,
  SharedLibrarySaved,
  SharedLibraryStatus,
  SharedRoutineState,
  SharedSkipped,
  SharedSync,
  SharedSyncOutcome,
} from "@orbit-office/contracts";

import { OrbitExitError, OrbitTimeoutError, type OrbitToolClient } from "../../orbit/client.js";
import { routineListing } from "../../orbit/definitions.js";
import { object, SettingsError } from "./gateway.js";

export const SHARED_SYNC_ROUTINE = "office-shared-sync";

type Client = Pick<OrbitToolClient, "callTool">;

const FIELDS = ["remote", "branch", "member"] as const;
const MEMBER = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const CODE = /^[a-z][a-z0-9_]{0,63}$/u;
/** The sync refusals the screen says in words; anything else is `unknown`. */
const SYNC_REASONS = new Set([
  "shared_not_configured", "shared_remote_unreachable", "shared_push_rejected", "shared_layout_invalid",
  "shared_identity_missing", "shared_storage_failed", "library_changed", "library_writer_busy", "sync_timeout",
]);

function hasControl(value: string): boolean {
  for (let index = 0; index < value.length; index += 1) {
    const code = value.charCodeAt(index);
    if (code <= 0x1f || code === 0x7f) return true;
  }
  return false;
}

function refuse(field?: (typeof FIELDS)[number]): never {
  throw new SettingsError(400, "invalid_input", field === undefined ? {} : { field });
}

/**
 * The closed block, checked for shape before any tool call. `office.config-set`
 * owns the remote and branch rules; only the member's form is checked here too,
 * so the screen can name the field.
 */
export function sharedLibraryInput(value: unknown): SharedLibraryBlock {
  if (!object(value) || Object.keys(value).some((key) => !(FIELDS as readonly string[]).includes(key))) refuse();
  const { remote, branch, member } = value;
  if (typeof remote !== "string" || remote.trim() === "" || remote.length > 2048 || hasControl(remote) || remote.trim() !== remote) refuse("remote");
  if (typeof branch !== "string" || branch === "" || branch.length > 255 || hasControl(branch) || /\s/u.test(branch)) refuse("branch");
  if (typeof member !== "string" || member.length > 32 || !MEMBER.test(member)) refuse("member");
  return { remote, branch, member };
}

/** A tool refusal code from an exec envelope or Orbit's stderr line. */
function refusal(error: unknown): string | undefined {
  if (error instanceof ToolRefusal) return error.code;
  if (!(error instanceof OrbitExitError)) return undefined;
  for (const line of error.stderr.split("\n").reverse()) {
    try {
      const parsed: unknown = JSON.parse(line);
      if (!object(parsed)) continue;
      const code = object(parsed.error) ? parsed.error.code : parsed.code;
      if (typeof code === "string") return CODE.test(code) ? code : "refused";
    } catch {
      // Not a JSON line.
    }
  }
  return undefined;
}

class ToolRefusal extends Error {
  constructor(readonly code: string, readonly detail: string) { super(code); }
}

async function call(client: Client, name: string, input: Record<string, unknown>): Promise<Record<string, unknown>> {
  const value = await client.callTool(name, input);
  if (!object(value)) throw new Error("invalid output");
  if (value.ok === false) {
    const error = object(value.error) ? value.error : {};
    const code = typeof error.code === "string" && CODE.test(error.code) ? error.code : "refused";
    throw new ToolRefusal(code, typeof error.message === "string" ? error.message : "");
  }
  const output = value.ok === true ? value.output : value;
  if (!object(output)) throw new Error("invalid output");
  return output;
}

function block(value: unknown): SharedLibraryBlock | null {
  if (value === null || value === undefined) return null;
  if (!object(value) || Object.keys(value).some((key) => !(FIELDS as readonly string[]).includes(key)) ||
      typeof value.remote !== "string" || typeof value.branch !== "string" || typeof value.member !== "string") {
    throw new SettingsError(502, "shared_library_unavailable");
  }
  return { remote: value.remote, branch: value.branch, member: value.member };
}

export async function readSharedBlock(client: Client): Promise<SharedLibraryBlock | null> {
  let output: Record<string, unknown>;
  try { output = await call(client, "office.status", {}); }
  catch { throw new SettingsError(502, "shared_library_unavailable"); }
  return block(output.shared);
}

/** Whether the hourly routine is on. A refused or malformed listing is `unavailable`. */
export async function routineState(client: Client): Promise<SharedRoutineState> {
  try {
    const row = (await routineListing(client)).find((item) => item.name === SHARED_SYNC_ROUTINE);
    if (row === undefined) return "unavailable";
    return row.effective === true ? "on" : "off";
  } catch {
    return "unavailable";
  }
}

/** Save the whole block. A refused field is named; its value is never returned. */
export async function saveSharedLibrary(client: Client, input: SharedLibraryBlock): Promise<SharedLibrarySaved> {
  let written: Record<string, unknown>;
  try {
    written = await call(client, "office.config-set", { key: "shared", value: { ...input } });
  } catch (error) {
    if (error instanceof ToolRefusal && (error.code === "invalid_library_setting" || error.code === "invalid_argument")) {
      const field = FIELDS.find((name) => error.detail.includes(`shared.${name}`));
      refuse(field);
    }
    throw new SettingsError(502, "shared_library_unavailable");
  }
  if (written.key !== "shared") throw new SettingsError(502, "shared_library_unavailable");
  return { shared: input, routine: await routineState(client) };
}

/** Turn the routine on through Orbit's guarded control, observing its state first. */
export async function enableSharedSync(client: Client): Promise<void> {
  let row: Record<string, unknown> | undefined;
  try { row = (await routineListing(client)).find((item) => item.name === SHARED_SYNC_ROUTINE); }
  catch { throw new SettingsError(502, "routine_unavailable"); }
  if (row === undefined) throw new SettingsError(502, "routine_unavailable");
  if (row.effective === true) return;
  try {
    await call(client, "orbit.routine.control", {
      action: "toggle", name: SHARED_SYNC_ROUTINE, expected_enabled: row.enabled, enabled: true, target: row.target,
    });
  } catch { throw new SettingsError(502, "routine_unavailable"); }
}

function paths(value: unknown): string[] {
  if (!Array.isArray(value) || value.some((item: unknown) => typeof item !== "string")) throw new Error("invalid sync result");
  return value as string[];
}

/**
 * Shared paths as documents: a page is its file; a source or artifact is its
 * folder under `members/<member>/`, with every companion file it carries.
 */
export function documentCount(list: readonly string[]): number {
  return new Set(list.map((path) => {
    const parts = path.split("/");
    return parts[0] === "members" && parts.length > 6 ? parts.slice(0, 6).join("/") : path;
  })).size;
}

/** A document name in words: `dom-client-acme` becomes "Client acme". */
export function documentName(id: string | null, path: string): string {
  const base = id ?? path.split("/").filter((part) => part !== "").at(-1)?.replace(/\.md$/u, "") ?? "";
  const plain = base.replace(/^.*\//u, "").replace(/^(?:dom|rb)-/u, "").replace(/^(?:src|art)-\d{8}-/u, "");
  const words = plain.replace(/[-_.]+/gu, " ").replace(/\s+/gu, " ").trim();
  return words === "" ? "A document" : words.charAt(0).toUpperCase() + words.slice(1);
}

function proposals(value: unknown): number {
  if (!Array.isArray(value) || value.some((item: unknown) => !object(item) || typeof item.page !== "string")) {
    throw new Error("invalid sync result");
  }
  return value.length;
}

function skipped(value: unknown): SharedSkipped[] {
  if (!Array.isArray(value)) throw new Error("invalid sync result");
  return value.map((item: unknown): SharedSkipped => {
    if (!object(item) || (item.phase !== "pull" && item.phase !== "push") || typeof item.path !== "string" ||
        !(item.id === null || typeof item.id === "string") || typeof item.reason !== "string") throw new Error("invalid sync result");
    return { phase: item.phase, name: documentName(item.id, item.path), reason: CODE.test(item.reason) ? item.reason : "refused" };
  });
}

export function projectSync(output: Record<string, unknown>): SharedSync {
  if (output.dry_run !== false || typeof output.synced_at !== "string") throw new Error("invalid sync result");
  return {
    synced_at: output.synced_at,
    shared: documentCount(paths(output.pushed)),
    removed: documentCount(paths(output.removed)),
    received: documentCount(paths(output.pulled)),
    incoming: proposals(output.incoming),
    conflicts: proposals(output.conflicts),
    skipped: skipped(output.skipped),
  };
}

/** The app's memory of its latest sync, shared by Settings and Today. */
export class SharedSyncMemory {
  private latest: SharedSyncOutcome | null = null;
  private running = false;

  get last(): SharedSyncOutcome | null { return this.latest; }

  /** Today's skipped documents were read: keep the counts, drop the list. */
  dismissSkipped(): void {
    if (this.latest?.ok === true) this.latest = { ok: true, sync: { ...this.latest.sync, skipped: [] } };
  }

  /** One sync at a time from the app; the tool's own lock covers routines and agents. */
  async sync(client: Client, now: () => Date = () => new Date()): Promise<SharedSync> {
    if (this.running) throw new SettingsError(409, "sync_failed", { reason: "library_writer_busy" });
    this.running = true;
    try {
      let output: Record<string, unknown>;
      try {
        output = await call(client, "office.library-sync", {});
      } catch (error) {
        const code = error instanceof OrbitTimeoutError ? "sync_timeout" : refusal(error);
        const reason = code !== undefined && SYNC_REASONS.has(code) ? code : "unknown";
        this.latest = { ok: false, at: now().toISOString(), reason };
        throw new SettingsError(409, "sync_failed", { reason });
      }
      let result: SharedSync;
      try { result = projectSync(output); }
      catch {
        this.latest = { ok: false, at: now().toISOString(), reason: "unknown" };
        throw new SettingsError(502, "sync_failed", { reason: "unknown" });
      }
      this.latest = { ok: true, sync: result };
      return result;
    } finally {
      this.running = false;
    }
  }
}

export async function readSharedLibrary(client: Client, memory: SharedSyncMemory): Promise<SharedLibraryStatus> {
  return { shared: await readSharedBlock(client), last_sync: memory.last };
}
