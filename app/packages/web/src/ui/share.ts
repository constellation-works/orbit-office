/**
 * Sharing with the team (docs/design/office/specs/shared-library.md, "App"):
 * the "Shared · from Alice" and "Shared" labels, and the **Share with the
 * team** switch a page, source or artifact view carries. The switch calls
 * `office.share-set` through the app server; stopping is confirmed first,
 * and a citation refusal offers **Share these too**.
 */
import type { ShareDocument, SharedSync, ShareRequest, ShareResult } from "@orbit-office/contracts";

import { ApiError, shareDocument } from "../api/client.js";
import { pill } from "./pill.js";

/** A finished sync in counts of documents: "Shared 4, received 12, 1 needs you". */
export function syncCounts(sync: Pick<SharedSync, "shared" | "removed" | "received" | "incoming" | "conflicts" | "skipped">): string {
  const needs = sync.incoming + sync.conflicts + sync.skipped.length;
  const parts = [`Shared ${sync.shared}`, `received ${sync.received}`];
  if (sync.removed > 0) parts.push(`stopped sharing ${sync.removed}`);
  parts.push(needs === 0 ? "nothing needs you" : `${needs} ${needs === 1 ? "needs" : "need"} you`);
  return parts.join(", ");
}

/** Why a sync failed, in plain words. The remote itself is never repeated. */
export function syncFailure(reason: string): string {
  switch (reason) {
    case "shared_not_configured": return "No shared library is connected yet. Save the remote, branch and your name first.";
    case "shared_remote_unreachable": return "The shared library couldn't be reached. Check your network and that your git sign-in for it works, then try again.";
    case "shared_push_rejected": return "A colleague shared changes at the same moment, so yours weren't sent. Sync again.";
    case "shared_layout_invalid": return "That repository isn't laid out as a shared library. Check the remote and branch.";
    case "shared_identity_missing": return "Git doesn't know your name and email on this computer, so nothing could be shared. Set them in git, then sync again.";
    case "shared_storage_failed": return "This computer's copy of the shared library couldn't be updated. Try again.";
    case "library_changed": return "Your Library changed while syncing. Sync again.";
    case "library_writer_busy": return "Another sync or Library change is running. Try again in a moment.";
    case "sync_timeout": return "The sync took too long and was stopped. Try again.";
    default: return "The sync didn't finish. Try again.";
  }
}

/** Why a sync left one document out, in plain words. */
export function skippedReason(reason: string): string {
  switch (reason) {
    case "share_private_refused": return "It’s marked private, so it stays on this computer.";
    case "share_cites_unshared": return "It cites a document you haven’t shared. Share that one too.";
    case "shared_file_linked": return "It’s a link to another file, which the shared library doesn’t take.";
    case "shared_file_special": return "It isn’t an ordinary file.";
    case "shared_file_unreadable": return "It isn’t readable text.";
    case "shared_path_outside_layout": return "It’s in a place the shared library doesn’t use.";
    case "shared_document_invalid": return "It belongs to a document that isn’t complete.";
    case "invalid_frontmatter": return "Its header can’t be read.";
    default: return "Your Library’s rules didn’t accept it.";
  }
}

/** A member name in words: `alice-smith` reads "Alice Smith". */
export function memberName(member: string): string {
  return member.split("-").filter((part) => part !== "").map((part) => part.charAt(0).toUpperCase() + part.slice(1)).join(" ");
}

/**
 * The label a document carries in lists and views: a colleague's shows who
 * shared it, the user's own shareable one says "Shared", others none.
 */
export function sharedLabel(sharedBy: string | null | undefined, shareable: boolean | undefined): HTMLElement | null {
  if (typeof sharedBy === "string" && sharedBy !== "") {
    const label = pill(`Shared · from ${memberName(sharedBy)}`);
    label.classList.add("pill--shared");
    return label;
  }
  if (shareable !== true) return null;
  const label = pill("Shared");
  label.classList.add("pill--shared");
  return label;
}

export interface ShareSwitchOptions {
  /** The user's own document. */
  id: string;
  /** What the view calls it in a sentence ("this page", "this source"). */
  noun: string;
  shareable: boolean;
  /** A private source can't be shared; the switch says so instead. */
  private?: boolean;
  /** Called after the Library changed, with the new value. */
  onChange?: (shareable: boolean) => void;
  share?: (input: ShareRequest) => Promise<ShareResult>;
}

function button(label: string, primary = false): HTMLButtonElement {
  const element = document.createElement("button");
  element.type = "button";
  element.textContent = label;
  if (primary) element.dataset.primary = "true";
  return element;
}

function paragraph(text: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.textContent = text;
  return element;
}

