/**
 * Scribe (scribe v2; specs/scribe.md, "Feed"; specs/web-app.md,
 * "Screens"): the one-card-per-project feed with its `previous` chain,
 * the Preferences and Decisions panels, the Tags editor, and an ask box
 * answered by `office.scribe-ask` with citations. The v1 Pending,
 * Preferences, Landed and Why views and the per-entry feed are retired.
 * Polls every ten seconds, except the Tags editor (reads, then saves) and
 * an open session record.
 */
import type { ScribeAnswer, ScribeClaim } from "@orbit-office/contracts";

import { ApiError, postJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { el, resumeRow } from "./dom.js";
import { createFeedView } from "./feed.js";
import { messageFor, notCapturedText } from "./model.js";
import { loadDecisions, loadPreferences } from "./panels.js";
import { styles } from "./style.js";
import { createTagsView } from "./tags.js";

/** How often the feed and the panels refresh. */
const POLL_MS = 10_000;

type Tab = "feed" | "preferences" | "decisions" | "tags";
const TABS: ReadonlyArray<[Tab, string]> = [["feed", "Feed"], ["preferences", "Preferences"], ["decisions", "Decisions"], ["tags", "Tags"]];

const codeOf = (error: unknown): string => error instanceof ApiError ? error.code : "unknown";

/** The citation anchor: `SCR file:line`, the message id, and a copyable resume command. */
function citation(fileLine: string | null, msg: string | null, resume: string | null): HTMLElement {
  const row = el("div", "", "scribe-cite");
  if (fileLine) { const code = el("code", fileLine); code.title = "SCR file:line"; row.append(code); }
  if (msg) row.append(el("span", msg));
  if (resume) row.append(...resumeRow(resume));
  return row;
}

function claimItem(claim: ScribeClaim): HTMLElement {
  const item = el("li", "", "scribe-claim");
  if (claim.withheld) item.append(el("p", "A private line answers this; it is cited by reference only."));
  else {
    if (claim.text) item.append(el("p", claim.text));
    if (claim.quote) item.append(el("blockquote", claim.quote, "scribe-quote"));
  }
  item.append(citation(claim.file_line, claim.msg, claim.resume));
  return item;
}

function drawAnswer(target: HTMLElement, answer: ScribeAnswer): void {
  const parts: HTMLElement[] = [];
  if (answer.status === "answered") {
    const list = el("ul", "", "scribe-list");
    for (const claim of answer.claims) list.append(claimItem(claim));
    parts.push(list);
  } else if (answer.status === "reference_only") {
    parts.push(el("p", "Only private lines match. Without a private model route they are cited by reference only:", "scribe-status"));
    const list = el("ul", "", "scribe-list");
    for (const reference of answer.references) { const li = el("li", "", "scribe-cite"); li.append(el("code", reference)); list.append(li); }
    parts.push(list);
  } else parts.push(el("p", notCapturedText(answer), "scribe-status"));
  const notes: string[] = [];
  if (answer.dropped > 0) notes.push(`${answer.dropped} ${answer.dropped === 1 ? "quote was" : "quotes were"} dropped: not found on the cited line.`);
  if (answer.model) notes.push(`Answered by ${answer.model}.`);
  if (notes.length) parts.push(el("p", notes.join(" "), "scribe-stated"));
  target.replaceChildren(...parts);
}

async function render(root: HTMLElement): Promise<void> {
  const page = el("section", "", "scribe-page");
  page.append(el("style", styles), el("h1", "Scribe"),
    el("p", "The latest session record per project, plus the global one. Turn the scribe on in a session with /scribe on; nothing is captured automatically.", "scribe-lede"));
  root.replaceChildren(page);
  const alive = (): boolean => page.isConnected;

  // Ask box.
  const askForm = el("form", "", "scribe-panel");
  askForm.setAttribute("aria-label", "Ask the scribe");
  const question = el("input");
  question.placeholder = "Did Daniel decide on the release?";
  question.setAttribute("aria-label", "Question");
  question.maxLength = 4096;
  const askButton = el("button", "Ask");
  askButton.type = "submit";
  const askRow = el("div", "", "scribe-row");
  askRow.append(question, askButton);
  const answerBox = el("div", "", "scribe-answer");
  answerBox.setAttribute("aria-live", "polite");
  askForm.append(el("h2", "Ask"), askRow, answerBox);
  askForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!question.value.trim()) { answerBox.replaceChildren(el("p", messageFor("question_required"), "scribe-status")); return; }
    askButton.disabled = true;
    answerBox.replaceChildren(el("p", "Searching the scribe, then asking the model about the shortlist.", "scribe-status"));
    void postJson<ScribeAnswer>("/api/scribe/ask", { question: question.value.trim() })
      .then((answer) => { if (alive()) drawAnswer(answerBox, answer); })
      .catch((error) => { if (alive()) answerBox.replaceChildren(el("p", messageFor(codeOf(error)), "scribe-status")); })
      .finally(() => { askButton.disabled = false; });
  });

  // Tabs: the feed, the two panels, and the Tags editor.
  const tabs = el("div", "", "scribe-tabs");
  tabs.setAttribute("role", "tablist");
  tabs.setAttribute("aria-label", "Scribe views");
  const stated = el("p", "", "scribe-stated");
  const panel = el("div", "", "scribe-results");
  panel.setAttribute("role", "tabpanel");
  const feed = createFeedView();
  const tags = createTagsView();
  let tab: Tab = "feed";
  const buttons = new Map<Tab, HTMLButtonElement>();
  for (const [name, title] of TABS) {
    const button = el("button", title);
    button.type = "button";
    button.setAttribute("role", "tab");
    button.addEventListener("click", () => { tab = name; drawTabs(); void refresh(true); });
    buttons.set(name, button);
    tabs.append(button);
  }
  const drawTabs = (): void => {
    for (const [name, button] of buttons) button.setAttribute("aria-selected", String(name === tab));
  };
  feed.onChange(() => { if (tab === "feed") void refresh(false); });

  page.append(askForm, tabs, stated, panel);
  drawTabs();

  let generation = 0;
  async function refresh(loading: boolean): Promise<void> {
    const current = ++generation; const showing = tab;
    if (loading) { panel.replaceChildren(el("p", "Loading.", "scribe-status")); stated.textContent = ""; }
    try {
      const draw = showing === "feed" ? await feed.load()
        : showing === "preferences" ? await loadPreferences()
        : showing === "decisions" ? await loadDecisions()
        : await tags.load();
      if (!alive() || current !== generation) return;
      draw(panel, stated);
    } catch (error) {
      if (alive() && current === generation) panel.replaceChildren(el("p", messageFor(codeOf(error)), "scribe-status"));
    }
  }

  const timer = window.setInterval(() => {
    if (!alive()) { window.clearInterval(timer); return; }
    // The editor and an open record hold typed or read state; leave them be.
    if (tab === "tags") return;
    if (tab === "feed" && feed.isOpen()) return;
    void refresh(false);
  }, POLL_MS);
  await refresh(true);
}

export const screen: Screen = { id: "scribe", title: "Scribe", status: "built", render };
