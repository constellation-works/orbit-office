/**
 * Plain-words shaping shared by the Scribe feed and Today's Handoffs
 * section (specs/scribe.md, "Feed"; specs/web-app.md, "Screens"). Both
 * show the same SCR cards: one per project plus the global card. Pure, so
 * it is unit-tested without a browser.
 */
import type { ScrCard } from "@orbit-office/contracts";

/** Where Today's summary links: the Scribe screen's feed. */
export const SCRIBE_FEED_HREF = "#scribe";

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

/** A card's scope in plain words: `Global`, else its project. */
export function scopeLabel(card: ScrCard): string {
  return card.scope === "global" ? "Global" : card.project ?? "No project";
}

/** A card's title, else its id (a v2 SCR always has a title; the id is the fallback). */
export function cardTitle(card: ScrCard): string {
  return card.title ?? card.id;
}

/** One line for a compact list: the Summary, or the withheld notice for a private session. */
export function cardSummary(card: ScrCard): string {
  if (card.withheld) return "Private session: content withheld.";
  return card.summary ?? "";
}

/**
 * The new Tags entry counts a card shows, in words: "2 decisions ·
 * 1 preference", or "No tagged entries". Group names are the SCR's Tags
 * subsections; the two defaults read as their tag names.
 */
export function tagCountsLabel(card: ScrCard): string {
  const names: Record<string, [string, string]> = {
    Decisions: ["decision", "decisions"],
    Preferences: ["preference", "preferences"],
    Requests: ["request", "requests"],
  };
  const parts = Object.entries(card.tag_counts)
    .filter(([, count]) => count > 0)
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([group, count]) => {
      const [one, many] = names[group] ?? [group, group];
      return `${count} ${count === 1 ? one : many}`;
    });
  return parts.length ? parts.join(" · ") : "No tagged entries";
}

/** The `orbit://task/<id>` link an assignment id opens (resolved by Orbit, as the brief's links are). */
export function taskHref(id: string): string {
  return `orbit://task/${encodeURIComponent(id)}`;
}
