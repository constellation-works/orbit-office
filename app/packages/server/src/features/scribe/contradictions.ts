/**
 * Open contradictions between a meeting decision and the prior ledger
 * (`GET /api/scribe/contradictions`, `POST /api/scribe/contradictions/resolve`).
 *
 * `office.extract` files each contradiction as an open `owner:you` to-do
 * tagged `office-scribe-contradictions` whose description lists both entries
 * (`- <entry> (<decided at>): <chosen> [<cite>]`) and ends with
 * `Conflicting entries: <a> <b>`; the agent to-dos it holds are `owner:you`
 * + `contested` with `Held until <to-do> is resolved.` A resolve calls
 * `office.decision-resolve` with the to-do and the entry that stands.
 */
import type { Contradiction, Contradictions, ContradictionSide } from "@orbit-office/contracts";

import type { OrbitToolClient } from "../../orbit/client.js";
import { ScribeError } from "./model.js";

type Client = Pick<OrbitToolClient, "callTool" | "pollTool">;
type Row = Record<string, unknown>;

const CONFLICT_TAG = "office-scribe-contradictions";
const HELD_TAG = "contested";
/** The open to-do statuses, as `office.decision-resolve` reads them. */
const OPEN = ["proposed", "backlog", "in-progress", "review", "blocked", "someday"];
const ENTRY = /^[A-Za-z0-9._:-]{1,160}$/;
const SIDE_LINE = /^- (\S+) \(([^)]*)\): (.*) \[([^\]]*)\]$/;

const record = (value: unknown): Row | null =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Row : null;
const text = (value: unknown): string | null => typeof value === "string" && value.length > 0 ? value : null;

/** An exec envelope's output, or the value itself. */
function outputOf(value: unknown): Row {
  const row = record(value);
  if (row === null) throw new ScribeError(502, "unavailable");
  if (row.ok === false) throw new ScribeError(502, "unavailable");
  return row.ok === true ? record(row.output) ?? {} : row;
}

async function openTasks(client: Client, tags: string[]): Promise<Row[]> {
  const listed = outputOf(await client.pollTool("orbit.task.list", {
    tag: tags, status: OPEN, limit: 200, fields: ["id", "title", "status", "tags", "description"],
  }));
  if (!Array.isArray(listed.tasks)) throw new ScribeError(502, "unavailable");
  return listed.tasks.flatMap((task: unknown): Row[] => { const row = record(task); return row === null ? [] : [row]; });
}

/** The two entries a conflict to-do names, and what its description says of each. */
function conflictOf(task: Row): { key: string; pair: [string, string]; sides: Map<string, ContradictionSide> } | null {
  const key = text(task.id); const description = text(task.description);
  if (key === null || description === null) return null;
  const lines = description.split("\n").map((line) => line.trim()).filter((line) => line.length > 0);
  const last = lines.at(-1)?.match(/^Conflicting entries: (\S+) (\S+)$/);
  if (!last || last[1] === undefined || last[2] === undefined || last[1] === last[2]) return null;
  if (!ENTRY.test(last[1]) || !ENTRY.test(last[2])) return null;
  const sides = new Map<string, ContradictionSide>();
  for (const line of lines) {
    const match = SIDE_LINE.exec(line);
    if (match?.[1] === undefined) continue;
    sides.set(match[1], {
      entry: match[1], text: match[3] ?? "", because: null, cite: match[4] ?? "",
      decided_at: text(match[2]), context: null,
    });
  }
  return { key, pair: [last[1], last[2]], sides };
}

/** The ledger entries by id, for the stated reasons; an unreadable ledger leaves them out. */
async function ledger(client: Client): Promise<Map<string, Row>> {
  try {
    const output = outputOf(await client.pollTool("office.scribe-query", { mode: "decisions" }));
    const rows = Array.isArray(output.decisions) ? output.decisions : [];
    return new Map(rows.flatMap((row: unknown) => {
      const entry = record(row); const id = entry === null ? null : text(entry.id);
      return entry === null || id === null ? [] : [[id, entry] as [string, Row]];
    }));
  } catch {
    return new Map();
  }
}

function side(id: string, described: ContradictionSide | undefined, entry: Row | undefined): ContradictionSide {
  const because = record(entry?.because);
  return {
    entry: id,
    text: described?.text || text(entry?.chosen) || text(entry?.quote) || id,
    because: because === null ? null : text(because.text),
    cite: described?.cite || text(entry?.source) || id,
    decided_at: described?.decided_at ?? text(entry?.decided_at),
    context: text(entry?.question),
  };
}

/** The matter the description names (`… an earlier decision about <matter>. Agent work …`), else the newer decision. */
function matterOf(task: Row, newer: ContradictionSide): string {
  const named = /an earlier decision about (.+?)\. Agent work/.exec(text(task.description) ?? "")?.[1];
  return named ?? newer.text;
}

/** The open contradictions, newer side first; a failed to-do read answers `available: false`. */
export async function readContradictions(client: Client): Promise<Contradictions> {
  let conflicts: Row[];
  let held: Row[];
  try {
    [conflicts, held] = await Promise.all([openTasks(client, ["owner:you", CONFLICT_TAG]), openTasks(client, ["owner:you", HELD_TAG])]);
  } catch {
    return { available: false, contradictions: [] };
  }
  const parsed = conflicts.flatMap((task) => {
    const conflict = conflictOf(task);
    return conflict === null ? [] : [{ task, ...conflict }];
  });
  const entries = parsed.length === 0 ? new Map<string, Row>() : await ledger(client);
  const contradictions: Contradiction[] = parsed.map(({ task, key, pair, sides }) => {
    const [a, b] = pair.map((id) => side(id, sides.get(id), entries.get(id))) as [ContradictionSide, ContradictionSide];
    const aFirst = (a.decided_at ?? "") >= (b.decided_at ?? "");
    const newer = aFirst ? a : b; const prior = aFirst ? b : a;
    const until = `Held until ${key} is resolved.`;
    const holds = held.filter((todo) => text(todo.description)?.split("\n").some((line) => line.trim() === until)).length;
    return { key, matter: matterOf(task, newer), newer, prior, held: holds };
  });
  return { available: true, contradictions };
}

/** Record which side stands through `office.decision-resolve`; the side is read again from the open to-do. */
export async function resolveContradiction(client: Client, body: unknown): Promise<{ ok: true }> {
  const input = record(body);
  const key = input === null ? null : text(input.key);
  const stands = input?.stands;
  if (input === null || key === null || !ENTRY.test(key) || (stands !== "newer" && stands !== "prior")
    || Object.keys(input).some((name) => name !== "key" && name !== "stands")) {
    throw new ScribeError(400, "invalid_input");
  }
  const open = await readContradictions(client);
  if (!open.available) throw new ScribeError(502, "unavailable");
  const item = open.contradictions.find((contradiction) => contradiction.key === key);
  if (item === undefined) throw new ScribeError(404, "not_found");
  const entry = (stands === "newer" ? item.newer : item.prior).entry;
  if (entry === undefined) throw new ScribeError(404, "not_found");
  const reply = record(await client.callTool("office.decision-resolve", { task: key, stands: entry }));
  if (reply?.ok === false) {
    const error = record(reply.error);
    throw new ScribeError(422, text(error?.code) ?? "refused");
  }
  return { ok: true };
}
