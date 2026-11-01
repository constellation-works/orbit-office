/**
 * Plain-words shaping shared by Scribe › Handoffs and Today's handoff
 * summary (specs/web-app.md, "Screens"). Pure, so it is unit-tested without
 * a browser.
 */
import type { ScribeHandoff } from "@orbit-office/contracts";

/** Where Today's summary links: the Scribe screen opened on Handoffs. */
export const HANDOFFS_HREF = "?scribe=handoffs#scribe";

/** How long ago `time` was, at `now`: "just now", "5 min ago", "3 h ago", "2 d ago". */
export function ageLabel(time: string, now: Date): string {
  const at = Date.parse(time);
  if (Number.isNaN(at)) return time;
  const minutes = Math.floor((now.getTime() - at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours} h ago`;
  return `${Math.floor(hours / 24)} d ago`;
}

/** A thread's display name: its project, else "No project". */
export function projectLabel(handoff: ScribeHandoff): string {
  return handoff.project ?? "No project";
}

/** One line for a compact list: the objective, the legacy text, or a withheld notice. */
export function handoffSummary(handoff: ScribeHandoff): string {
  if (handoff.withheld) return "Private session: content withheld.";
  if (handoff.fields) return handoff.fields.objective;
  return handoff.text;
}

/** The `orbit://task/<id>` link an assignment id opens (resolved by Orbit, as the brief's links are). */
export function taskHref(id: string): string {
  return `orbit://task/${encodeURIComponent(id)}`;
}

/** A thread filter value in plain words: `task:ORB-1` reads `ORB-1`. */
export function threadLabel(thread: string): string {
  if (thread.startsWith("task:")) return thread.slice("task:".length);
  const match = /^project:(.*) orchestrator:(.*)$/.exec(thread);
  if (!match) return thread;
  return [match[1] || "No project", match[2] || "no orchestrator"].join(" · ");
}

/** Group handoffs by project, keeping the input order (newest first) inside and between groups. */
export function byProject(handoffs: readonly ScribeHandoff[]): Array<{ project: string; handoffs: ScribeHandoff[] }> {
  const groups = new Map<string, ScribeHandoff[]>();
  for (const handoff of handoffs) {
    const key = projectLabel(handoff);
    const group = groups.get(key);
    if (group) group.push(handoff); else groups.set(key, [handoff]);
  }
  return [...groups].map(([project, list]) => ({ project, handoffs: list }));
}
