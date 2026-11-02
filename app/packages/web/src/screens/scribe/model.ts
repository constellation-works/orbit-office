/**
 * Plain-words shaping for the Scribe screen: times, refusal messages, and
 * the Tags editor's sink/mode pairing. Pure, so it is unit-tested without
 * a browser.
 */
import type { Contradiction, ContradictionSide, ScribeAgreementCategory, ScribeAnswer, ScribeDecision, ScribeTagDef, ScribeTagMode, ScribeTagScope, ScribeTagSink } from "@orbit-office/contracts";

import { sourceHref } from "../../api/source-location.js";

/** A line's time as written (already in its local zone), without seconds or offset. */
export function timeLabel(time: string): string {
  const match = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/.exec(time);
  return match ? `${match[1]} ${match[2]}` : time;
}

export const TAG_SCOPES: ReadonlyArray<[ScribeTagScope, string]> = [
  ["global", "Global"],
  ["project", "Project"],
  ["session", "This session's scope"],
];

export const TAG_SINKS: ReadonlyArray<[ScribeTagSink, string]> = [
  ["preference_page", "Preference page"],
  ["decisions_ledger", "Decision ledger"],
  ["scr_only", "Session record only"],
  ["orbit_task", "Proposed Orbit task"],
];

export const TAG_MODES: ReadonlyArray<[ScribeTagMode, string]> = [
  ["propose", "Propose first"],
  ["apply", "Apply directly"],
  ["append", "Append as evidence"],
];

/** The modes a sink accepts; any other pair is refused by Office (specs/scribe.md, "Custom tags"). */
export function modesForSink(sink: ScribeTagSink): ScribeTagMode[] {
  if (sink === "preference_page") return ["propose", "apply"];
  if (sink === "orbit_task") return ["propose"];
  return ["append"];
}

/** The mode a sink takes by default. */
export function defaultModeForSink(sink: ScribeTagSink): ScribeTagMode {
  return modesForSink(sink)[0] ?? "append";
}

/** A new tag's starting definition. */
export function blankTag(): ScribeTagDef {
  return { name: "", scope: "session", instruction: "", sink: "scr_only", mode: "append", capture: "markers" };
}

/** The two shipped default tag names. */
export const DEFAULT_TAG_NAMES: readonly string[] = ["preferences", "decisions"];

/** A fraction as a whole percent, or an em dash when the readout has none. */
export function percentLabel(value: number | null): string {
  return value === null ? "—" : `${Math.round(value * 100)}%`;
}

/** Accuracy with its Wilson 95% interval, e.g. `81% (68–90%)`. */
export function accuracyLabel(category: ScribeAgreementCategory): string {
  if (category.accuracy === null) return "—";
  if (category.accuracy_wilson_low === null || category.accuracy_wilson_high === null) return percentLabel(category.accuracy);
  return `${percentLabel(category.accuracy)} (${percentLabel(category.accuracy_wilson_low)}–${percentLabel(category.accuracy_wilson_high)})`;
}

/** Below `min_samples` a category shows "Not enough decisions yet" instead of percentages. */
export function hasEnoughSamples(category: ScribeAgreementCategory): boolean {
  return category.scored_requests >= category.min_samples;
}

/** The category in words: workspace and kind, underscores as spaces. */
export function categoryLabel(category: ScribeAgreementCategory): string {
  const workspace = category.workspace ?? "Global";
  const kind = category.kind.replaceAll("_", " ");
  return `${workspace} · ${kind}`;
}

/** Where a ledger decision stands: one side of an open contradiction, reversed, or standing. */
export type LedgerState = "contested" | "reversed" | "standing";

/**
 * Whether `decision` is `side` of a contradiction: by its ledger entry id
 * when the side names one, else by the entry id in its cite, or by the same
 * evidence source and the same decision text.
 */
