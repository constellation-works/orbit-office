/**
 * Scribe (scribe v2; specs/scribe.md, "Feed"; specs/web-app.md,
 * "Screens"): the decision ledger. Top to bottom: an ask box answered by
 * `office.scribe-ask` with citations; the open contradictions (or one quiet
 * line); the Decisions ledger newest first beside the one-card-per-project
 * Feed with its `previous` chain; Preferences and the Agreement view; and
 * the Tags editor. The v1 Pending, Preferences, Landed and Why views and
 * the per-entry feed are retired. Everything polls every ten seconds,
 * except the Tags editor (reads, then saves) and an open session record.
 */
import type { Contradictions, ScribeAnswer, ScribeClaim } from "@orbit-office/contracts";

import { ApiError, postJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { panel, panelBody, panelGrid, screenHead } from "../../ui/index.js";
import { drawContradictions, loadContradictions } from "./contradictions.js";
import { el, resumeRow } from "./dom.js";
import { createFeedView } from "./feed.js";
import { messageFor, notCapturedText } from "./model.js";
import { agreementSection, drawDecisions, loadAgreement, loadDecisions, loadPreferences } from "./panels.js";
import { styles } from "./style.js";
import { createTagsView } from "./tags.js";

/** How often the ledger, the feed and the panels refresh. */
const POLL_MS = 10_000;

const codeOf = (error: unknown): string => error instanceof ApiError ? error.code : "unknown";

/** A panel with a stated line and a body the loaders draw into. */
function section(title: string, id: string): { panel: HTMLElement; stated: HTMLElement; body: HTMLElement } {
  const box = panel(title, { variant: "plain", id });
  const stated = el("p", "", "scribe-stated");
  const body = el("div", "", "scribe-results");
  body.append(el("p", "Loading.", "scribe-status"));
  box.append(panelBody(stated, body));
  return { panel: box, stated, body };
}

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
  page.append(el("style", styles), screenHead("Scribe",
    "The decision ledger. Each meeting adds its decisions here, and Muse Spark checks every new one against what you decided before."));
  root.replaceChildren(page);
  const alive = (): boolean => page.isConnected;

  // Ask the ledger.
  const askForm = el("form", "", "scribe-ask");
  askForm.setAttribute("aria-label", "Ask the ledger");
  const question = el("input");
  question.type = "text";
  question.placeholder = "Did we ever decide how often to rotate the keys?";
  question.setAttribute("aria-label", "Question");
  question.maxLength = 4096;
  const askButton = el("button", "Ask", "primary-action");
  askButton.type = "submit";
  const askRow = el("div", "", "scribe-ask-row");
  askRow.append(question, askButton);
  const answerBox = el("div", "", "scribe-answer");
  answerBox.setAttribute("aria-live", "polite");
  askForm.append(askRow, answerBox);
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

  const conflicts = el("div", "", "scribe-conflicts");
  const decisions = section("Decisions · newest first", "scribe-decisions-h");
  const feedBox = section("Feed · one card per project", "scribe-feed-h");
  const preferences = section("Preferences", "scribe-preferences-h");
  const agreementBox = panel("Agreement", { variant: "plain", id: "scribe-agreement-h" });
  const agreementBody = panelBody();
  agreementBox.append(agreementBody);
  agreementBox.hidden = true;
  const tagsBox = section("Tags", "scribe-tags-h");
  page.append(askForm, conflicts, panelGrid(decisions.panel, feedBox.panel), panelGrid(preferences.panel, agreementBox), tagsBox.panel);

  const feed = createFeedView();
  const tags = createTagsView();

  // The ledger and the contradictions are read together: Contested comes from the open ones.
  let conflictsDrawn = "";
  let ledgerGeneration = 0;
  async function refreshLedger(): Promise<void> {
    const current = ++ledgerGeneration;
    const [ledger, open] = await Promise.allSettled([loadDecisions(), loadContradictions()]);
    if (!alive() || current !== ledgerGeneration) return;
    const found: Contradictions = open.status === "fulfilled" ? open.value : { available: false, contradictions: [] };
    const drawn = JSON.stringify(found);
    // Redraw only on a change, so a pending choice keeps its status.
    if (drawn !== conflictsDrawn) { conflictsDrawn = drawn; drawContradictions(conflicts, found, () => { conflictsDrawn = ""; void refreshLedger(); }); }
    if (ledger.status === "fulfilled") drawDecisions(decisions.body, decisions.stated, ledger.value, found.contradictions);
    else { decisions.stated.textContent = ""; decisions.body.replaceChildren(el("p", messageFor(codeOf(ledger.reason)), "scribe-status")); }
  }

  const generations = new Map<string, number>();
  async function refreshInto(name: string, target: { body: HTMLElement; stated: HTMLElement }, load: () => Promise<(body: HTMLElement, stated: HTMLElement) => void>): Promise<void> {
    const current = (generations.get(name) ?? 0) + 1;
    generations.set(name, current);
    try {
      const draw = await load();
      if (!alive() || current !== generations.get(name)) return;
      draw(target.body, target.stated);
    } catch (error) {
      if (alive() && current === generations.get(name)) { target.stated.textContent = ""; target.body.replaceChildren(el("p", messageFor(codeOf(error)), "scribe-status")); }
    }
  }

  async function refreshAgreement(): Promise<void> {
    const state = await loadAgreement();
    if (!alive()) return;
    const shown = agreementSection(state);
    agreementBox.hidden = shown === null;
    agreementBody.replaceChildren(...(shown === null ? [] : [shown]));
  }

  feed.onChange(() => { void refreshInto("feed", feedBox, feed.load); });

  const refreshAll = (): Promise<unknown> => Promise.all([
    refreshLedger(),
    feed.isOpen() ? Promise.resolve() : refreshInto("feed", feedBox, feed.load),
    refreshInto("preferences", preferences, loadPreferences),
    refreshAgreement(),
  ]);

  const timer = window.setInterval(() => {
    if (!alive()) { window.clearInterval(timer); return; }
    // The Tags editor and an open record hold typed or read state; they are not polled.
    void refreshAll();
  }, POLL_MS);
  await Promise.all([refreshAll(), refreshInto("tags", tagsBox, tags.load)]);
}

export const screen: Screen = { id: "scribe", title: "Scribe", status: "built", render };