/** A refusal in plain words; never a code, an id or a path. */
function refusalWords(error: unknown, noun: string): string {
  if (!(error instanceof ApiError)) return "Sharing didn't change. Try again.";
  if (error.status === 0) return "The app server can't be reached. Sharing didn't change.";
  switch (error.reason) {
    case "share_private_refused": return `${capitalized(noun)} is marked private, so it can't be shared. Mark it normal first.`;
    case "document_not_found": return `${capitalized(noun)} is no longer in your Library.`;
    case "library_writer_busy": return "The Library is busy with another change. Try again in a moment.";
    case "invalid_frontmatter": return `${capitalized(noun)} has a citation the Library can't read, so it can't be shared yet.`;
    default: return "Sharing didn't change. Try again.";
  }
}

function capitalized(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/** The **Share with the team** switch, its hint, and a panel for confirmations and refusals. */
export function shareSwitch(options: ShareSwitchOptions): HTMLElement {
  const share = options.share ?? shareDocument;
  let shareable = options.shareable;
  const root = document.createElement("section");
  root.className = "share";
  root.setAttribute("aria-label", "Sharing");
  const label = document.createElement("label");
  label.className = "share-switch";
  const input = document.createElement("input");
  input.type = "checkbox";
  input.setAttribute("role", "switch");
  const text = document.createElement("span");
  text.textContent = "Share with the team";
  label.append(input, text);
  const hint = document.createElement("p");
  hint.className = "share-hint";
  const panel = document.createElement("div");
  panel.className = "share-panel";
  panel.setAttribute("role", "status");
  panel.setAttribute("aria-live", "polite");
  root.append(label, hint, panel);

  function draw(): void {
    input.checked = shareable;
    input.disabled = options.private === true && !shareable;
    hint.textContent = options.private === true && !shareable
      ? "Private documents stay on this computer and can't be shared."
      : shareable
      ? "Shared with the team: colleagues get it on their next sync."
      : "Only you have it. Share it, and colleagues get it on their next sync.";
  }

  function busy(on: boolean): void {
    input.disabled = on || (options.private === true && !shareable);
    for (const element of panel.querySelectorAll("button")) element.disabled = on;
    root.toggleAttribute("aria-busy", on);
  }

  function failed(words: string): void {
    panel.dataset.tone = "danger";
    panel.replaceChildren(paragraph(words));
  }

  function offerCited(documents: ShareDocument[]): void {
    panel.dataset.tone = "attention";
    const list = document.createElement("ul");
    for (const cited of documents) {
      const item = document.createElement("li");
      item.textContent = cited.title;
      list.append(item);
    }
    const also = button("Share these too", true);
    const cancel = button("Not now");
    const actions = document.createElement("div");
    actions.className = "share-actions";
    actions.append(also, cancel);
    panel.replaceChildren(
      paragraph(`${capitalized(options.noun)} cites ${documents.length === 1 ? "a document" : "documents"} you haven't shared, so colleagues couldn't open ${documents.length === 1 ? "it" : "them"}:`),
      list,
      actions,
    );
    also.addEventListener("click", () => {
      void apply(true, documents.map((cited) => cited.id));
    });
    cancel.addEventListener("click", () => { panel.replaceChildren(); });
    also.focus();
  }

  function confirmStop(): void {
    panel.dataset.tone = "attention";
    const stop = button("Stop sharing", true);
    const keep = button("Keep sharing");
    const actions = document.createElement("div");
    actions.className = "share-actions";
    actions.append(stop, keep);
    panel.replaceChildren(
      paragraph(`Stop sharing ${options.noun}? It leaves the shared library on your next sync. Colleagues who already synced keep their copy, and the shared library's history keeps it too.`),
      actions,
    );
    stop.addEventListener("click", () => { void apply(false); });
    keep.addEventListener("click", () => { panel.replaceChildren(); input.focus(); });
    stop.focus();
  }

  async function apply(next: boolean, also?: string[]): Promise<void> {
    busy(true);
    try {
      await share(also === undefined ? { id: options.id, shareable: next } : { id: options.id, shareable: next, also });
      shareable = next;
      delete panel.dataset.tone;
      panel.replaceChildren(paragraph(next
        ? also === undefined ? "Shared. It goes to the team on your next sync." : `Shared, with ${also.length === 1 ? "the document it cites" : `the ${also.length} documents it cites`}. They go to the team on your next sync.`
        : "Stopped sharing. It leaves the shared library on your next sync."));
      options.onChange?.(next);
    } catch (error) {
      if (error instanceof ApiError && error.reason === "share_cites_unshared" && error.documents.length > 0 && next) {
        offerCited(error.documents);
        return;
      }
      failed(refusalWords(error, options.noun));
    } finally {
      busy(false);
      draw();
    }
  }

  input.addEventListener("change", () => {
    const next = input.checked;
    // The switch shows the Library's value until the change is saved.
    input.checked = shareable;
    if (next) void apply(true);
    else confirmStop();
  });

  draw();
  return root;
}
