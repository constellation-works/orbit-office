/**
 * Plain-words shaping for the Scribe screen: times, refusal messages, and
 * the Tags editor's sink/mode pairing. Pure, so it is unit-tested without
 * a browser.
 */
import type { ScribeAnswer, ScribeTagDef, ScribeTagMode, ScribeTagScope, ScribeTagSink } from "@orbit-office/contracts";

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

const CODES: Record<string, string> = {
  question_required: "Write a question first.",
  model_unavailable: "The model is unavailable right now. The feed and panels still work.",
  network_error: "The app server is not reachable. Check that it is running.",
  scribe_scr_not_found: "That session record is no longer there.",
  not_permitted: "The app isn't allowed to change the scribe settings yet. Run the command below instead; nothing was saved.",
  invalid_library_setting: "That tag definition isn't allowed. Check the name, and that the mode fits where its entries go.",
  unknown_library_setting: "That setting isn't one the scribe has.",
  tag_sink_refused: "A tag's entries can only go to a Library page, a ledger, the session record, or a proposed task.",
  invalid_input: "That didn't fit the form. Check it and try again.",
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
