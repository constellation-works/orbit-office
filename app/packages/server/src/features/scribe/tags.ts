/**
 * The Tags editor's settings (specs/scribe.md, "Configuration"): the
 * effective tags come from `office.status`; a save (create, edit or delete
 * one tag, or restore the defaults) goes through `office.config-set` with
 * a `scribe.*` key. Office owns the definition rules; this file only
 * checks the closed request shape before any tool call.
 *
 * Until Orbit grants the app `office.config-set` for the `scribe.*` keys
 * (ORB-20363), Orbit refuses the save for the app's authority. That refusal
 * is `not_permitted` and carries the equivalent operator command; it is
 * never reported as a saved tag.
 */
import type { ScribeTagDef, ScribeTagSave, ScribeTags } from "@orbit-office/contracts";

import { outputOf, ScribeError } from "./model.js";

type Row = Record<string, unknown>;

const NAME = /^[a-z][a-z0-9-]{0,31}$/;
const SCOPES = ["global", "project", "session"] as const;
const SINKS = ["decisions_ledger", "preference_page", "scr_only", "orbit_task"] as const;
const MODES = ["propose", "apply", "append"] as const;
const CAPTURES = ["markers", "markers+inferred"] as const;
const TAG_KEYS = ["name", "scope", "instruction", "sink", "mode", "capture", "workspace"] as const;

const record = (value: unknown): Row | null =>
  value !== null && typeof value === "object" && !Array.isArray(value) ? value as Row : null;
const text = (value: unknown): string | null => typeof value === "string" ? value : null;

function tagOf(value: unknown): ScribeTagDef | null {
  const row = record(value);
  if (row === null) return null;
  const name = text(row.name); const instruction = text(row.instruction);
  const scope = SCOPES.find((item) => item === row.scope);
  const sink = SINKS.find((item) => item === row.sink);
  const mode = MODES.find((item) => item === row.mode);
  const capture = CAPTURES.find((item) => item === row.capture);
  if (name === null || instruction === null || scope === undefined || sink === undefined || mode === undefined || capture === undefined) return null;
  const workspace = text(row.workspace);
  return { name, scope, instruction, sink, mode, capture, ...(workspace !== null ? { workspace } : {}) };
}

/** The effective `scribe` block of an `office.status` output. */
export function tagsOf(output: Row): ScribeTags {
  const scribe = record(output.scribe);
  const cadence = record(scribe?.cadence);
  if (scribe === null || cadence === null || !Array.isArray(scribe.tags)
    || typeof cadence.minutes !== "number" || typeof cadence.turns !== "number") {
    throw new ScribeError(502, "unavailable");
  }
  const tags = scribe.tags.map(tagOf);
  if (tags.some((tag) => tag === null)) throw new ScribeError(502, "unavailable");
  return {
    tags: tags.filter((tag): tag is ScribeTagDef => tag !== null),
    cadence: { minutes: cadence.minutes, turns: cadence.turns },
    tags_customized: scribe.tags_customized === true,
  };
}

/**
 * One editor save, checked before any tool call: the closed body only.
 * `office.config-set` owns the semantic rules (sink and mode pairs, name
 * form, workspaces); a shape Office would refuse is still sent to it so
 * the refusal it names is the one the editor shows.
 */
export function tagSaveOf(body: unknown): ScribeTagSave {
  const row = record(body);
  if (row === null) throw new ScribeError(400, "invalid_input");
  if (row.action === "restore") {
    if (Object.keys(row).some((key) => key !== "action")) throw new ScribeError(400, "invalid_input");
    return { action: "restore" };
  }
  if (row.action === "delete") {
    if (Object.keys(row).some((key) => key !== "action" && key !== "name")) throw new ScribeError(400, "invalid_input");
    if (typeof row.name !== "string" || !NAME.test(row.name)) throw new ScribeError(400, "invalid_input");
    return { action: "delete", name: row.name };
  }
  if (row.action === "upsert") {
    if (Object.keys(row).some((key) => key !== "action" && key !== "tag")) throw new ScribeError(400, "invalid_input");
    const tag = record(row.tag);
    if (tag === null || Object.keys(tag).some((key) => !(TAG_KEYS as readonly string[]).includes(key))) {
      throw new ScribeError(400, "invalid_input");
    }
    const checked = tagOf(tag);
    if (checked === null || !NAME.test(checked.name)
      || checked.instruction.trim() === "" || checked.instruction.length > 2000
      || (checked.workspace !== undefined && (checked.workspace === "" || checked.workspace.length > 256))) {
      throw new ScribeError(400, "invalid_input");
    }
    return { action: "upsert", tag: checked };
  }
  throw new ScribeError(400, "invalid_input");
}

/** The `office.config-set` input one save writes. */
export function configSetInput(save: ScribeTagSave): Row {
  if (save.action === "restore") return { key: "scribe.tags", value: null };
  if (save.action === "delete") return { key: `scribe.tags.${save.name}`, value: null };
  const { name, ...definition } = save.tag;
  return { key: `scribe.tags.${name}`, value: { name, ...definition } };
}

/** The command an operator runs for the same save, shown while Orbit refuses the app's own save. */
export function operatorCommand(save: ScribeTagSave): string {
  return `orbit tool run office.config-set --input '${JSON.stringify(configSetInput(save))}'`;
}

/** Read the effective tags back (after a save, so the editor shows what Office holds). */
export function tagsFromStatus(value: unknown): ScribeTags {
  return tagsOf(outputOf(value));
}