export function isSide(decision: ScribeDecision, side: ContradictionSide): boolean {
  if (side.entry !== undefined) return side.entry === decision.id;
  const [source = "", anchor = ""] = side.cite.split("#", 2);
  if (source === decision.id || anchor === decision.id) return true;
  if (source !== decision.source) return false;
  return side.text === decision.quote || side.text === decision.chosen;
}

export function ledgerState(decision: ScribeDecision, open: readonly Contradiction[]): LedgerState {
  if (open.some((item) => isSide(decision, item.newer) || isSide(decision, item.prior))) return "contested";
  if (decision.reversed_by !== null || !decision.current) return "reversed";
  return "standing";
}

/** The ledger newest first by `decided_at`, then by id, without changing the input. */
export function newestFirst(decisions: readonly ScribeDecision[]): ScribeDecision[] {
  return [...decisions].sort((a, b) => b.decided_at.localeCompare(a.decided_at) || b.id.localeCompare(a.id));
}

/** A `src-…` cite opens the Source screen at its anchor; any other cite has no link. */
export function citeHref(cite: string): string | null {
  const match = /^(src-[a-z0-9]+(?:-[a-z0-9]+)*)(#.+)?$/.exec(cite);
  if (match === null || match[1] === undefined) return null;
  return sourceHref(match[1], match[2] ?? "#source");
}

/** The label on a choice: "<decision> stands", or the side's place in time when the text is long. */
export function choiceLabel(side: ContradictionSide, which: "newer" | "prior"): string {
  const text = side.text.trim().replace(/[.]$/, "");
  if (text.length > 0 && text.length <= 48) return `${text} stands`;
  return which === "newer" ? "The newer decision stands" : "The earlier decision stands";
}

/** When and where a side was decided, in words. */
export function sideWhen(side: ContradictionSide): string {
  const parts = [side.decided_at === null ? null : timeLabel(side.decided_at), side.context].filter((part): part is string => part !== null && part !== "");
  return parts.length ? parts.join(" · ") : "When and where are not recorded";
}

/** "Holds 1 agent to-do", or null when it holds none. */
export function heldLabel(held: number): string | null {
  if (held <= 0) return null;
  return `Holds ${held} agent ${held === 1 ? "to-do" : "to-dos"}`;
}

/** The quiet line shown when no contradiction is open. */
export function noContradictionText(available: boolean): string {
  return available ? "No open contradictions." : "Couldn’t read open contradictions. We’ll try again shortly.";
}

const CODES: Record<string, string> = {
  agreement_unavailable: "Could not read Orbit's measurement.",
  question_required: "Write a question first.",
  model_unavailable: "The model is unavailable right now. The feed and panels still work.",
  network_error: "The app server is not reachable. Check that it is running.",
  scribe_scr_not_found: "That session record is no longer there.",
  not_permitted: "The app isn't allowed to change the scribe settings yet. Run the command below instead; nothing was saved.",
  invalid_library_setting: "That tag definition isn't allowed. Check the name, and that the mode fits where its entries go.",
  unknown_library_setting: "That setting isn't one the scribe has.",
  tag_sink_refused: "A tag's entries can only go to a Library page, a ledger, the session record, or a proposed task.",
  invalid_input: "That didn't fit the form. Check it and try again.",
  not_available: "Resolving arrives with meeting decisions in the ledger. Nothing was changed.",
};

/** A refusal or failure in plain words; the code itself is never shown. */
export function messageFor(code: string): string {
  return CODES[code] ?? "That did not finish. Try again in a moment.";
}

/** "Not captured", with the searched range and topics. */
export function notCapturedText(answer: ScribeAnswer): string {
  const range = answer.since || answer.until ? `${answer.since ?? "the start"} to ${answer.until ?? "now"}` : "every date";
  const files = `${answer.files_searched} ${answer.files_searched === 1 ? "file" : "files"}`;
  const topics = answer.topics.length ? `; topics ${answer.topics.join(", ")}` : "";
  return `Not captured. Searched ${range} across ${files}${topics}.`;
}
