/** Today: the user's next steps, live agent work and openable outcomes. */
import type { Today, TodayItem, TodayNeed, TodayResult } from "@orbit-office/contracts";

import { getJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { card, emptyState, errorState, pill, provenanceLink } from "../../ui/index.js";

function paragraph(text: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.textContent = text;
  return element;
}

function background(item: TodayItem): HTMLElement {
  const container = document.createElement("div");
  const link = provenanceLink("About this work", "#today");
  link.dataset.focus = `${item.key}-background`;
  link.setAttribute("aria-expanded", "false");
  const detail = paragraph(item.background || "No background has been added yet.");
  detail.hidden = true;
  detail.className = "today-background";
  link.addEventListener("click", (event) => {
    event.preventDefault();
    detail.hidden = !detail.hidden;
    link.setAttribute("aria-expanded", String(!detail.hidden));
  });
  container.append(link, detail);
  return container;
}

function needCard(item: TodayNeed): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  const labels = { proposal: "A suggestion", approval: "Ready for your review", todo: "For you" };
  content.append(pill(labels[item.kind]), background(item));
  if (item.kind !== "todo") {
    const slot = document.createElement("div");
    slot.dataset.slot = "approval-actions";
    slot.dataset.key = item.key;
    content.append(slot);
  }
  return card(item.title, content);
}

function resultCard(item: TodayResult): HTMLElement {
  const content = document.createElement("div");
  content.className = "card-content";
  const result = document.createElement("details");
  result.dataset.key = item.key;
  const summary = document.createElement("summary");
  summary.dataset.focus = `${item.key}-result`;
  summary.textContent = "Open result";
  const outcome = paragraph(item.result ?? "No written result was recorded for this work.");
  outcome.className = "today-result";
  result.append(summary, outcome);
  content.append(result, background(item));
  return card(item.title, content);
}

function section<T extends TodayItem>(title: string, items: readonly T[], empty: string, draw: (item: T) => HTMLElement): HTMLElement {
  const element = document.createElement("section");
  element.className = "today-section";
  const heading = document.createElement("h2");
  heading.textContent = title;
  element.setAttribute("aria-label", title);
  const header = document.createElement("div");
  header.className = "today-section-heading";
  header.append(heading, pill(String(items.length)));
  element.append(header);
  if (items.length === 0) {
    element.append(emptyState(empty, "This space will update as your work moves forward."));
  } else {
    const list = document.createElement("div");
    list.className = "card-grid";
    list.append(...items.map(draw));
    element.append(list);
  }
  return element;
}

function drawToday(snapshot: Today): HTMLElement {
  const body = document.createElement("div");
  const needs = section("Needs you", snapshot.needs_you, "Nothing needs your attention.", needCard);
  const working = section("Agents working on", snapshot.agents_working_on, "No agent work is underway.", (item) => {
    const line = document.createElement("article");
    line.className = "today-working-line";
    const title = document.createElement("h3");
    title.textContent = item.title;
    line.append(title, background(item));
    return line;
  });
  working.classList.add("today-working");
  const done = section("Done today", snapshot.done_today, "No completed work yet today.", resultCard);
  body.append(needs, working, done);
  if (snapshot.truncated || snapshot.completion_dates_unavailable > 0) {
    const notice = paragraph([
      snapshot.truncated ? "Showing a limited selection of your work." : "",
      snapshot.completion_dates_unavailable > 0 ? "Some finished work has no completion date and cannot be placed today." : "",
    ].filter(Boolean).join(" "));
    notice.className = "intro";
    body.append(notice);
  }
  return body;
}

export interface TodayScreenOptions {
  load?: () => Promise<Today>;
  /** Poll after the last request settles, so requests never overlap. */
  pollIntervalMs?: number;
}

/** The production screen and browser tests use the same renderer and polling lifecycle. */
export function createTodayScreen({ load = () => getJson<Today>("/api/today"), pollIntervalMs = 5_000 }: TodayScreenOptions = {}): Screen {
  return {
    id: "today",
    title: "Today",
    status: "built",
    async render(root) {
      const view = document.createElement("div");
      view.className = "today";
      const heading = document.createElement("h1");
      heading.textContent = "Today";
      const intro = paragraph("Your next steps and the work moving forward.");
      intro.className = "intro";
      const uploadSlot = document.createElement("div");
      uploadSlot.dataset.slot = "drop-zone";
      const feedback = document.createElement("div");
      feedback.setAttribute("aria-live", "polite");
      const content = document.createElement("div");
      content.append(paragraph("Gathering your work…"));
      content.setAttribute("aria-busy", "true");
      view.append(heading, intro, uploadSlot, feedback, content);
      root.replaceChildren(view);
      let previous: string | undefined;

      async function refresh(): Promise<void> {
        if (!root.contains(view) || !view.isConnected) return;
        try {
          const snapshot = await load();
          if (!root.contains(view) || !view.isConnected) return;
          const serialized = JSON.stringify(snapshot);
          if (serialized !== previous) {
            const open = new Set(Array.from(content.querySelectorAll<HTMLDetailsElement>("details[open]"), (detail) => detail.dataset.key));
            const expanded = new Set(Array.from(content.querySelectorAll<HTMLElement>('[aria-expanded="true"]'), (link) => link.dataset.focus));
            const focused = content.contains(document.activeElement)
              ? (document.activeElement as HTMLElement).dataset.focus : undefined;
            content.replaceChildren(drawToday(snapshot));
            for (const detail of content.querySelectorAll<HTMLDetailsElement>("details")) {
              detail.open = open.has(detail.dataset.key);
            }
            for (const link of content.querySelectorAll<HTMLElement>("[data-focus]")) {
              if (expanded.has(link.dataset.focus)) link.click();
              if (focused !== undefined && link.dataset.focus === focused) link.focus();
            }
            previous = serialized;
          }
          feedback.replaceChildren();
        } catch {
          if (!root.contains(view) || !view.isConnected) return;
          feedback.replaceChildren(errorState("Couldn’t refresh your work", previous === undefined
            ? "Please try again. We’ll keep checking for your work."
            : "The work below may have changed. We’ll try again shortly."));
          if (previous === undefined) content.replaceChildren();
        } finally {
          content.setAttribute("aria-busy", "false");
          // Navigation replaces view. A pending read never redraws another screen.
          if (root.contains(view) && view.isConnected) window.setTimeout(() => { void refresh(); }, pollIntervalMs);
        }
      }
      await refresh();
    },
  };
}

export const screen: Screen = createTodayScreen();
