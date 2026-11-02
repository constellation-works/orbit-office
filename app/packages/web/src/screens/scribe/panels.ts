/**
 * The Preferences and Decisions panels (specs/scribe.md, "Feed" and
 * "Answering"): read-only views over `office.scribe-query` modes
 * `preferences` and `decisions`, replacing the retired v1 canned views.
 */
import type { ScribeDecision, ScribeDecisions, ScribePreference, ScribePreferences } from "@orbit-office/contracts";

import { getJson } from "../../api/client.js";
import { pill } from "../../ui/index.js";
import { el } from "./dom.js";
import { timeLabel } from "./model.js";

function scopeName(scope: string): string {
  return scope === "global" ? "Global" : scope.replace(/^project:/, "");
}

function preferenceItem(preference: ScribePreference): HTMLLIElement {
  const item = el("li", "", "scribe-entry");
  item.dataset.citation = preference.citation;
  const head = el("div", "", "scribe-entry-head");
  const scope = pill(scopeName(preference.scope));
  scope.classList.add("scribe-chip");
  head.append(scope, el("span", preference.matter));
  if (preference.stale) head.append(pill(`Stale · review by ${preference.review_by}`, "attention"));
  item.append(head, el("blockquote", preference.statement, "scribe-quote"));
  const list = el("dl", "", "scribe-fields");
  list.append(
    el("dt", "stated"), el("dd", preference.stated_at),
    el("dt", "review by"), el("dd", preference.review_by),
    el("dt", "evidence"), el("dd", preference.cites.join(", ")),
  );
  if (preference.proposed.length) {
    list.append(el("dt", "proposed change"), el("dd", preference.proposed.map((row) => row.entry).join(", ")));
  }
  const cite = el("div", "", "scribe-cite");
  cite.append(el("code", preference.citation));
  item.append(list, cite);
  return item;
}

/** The current preference pages. */
export async function loadPreferences(): Promise<(panel: HTMLElement, stated: HTMLElement) => void> {
  const result = await getJson<ScribePreferences>("/api/scribe/preferences");
  return (panel, stated) => {
    stated.textContent = result.preferences.length
      ? `Current preferences, today ${result.today}. A new one on the same matter replaces it.`
      : "No preferences recorded.";
    const list = el("ul", "", "scribe-list");
    for (const preference of result.preferences) list.append(preferenceItem(preference));
    panel.replaceChildren(result.preferences.length ? list : el("p", "No preferences recorded.", "scribe-status"));
  };
}

function decisionItem(decision: ScribeDecision): HTMLLIElement {
  const item = el("li", "", "scribe-entry");
  item.dataset.id = decision.id;
  const head = el("div", "", "scribe-entry-head");
  const time = el("time", timeLabel(decision.decided_at));
  time.dateTime = decision.decided_at;
  const scope = pill(scopeName(decision.scope));
  scope.classList.add("scribe-chip");
  head.append(time, scope, el("span", `by ${decision.decided_by}`));
  if (!decision.current && decision.reversed_by) head.append(pill(`Reversed by ${decision.reversed_by}`));
  if (decision.private) head.append(pill("Private", "private"));
  item.append(head);
  if (decision.private || decision.quote === null) {
    item.append(el("p", "Private session: cited by reference only.", "scribe-status"));
  } else {
    if (decision.question) item.append(el("p", decision.question, "scribe-card-summary"));
    item.append(el("blockquote", decision.quote, "scribe-quote"));
    const list = el("dl", "", "scribe-fields");
    if (decision.chosen) list.append(el("dt", "chosen"), el("dd", decision.chosen));
    list.append(el("dt", "why"), el("dd", decision.because ?? "no recorded reason"));
    if (decision.reverses) list.append(el("dt", "reverses"), el("dd", decision.reverses));
    item.append(list);
  }
  const cite = el("div", "", "scribe-cite");
  const code = el("code", decision.id);
  code.title = "Decision ledger entry";
  cite.append(code, el("span", decision.source));
  item.append(cite);
  return item;
}

/** The decision ledgers, newest first. */
export async function loadDecisions(): Promise<(panel: HTMLElement, stated: HTMLElement) => void> {
  const result = await getJson<ScribeDecisions>("/api/scribe/decisions");
  return (panel, stated) => {
    stated.textContent = result.decisions.length
      ? `${result.decisions.length} recorded ${result.decisions.length === 1 ? "decision" : "decisions"}, newest first.`
      : "No recorded decisions.";
    const list = el("ul", "", "scribe-list");
    for (const decision of result.decisions) list.append(decisionItem(decision));
    panel.replaceChildren(result.decisions.length ? list : el("p", "No recorded decisions.", "scribe-status"));
  };
}
