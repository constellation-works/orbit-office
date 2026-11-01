/**
 * Plain-words shaping for the Scribe screen: labels, the note tag a text
 * suggests, and messages for refusal codes. Pure, so it is unit-tested
 * without a browser.
 */
import type { ScribeAnswer, ScribeSession, ScribeView } from "@orbit-office/contracts";

/** Tags Daniel's own note may take under the default scribe rules, in menu order. */
export const NOTE_TAGS = ["fact", "todo", "decision", "direction", "question", "reminder", "preference"] as const;

export const VIEW_TITLES: Record<ScribeView, string> = {
  pending: "Pending",
  pref: "Preferences",
  landed: "Landed",
  why: "Why",
};

/** A line's time as written (already in its local zone), without seconds or offset. */
export function timeLabel(time: string): string {
  const match = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/.exec(time);
  return match ? `${match[1]} ${match[2]}` : time;
}

export function sessionTitle(session: ScribeSession): string {
  return session.title ?? "Untitled session";
}

/** Project and counterpart, when known. */
export function sessionMeta(session: ScribeSession): string {
  const count = `${session.entries.length} ${session.entries.length === 1 ? "line" : "lines"}`;
  return [session.project, session.counterpart_name, count].filter((part): part is string => !!part).join(" · ");
}

/** The tag a leading `#tag` names, if any. */
export function leadingTag(text: string): string | null {
  const match = /^#([a-z0-9][a-z0-9._-]{0,63})(?:\s|$)/.exec(text.trim());
  return match ? match[1] ?? null : null;
}

/**
 * The tag a note's words suggest, model-free: a leading `#tag` wins, then a
 * few plain cues; otherwise `fact`. The menu shows it and Daniel can change it.
 */
export function suggestedTag(text: string): string {
  const leading = leadingTag(text);
  if (leading !== null) return leading;
  const words = text.trim().toLowerCase();
  if (/^(decision|decided|we decided|let'?s go with)\b/.test(words)) return "decision";
  if (/^(i prefer|prefer|always|never)\b/.test(words)) return "preference";
  if (/\bremind\b|\bremember to\b/.test(words)) return "reminder";
  if (/^(todo|to do|i need to|need to|i have to|i must)\b/.test(words)) return "todo";
  if (words.endsWith("?")) return "question";
  return "fact";
}

const CODES: Record<string, string> = {
  no_cursor: "This session has no scribe cursor yet. Run /scribe in it once, then add the note.",
  scribe_not_parsed: "This session has not been parsed on this computer. Run /scribe in it, then add the note.",
  tag_not_permitted: "That tag is for agents. Pick one of your own tags.",
  unknown_tag: "That tag is not in the scribe rules.",
  unknown_topic: "That topic is not in the scribe rules.",
  unknown_matter: "That matter is not in the scribe rules.",
  duplicate: "That note is already recorded.",
  note_text_required: "Write the note first.",
  tag_required: "Pick a tag for the note.",
  question_required: "Write a question first.",
  model_unavailable: "The model is unavailable right now. The canned views still work.",
  network_error: "The app server is not reachable. Check that it is running.",
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

/** The empty line for a canned view. */
export function emptyView(view: ScribeView, about: string | null, timeZone: string, today: string): string {
  if (view === "pending") return "Nothing pending.";
  if (view === "pref") return about ? `No preference recorded on ${about}.` : "No preferences recorded.";
  if (view === "landed") return `Nothing landed today (${today}, ${timeZone}).`;
  return about ? `No recorded decision on ${about}.` : "No recorded decisions.";
}
