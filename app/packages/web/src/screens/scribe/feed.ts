/**
 * The Scribe feed (specs/scribe.md, "Feed"): exactly one card per project
 * (its latest SCR) plus one global card. A card shows the title, scope,
 * when it was updated, Summary, Next, Outcome and Blockers and the new
 * Tags entry counts. Opening a card shows the whole SCR; its Previous
 * button walks the `previous:` chain one SCR at a time — the only way to
 * reach an older SCR. There is no list of past sessions.
 */
import type { ScrCard, ScrDetail, ScrFeed } from "@orbit-office/contracts";

import { ApiError, getJson } from "../../api/client.js";
import { ageLabel, cardSummary, cardTitle, scopeLabel, tagCountsLabel } from "../../ui/handoff.js";
import { pill, renderMarkdown } from "../../ui/index.js";
import { el } from "./dom.js";
import { messageFor, timeLabel } from "./model.js";

const codeOf = (error: unknown): string => error instanceof ApiError ? error.code : "unknown";

function sectionList(title: string, items: readonly string[]): HTMLElement | null {
  if (items.length === 0) return null;
  const box = el("div", "", "scribe-card-section");
  const list = el("ul", "", "scribe-card-items");
  for (const item of items) list.append(el("li", item));
  box.append(el("h4", title), list);
  return box;
}

/** One feed card: title and age, then scope, Summary, Next, Outcome and the tag counts. `open` shows the whole SCR. */
export function cardItem(card: ScrCard, now: Date, open: (card: ScrCard) => void): HTMLElement {
  const item = el("article", "", "scribe-card");
  item.dataset.id = card.id;
  const top = el("div", "", "scribe-card-top");
  const title = el("button", cardTitle(card), "scribe-card-title");
  title.type = "button";
  title.setAttribute("aria-label", `Open ${cardTitle(card)}`);
  title.addEventListener("click", () => open(card));
  const age = el("time", ageLabel(card.updated, now), "scribe-card-age");
  age.dateTime = card.updated;
  age.title = timeLabel(card.updated);
  top.append(title, age);
  const head = el("div", "", "scribe-entry-head");
  const scope = pill(scopeLabel(card));
  scope.classList.add("scribe-chip");
  head.append(scope, el("span", `Updated ${timeLabel(card.updated)}`, "scribe-line"));
  if (card.withheld) head.append(pill("Private", "private"));
  item.append(top, head);
  if (card.withheld) {
    item.append(el("p", cardSummary(card), "scribe-status"));
    return item;
  }
  if (card.summary) item.append(renderMarkdown(card.summary, { className: "scribe-card-summary", headingLevel: 5 }));
  for (const section of [sectionList("Next", card.next), sectionList("Outcome and Blockers", card.outcome)]) {
    if (section) item.append(section);
  }
  item.append(el("p", tagCountsLabel(card), "scribe-card-counts"));
  return item;
}

/** The whole SCR: every section and its Tags entries, with Previous and Back. */
function detailItem(detail: ScrDetail, now: Date, back: () => void, previous: (id: string) => void): HTMLElement {
  const { scr } = detail;
  const item = el("article", "", "scribe-card scribe-detail");
  item.dataset.id = scr.id;
  const head = el("div", "", "scribe-entry-head");
  const scope = pill(scopeLabel(scr));
  scope.classList.add("scribe-chip");
  const time = el("time", `Updated ${timeLabel(scr.updated)} · ${ageLabel(scr.updated, now)}`);
  time.dateTime = scr.updated;
  head.append(scope, time);
  if (scr.withheld) head.append(pill("Private", "private"));
  item.append(head, el("h3", cardTitle(scr)));
  if (scr.withheld) {
    item.append(el("p", cardSummary(scr), "scribe-status"));
  } else {
    if (scr.summary) {
      const box = el("div", "", "scribe-card-section");
      box.append(el("h4", "Summary"), renderMarkdown(scr.summary, { className: "scribe-card-summary", headingLevel: 5 }));
      item.append(box);
    }
    for (const section of [
      sectionList("Goals", scr.goals),
      sectionList("Outcome and Blockers", scr.outcome),
      sectionList("Next", scr.next),
    ]) if (section) item.append(section);
    if (scr.tags.length) {
      const box = el("div", "", "scribe-card-section");
      box.append(el("h4", "Tags"));
      const list = el("ul", "", "scribe-list");
      for (const entry of scr.tags) {
        const row = el("li", "", "scribe-tag-entry");
        const meta = el("div", "", "scribe-cite");
        const code = el("code", entry.citation);
        code.title = "SCR Tags entry";
        meta.append(code, el("span", `${entry.group} · ${entry.by} · ${timeLabel(entry.time)}`));
        row.append(el("blockquote", entry.quote, "scribe-quote"), meta);
        list.append(row);
      }
      box.append(list);
      item.append(box);
    }
  }
  const cite = el("div", "", "scribe-cite");
  cite.append(el("code", scr.path), el("span", scr.id));
  item.append(cite);
  const actions = el("div", "", "scribe-actions");
  const backButton = el("button", "Back to the feed");
  backButton.type = "button";
  backButton.addEventListener("click", back);
  actions.append(backButton);
  if (scr.previous !== null) {
    const id = scr.previous;
    const previousButton = el("button", "Previous");
    previousButton.type = "button";
    previousButton.setAttribute("aria-label", `Previous session record, ${id}`);
    previousButton.addEventListener("click", () => previous(id));
    actions.append(previousButton);
  }
  item.append(actions);
  return item;
}

/**
 * The feed tab: `load` fetches the cards (or the open SCR's chain step) and
 * returns a draw function for the shared panel. While an SCR is open a poll
 * does not redraw it; Previous and Back ask for a redraw through `onChange`.
 */
export function createFeedView(): {
  load: () => Promise<(panel: HTMLElement, stated: HTMLElement) => void>;
  onChange: (listener: () => void) => void;
  isOpen: () => boolean;
} {
  let detail: ScrDetail | null = null;
  let changed = (): void => {};
  const open = (card: ScrCard): void => {
    detail = { scr: card, previous: null };
    changed();
  };
  const load = async (): Promise<(panel: HTMLElement, stated: HTMLElement) => void> => {
    if (detail !== null) {
      const showing = detail;
      return (panel, stated) => {
        stated.textContent = "One session record. Previous walks to the record it replaced.";
        panel.replaceChildren(detailItem(showing, new Date(), () => { detail = null; changed(); }, (id) => {
          void getJson<ScrDetail>(`/api/scribe/feed?id=${encodeURIComponent(id)}`)
            .then((next) => { detail = next; changed(); })
            .catch((error: unknown) => {
              stated.textContent = messageFor(codeOf(error));
            });
        }));
      };
    }
    const feed: ScrFeed = await getJson<ScrFeed>("/api/scribe/feed");
    return (panel, stated) => {
      const now = new Date();
      stated.textContent = feed.cards.length
        ? `${feed.cards.length} ${feed.cards.length === 1 ? "card" : "cards"}: the latest session record per project, plus the global one.`
        : "";
      panel.replaceChildren(...(feed.cards.length
        ? feed.cards.map((card) => cardItem(card, now, open))
        : [el("p", "Nothing scribed yet. Turn the scribe on in a session with /scribe on.", "scribe-status")]));
    };
  };
  return { load, onChange: (listener) => { changed = listener; }, isOpen: () => detail !== null };
}
