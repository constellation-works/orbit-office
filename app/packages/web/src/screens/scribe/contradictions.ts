/**
 * The Open contradiction panel: a new decision and the prior ledger
 * decision it contradicts, side by side, with the agent to-dos it holds and
 * a choice of which stands. Read from `GET /api/scribe/contradictions` and
 * resolved through `POST /api/scribe/contradictions/resolve`; both answer
 * "not available" until ORB-21340 installs the contradiction flow, and the
 * screen then shows one quiet line instead of the panel.
 */
import type { Contradiction, Contradictions, ContradictionSide } from "@orbit-office/contracts";

import { ApiError, getContradictions, resolveContradiction } from "../../api/client.js";
import { monoMeta, pill } from "../../ui/index.js";
import { el } from "./dom.js";
import { choiceLabel, citeHref, heldLabel, messageFor, noContradictionText, sideWhen } from "./model.js";

/** The open contradictions, or an unavailable read when the route fails. */
export async function loadContradictions(): Promise<Contradictions> {
  try {
    return await getContradictions();
  } catch {
    return { available: false, contradictions: [] };
  }
}

function sideCard(side: ContradictionSide, which: "newer" | "prior"): HTMLElement {
  const card = el("div", "", "scribe-conflict-side");
  card.dataset.side = which;
  card.append(
    el("span", sideWhen(side), "scribe-conflict-when"),
    el("p", side.text, "scribe-conflict-text"),
    el("span", side.because ? `Why: ${side.because}` : "Why: no recorded reason.", "scribe-conflict-why"),
    monoMeta(side.cite, citeHref(side.cite) ?? undefined),
  );
  return card;
}

let panels = 0;

function contradictionPanel(item: Contradiction, resolved: () => void): HTMLElement {
  const section = el("section", "", "scribe-conflict");
  const headingId = `scribe-conflict-${++panels}`;
  section.setAttribute("aria-labelledby", headingId);
  const head = el("div", "", "scribe-conflict-head");
  const heading = el("h2", `Open contradiction · ${item.matter}`);
  heading.id = headingId;
  head.append(heading);
  const held = heldLabel(item.held);
  if (held !== null) head.append(pill(held, "attention"));
  const sides = el("div", "", "scribe-conflict-sides");
  sides.append(sideCard(item.newer, "newer"), sideCard(item.prior, "prior"));
  const choices = el("div", "", "choice-row");
  const status = el("p", "", "scribe-stated");
  status.setAttribute("role", "status");
  const buttons: HTMLButtonElement[] = [];
  for (const which of ["newer", "prior"] as const) {
    const side = item[which];
    const choice = el("button", choiceLabel(side, which), "choice");
    choice.type = "button";
    choice.setAttribute("aria-pressed", "false");
    choice.setAttribute("aria-label", `${side.text} stands`);
    choice.addEventListener("click", () => {
      for (const other of buttons) { other.disabled = true; other.setAttribute("aria-pressed", String(other === choice)); }
      status.textContent = "Recording your choice.";
      void resolveContradiction({ key: item.key, stands: which })
        .then(() => {
          if (!section.isConnected) return;
          status.replaceChildren(el("span", "Recorded. The agent to-dos it held are released.", "callout-done"));
          resolved();
        })
        .catch((error: unknown) => {
          if (!section.isConnected) return;
          for (const other of buttons) { other.disabled = false; other.setAttribute("aria-pressed", "false"); }
          status.textContent = messageFor(error instanceof ApiError ? error.code : "unknown");
        });
    });
    buttons.push(choice);
    choices.append(choice);
  }
  section.append(head, sides, choices, status);
  return section;
}

/** The contradiction panels, or one quiet line when none is open. */
export function drawContradictions(slot: HTMLElement, result: Contradictions, resolved: () => void): void {
  if (result.contradictions.length === 0) {
    slot.replaceChildren(el("p", noContradictionText(result.available), "scribe-stated scribe-conflict-none"));
    return;
  }
  slot.replaceChildren(...result.contradictions.map((item) => contradictionPanel(item, resolved)));
}
