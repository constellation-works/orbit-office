/**
 * The Preferences and Decisions panels (specs/scribe.md, "Feed" and
 * "Answering"): read-only views over `office.scribe-query` modes
 * `preferences` and `decisions`, replacing the retired v1 canned views.
 * The decision ledger marks each entry Contested, Reversed or Standing;
 * Contested comes from the open contradictions (contradictions.ts).
 */
import type { Contradiction, ScribeAgreementCategory, ScribeAgreementResponse, ScribeDecision, ScribeDecisionAgreement, ScribeDecisions, ScribePreference, ScribePreferences } from "@orbit-office/contracts";

import { ApiError, getJson } from "../../api/client.js";
import { monoMeta, pill, row, rows, type Tone } from "../../ui/index.js";
import { el } from "./dom.js";
import { accuracyLabel, categoryLabel, citeHref, hasEnoughSamples, ledgerState, type LedgerState, messageFor, newestFirst, percentLabel, timeLabel } from "./model.js";

/** The Agreement view is polled at most every 60 seconds. */
export const AGREEMENT_POLL_MS = 60_000;

export type AgreementState =
  | { kind: "hidden" }
  | { kind: "ready"; agreement: ScribeDecisionAgreement }
  | { kind: "failed"; code: string };

let agreementCache: { at: number; state: AgreementState } | null = null;

/** Test seam: forget the throttled agreement read. */
export function resetAgreementCache(): void { agreementCache = null; }

export async function loadAgreement(now: number = Date.now()): Promise<AgreementState> {
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
  // The Agreement panel around it carries the heading and the region name.
  const section = el("div", "", "scribe-agreement");
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

/** A dim line: `label` in words, then the value. */
function labelled(label: string, value: string, className = "scribe-line"): HTMLElement {
  const line = el("span", "", className);
  line.append(el("span", `${label} `, "scribe-line-label"), el("span", value));
  return line;
}

function preferenceItem(preference: ScribePreference): HTMLLIElement {
  const details: Node[] = [
    el("span", `${scopeName(preference.scope)} · ${preference.matter}`, "scribe-line"),
    labelled("Stated", `${preference.stated_at} · review by ${preference.review_by}`),
    labelled("Evidence", preference.cites.join(", ")),
  ];
  if (preference.proposed.length) details.push(labelled("Proposed change", preference.proposed.map((item) => item.entry).join(", ")));
  details.push(monoMeta(preference.citation));
  const item = row(preference.statement, {
    details,
    trail: preference.stale ? [pill(`Stale · review by ${preference.review_by}`, "attention")] : [],
  });
  item.classList.add("scribe-row-item");
  item.dataset.citation = preference.citation;
  return item;
}

/** The current preference pages. */
export async function loadPreferences(): Promise<(panel: HTMLElement, stated: HTMLElement) => void> {
  const result = await getJson<ScribePreferences>("/api/scribe/preferences");
  return (panel, stated) => {
    stated.textContent = result.preferences.length
      ? `Current preferences, today ${result.today}. A new one on the same matter replaces it.`
      : "";
    panel.replaceChildren(result.preferences.length
      ? rows(result.preferences.map(preferenceItem), "Preferences")
      : el("p", "No preferences recorded.", "scribe-status"));
  };
}

const STATES: Record<LedgerState, [string, Tone]> = {
  contested: ["Contested", "attention"],
  reversed: ["Reversed", "neutral"],
  standing: ["Standing", "neutral"],
};

function decisionItem(decision: ScribeDecision, open: readonly Contradiction[]): HTMLLIElement {
  const state = ledgerState(decision, open);
  const [label, tone] = STATES[state];
  const trail: Node[] = [pill(label, tone)];
  if (decision.private) trail.push(pill("Private", "private"));
  const time = el("time", timeLabel(decision.decided_at));
  time.dateTime = decision.decided_at;
  const meta = el("span", "", "scribe-line");
  meta.append(time, document.createTextNode(` · ${scopeName(decision.scope)} · by ${decision.decided_by}`));
  const details: Node[] = [meta];
  let title: string;
  if (decision.private || decision.quote === null) {
    title = "Private session: cited by reference only.";
  } else {
    title = decision.chosen ?? decision.quote;
    if (decision.question) details.push(labelled("On", decision.question));
    if (decision.chosen && decision.chosen !== decision.quote) details.push(el("blockquote", decision.quote, "scribe-quote"));
    details.push(labelled("Why:", decision.because ?? "no recorded reason"));
    if (decision.reverses) details.push(labelled("Reverses", decision.reverses));
  }
  if (decision.reversed_by) details.push(labelled("Reversed by", decision.reversed_by));
  const href = citeHref(decision.source);
  const cite = el("span", "", "scribe-cite");
  const code = el("code", decision.id);
  code.title = "Decision ledger entry";
  cite.append(code, monoMeta(decision.source, href ?? undefined));
  details.push(cite);
  const item = row(title, { details, trail });
  item.classList.add("scribe-row-item");
  item.dataset.id = decision.id;
  item.dataset.state = state;
  return item;
}

/** The decision ledger: entries newest first, each marked against the open contradictions. */
export async function loadDecisions(): Promise<ScribeDecision[]> {
  const result = await getJson<ScribeDecisions>("/api/scribe/decisions");
  return newestFirst(result.decisions);
}

export function drawDecisions(panel: HTMLElement, stated: HTMLElement, decisions: readonly ScribeDecision[], open: readonly Contradiction[]): void {
  stated.textContent = decisions.length
    ? `${decisions.length} recorded ${decisions.length === 1 ? "decision" : "decisions"}.`
    : "";
  panel.replaceChildren(decisions.length
    ? rows(decisions.map((decision) => decisionItem(decision, open)), "Decisions")
    : el("p", "No recorded decisions.", "scribe-status"));
}
