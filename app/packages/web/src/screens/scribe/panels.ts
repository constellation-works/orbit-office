/**
 * The Preferences and Decisions panels (specs/scribe.md, "Feed" and
 * "Answering"): read-only views over `office.scribe-query` modes
 * `preferences` and `decisions`, replacing the retired v1 canned views.
 */
import type { ScribeAgreementCategory, ScribeAgreementResponse, ScribeDecision, ScribeDecisionAgreement, ScribeDecisions, ScribePreference, ScribePreferences } from "@orbit-office/contracts";

import { ApiError, getJson } from "../../api/client.js";
import { pill } from "../../ui/index.js";
import { el } from "./dom.js";
import { accuracyLabel, categoryLabel, hasEnoughSamples, messageFor, percentLabel, timeLabel } from "./model.js";

/** The Agreement view is polled at most every 60 seconds. */
export const AGREEMENT_POLL_MS = 60_000;

type AgreementState =
  | { kind: "hidden" }
  | { kind: "ready"; agreement: ScribeDecisionAgreement }
  | { kind: "failed"; code: string };

let agreementCache: { at: number; state: AgreementState } | null = null;

/** Test seam: forget the throttled agreement read. */
export function resetAgreementCache(): void { agreementCache = null; }

async function loadAgreement(now: number = Date.now()): Promise<AgreementState> {
  if (agreementCache !== null && now - agreementCache.at < AGREEMENT_POLL_MS) return agreementCache.state;
  let state: AgreementState;
  try {
    const result = await getJson<ScribeAgreementResponse>("/api/scribe/decision-agreement");
    state = result.agreement === null ? { kind: "hidden" } : { kind: "ready", agreement: result.agreement };
  } catch (error) {
    state = { kind: "failed", code: error instanceof ApiError ? error.code : "agreement_unavailable" };
  }
  agreementCache = { at: now, state };
  return state;
}

function agreementCategoryItem(category: ScribeAgreementCategory): HTMLLIElement {
  const item = el("li", "", "scribe-entry scribe-agreement-row");
  item.dataset.category = category.category;
  const head = el("div", "", "scribe-entry-head");
  head.append(el("span", categoryLabel(category)), pill(category.mode === "shadow" ? "Shadow" : "Enabled"));
  item.append(head);
  const list = el("dl", "", "scribe-fields");
  list.append(el("dt", "mode"), el("dd", category.mode));
  list.append(el("dt", "scored requests"), el("dd", String(category.scored_requests)));
  if (!hasEnoughSamples(category)) {
    list.append(el("dt", "accuracy"), el("dd", "Not enough decisions yet"));
  } else {
    list.append(
      el("dt", "accuracy"), el("dd", accuracyLabel(category)),
      el("dt", "coverage"), el("dd", percentLabel(category.coverage)),
      el("dt", "calibration error"), el("dd", percentLabel(category.calibration_error)),
      el("dt", "abstention"), el("dd", percentLabel(category.abstention_rate)),
      el("dt", "conflicts"), el("dd", percentLabel(category.conflict_rate)),
    );
  }
  item.append(list);
  return item;
}

/** The read-only Agreement section, or null when it is hidden. */
export function agreementSection(state: AgreementState): HTMLElement | null {
  if (state.kind === "hidden") return null;
  const section = el("section", "", "scribe-panel scribe-agreement");
  section.setAttribute("aria-label", "Agreement");
  section.append(el("h3", "Agreement"));
  if (state.kind === "failed") {
    section.append(el("p", messageFor(state.code === "unknown" ? "agreement_unavailable" : state.code), "scribe-status"));
    return section;
  }
  const { agreement } = state;
  const window = agreement.window_start || agreement.window_end
    ? `${agreement.window_start ?? "the start"} to ${agreement.window_end ?? "now"}` : null;
  const meta: string[] = [];
  if (window) meta.push(`Window ${window}`);
  if (agreement.generated_at) meta.push(`Read ${timeLabel(agreement.generated_at)}`);
  meta.push(`Mode ${agreement.consumer_mode}`);
  section.append(el("p", meta.join(" · "), "scribe-stated"));
  if (!agreement.categories.length) {
    section.append(el("p", "No decisions yet.", "scribe-status"));
    return section;
  }
  const list = el("ul", "", "scribe-list");
  for (const category of agreement.categories) list.append(agreementCategoryItem(category));
  section.append(list);
  return section;
}

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

/** The decision ledgers, newest first, with the Agreement section when it is visible. */
export async function loadDecisions(): Promise<(panel: HTMLElement, stated: HTMLElement) => void> {
  const [result, agreement] = await Promise.all([
    getJson<ScribeDecisions>("/api/scribe/decisions"),
    loadAgreement(),
  ]);
  return (panel, stated) => {
    stated.textContent = result.decisions.length
      ? `${result.decisions.length} recorded ${result.decisions.length === 1 ? "decision" : "decisions"}, newest first.`
      : "No recorded decisions.";
    const parts: HTMLElement[] = [];
    const section = agreementSection(agreement);
    if (section !== null) parts.push(section);
    const list = el("ul", "", "scribe-list");
    for (const decision of result.decisions) list.append(decisionItem(decision));
    parts.push(result.decisions.length ? list : el("p", "No recorded decisions.", "scribe-status"));
    panel.replaceChildren(...parts);
  };
}
