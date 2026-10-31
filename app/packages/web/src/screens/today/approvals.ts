/**
 * The Needs-you actions on Today: approve or send back reviewed work, keep or
 * dismiss a script's files, accept or dismiss proposals (alone or a whole
 * card), settle the shared library's incoming updates and conflicts, and
 * approve or dismiss a request to another system. Messages are plain words;
 * tool codes are never shown.
 */
import type {
  AcceptAllResult,
  AcceptRequest,
  ApprovalDetail,
  ApprovalOk,
  ApproveResult,
  ConflictChoice,
  GatewaySendResult,
  SharedSkipped,
  TodayNeed,
} from "@orbit-office/contracts";

import {
  acceptProposal,
  acceptProposals,
  ApiError,
  approveOutput,
  dismissNeed,
  getApprovalDetail,
  sendBack,
  sendDraft,
} from "../../api/client.js";
import { skippedReason } from "../../ui/index.js";

/** The server calls the controls make. Tests pass their own. */
export interface ApprovalApi {
  detail(key: string): Promise<ApprovalDetail>;
  approve(key: string): Promise<ApproveResult>;
  sendBack(key: string, comment: string): Promise<ApprovalOk>;
  dismiss(key: string, reason?: string): Promise<ApprovalOk>;
  /** `choice` settles a shared-library conflict; other proposals take none. */
  accept(key: string, sha256: string, choice?: ConflictChoice): Promise<ApprovalOk>;
  acceptAll(proposals: AcceptRequest[]): Promise<AcceptAllResult>;
  send(key: string, sha256: string): Promise<GatewaySendResult>;
}

export const serverApprovals: ApprovalApi = {
  detail: getApprovalDetail,
  approve: approveOutput,
  sendBack,
  dismiss: dismissNeed,
  accept: acceptProposal,
  acceptAll: acceptProposals,
  send: sendDraft,
};

type Tone = "done" | "info" | "error";

/** Orbit refused the app's authority: nothing changed, and trying again won't help. */
const NOT_PERMITTED = "The app isn’t allowed to do this yet, so nothing changed. Trying again won’t help.";

const REASONS: Record<string, string> = {
  task_not_reviewable: "It isn’t ready for review yet.",
  task_has_no_artifacts: "There is no output to add yet.",
  invalid_artifact: "The output isn’t in a form the Library accepts.",
  artifact_payload_invalid: "The output isn’t in a form the Library accepts.",
  artifact_too_large: "The output is too large for the Library.",
  artifact_not_found: "Some of the output is missing.",
  digest_already_exists: "The same output is already in your Library.",
  proposal_closed: "It was already accepted or dismissed.",
  proposal_payload_changed: "It changed after you saw it. Review it again.",
  proposal_page_missing: "The page it changes is no longer there.",
  share_cites_unshared: "It cites a document you haven’t shared yet. Share that one first.",
  share_private_refused: "The page is marked private, so it can’t be shared.",
  draft_changed: "The request changed after you saw it, so nothing was sent. Review it again.",
  draft_dismissed: "The request was dismissed, so nothing was sent.",
  draft_not_pending: "The request was already sent or is being sent. It is never sent twice.",
  not_permitted: NOT_PERMITTED,
};

function reasonText(code: string | undefined): string {
  return code === undefined ? "" : REASONS[code] ?? "";
}

/** One plain sentence for a failed action, never the code itself. */
function problem(error: unknown, action: string): string {
  if (error instanceof ApiError) {
    if (error.status === 0) return `${action} We couldn’t reach the app. Try again in a moment.`;
    if (error.code === "not_actionable" || error.code === "not_found") {
      return `${action} It was already handled or has changed. The list will update shortly.`;
    }
    if (error.code === "not_permitted") return `${action} ${NOT_PERMITTED}`;
    const reason = reasonText(error.reason);
    if (reason !== "") return `${action} ${reason}`;
  }
  return `${action} Please try again.`;
}

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className?: string, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className !== undefined) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function button(label: string, primary: boolean, onClick: () => void): HTMLButtonElement {
  const node = element("button", primary ? "primary-action" : "secondary-action", label);
  node.type = "button";
  node.addEventListener("click", onClick);
  return node;
}

/**
 * Card state that must survive the screen's redraws: what is loading or
 * running, and the last message per card. Details are fetched once per item.
 */
export class ApprovalControls {
  private readonly details = new Map<string, Promise<ApprovalDetail>>();
  private readonly messages = new Map<string, { tone: Tone; text: string }>();
  private readonly busy = new Set<string>();
  private readonly sendingBack = new Set<string>();
  private redraw: () => void = () => {};

  constructor(
    private readonly api: ApprovalApi,
    /** Announces finished actions where the user will see them after the card goes. */
    private readonly announce: (tone: Tone, text: string) => void,
  ) {}

  /** The screen redraws its last snapshot when a card's state changes. */
  bind(redraw: () => void): void {
    this.redraw = redraw;
  }

  private detail(key: string): Promise<ApprovalDetail> {
    let pending = this.details.get(key);
    if (pending === undefined) {
      pending = this.api.detail(key);
      this.details.set(key, pending);
      // A failed read is tried again on the next draw.
      pending.catch(() => { if (this.details.get(key) === pending) this.details.delete(key); });
    }
    return pending;
  }

  private async run(keys: readonly string[], work: () => Promise<void>): Promise<void> {
    if (keys.some((key) => this.busy.has(key))) return;
    for (const key of keys) {
      this.busy.add(key);
      this.messages.delete(key);
    }
    this.redraw();
    try {
      await work();
    } finally {
      for (const key of keys) this.busy.delete(key);
      this.redraw();
    }
  }

  private fail(key: string, text: string, stale = false): void {
    if (stale) this.details.delete(key);
    this.messages.set(key, { tone: "error", text });
  }

  private message(key: string): HTMLElement {
    const box = element("p", "approval-message");
    box.setAttribute("aria-live", "polite");
    const message = this.messages.get(key);
    if (message !== undefined) {
      box.textContent = message.text;
      box.dataset.tone = message.tone;
      if (message.tone === "error") box.setAttribute("role", "alert");
    } else {
      box.hidden = true;
    }
    return box;
  }

  private actions(key: string, ...buttons: HTMLButtonElement[]): HTMLElement {
    const row = element("div", "approval-actions");
    row.dataset.slot = "approval-actions";
    row.dataset.key = key;
    for (const item of buttons) item.disabled = this.busy.has(key);
    row.append(...buttons);
    return row;
  }

  /** Fill `target` from the item's detail once it arrives. */
  private fill(key: string, target: HTMLElement, draw: (detail: ApprovalDetail) => Node[]): HTMLElement {
    target.append(element("p", "approval-loading", "Loading the details…"));
    void this.detail(key).then(
      (detail) => { target.replaceChildren(...draw(detail)); },
      () => { target.replaceChildren(element("p", "approval-loading", "The details couldn’t be loaded. We’ll try again shortly.")); },
    );
    return target;
  }

  private files(key: string, intro: string): HTMLElement {
    return this.fill(key, element("div", "approval-detail"), (detail) => {
      if (detail.kind !== "approval" && detail.kind !== "script_output") return [];
      if (detail.files.length === 0) return [element("p", undefined, "No files are attached.")];
      const list = element("ul", "approval-files");
      list.append(...detail.files.map((name) => element("li", undefined, name)));
      return [element("p", undefined, intro), list];
    });
  }

  private preview(key: string, label: string): HTMLElement {
    const details = element("details", "approval-preview");
    details.dataset.key = `${key}-preview`;
    const summary = element("summary", undefined, label);
    summary.dataset.focus = `${key}-preview`;
    const body = this.fill(key, element("div"), (detail) => {
      if (detail.kind !== "proposal") return [];
      const text = element("pre", "approval-text", detail.preview);
      return detail.shareable ? [element("p", "approval-shared", "Will be shared with the team"), text] : [text];
    });
    details.append(summary, body);
    return details;
  }

  /** Approve, or send back with a comment, a reviewed agent to-do. */
  reviewed(item: TodayNeed): HTMLElement[] {
    const key = item.key;
    const approve = button("Approve", true, () => { void this.run([key], async () => {
      try {
        const result = await this.api.approve(key);
        this.announce("done", result.closed
          ? `Approved “${item.title}”. Its output is in your Library.`
          : `Approved “${item.title}”. Its output is in your Library, but the to-do couldn’t be closed yet.`);
      } catch (error) {
        this.fail(key, problem(error, "Couldn’t add this to your Library, so it is still waiting for your review."));
      }
    }); });
    const parts: HTMLElement[] = [this.files(key, "Output to review:")];
    if (!this.sendingBack.has(key)) {
      const back = button("Send back", false, () => { this.sendingBack.add(key); this.redraw(); });
      parts.push(this.actions(key, approve, back));
    } else {
      const form = element("form", "approval-send-back");
      const label = element("label", undefined, "What should change?");
      const field = element("textarea");
      field.name = "comment";
      field.required = true;
      field.rows = 3;
      field.dataset.focus = `${key}-comment`;
      label.append(field);
      const submit = button("Send back", true, () => {});
      submit.type = "submit";
      const cancel = button("Cancel", false, () => { this.sendingBack.delete(key); this.redraw(); });
      form.append(label, this.actions(key, submit, cancel));
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        const comment = field.value.trim();
        if (comment === "") { field.focus(); return; }
        void this.run([key], async () => {
          try {
            await this.api.sendBack(key, comment);
            this.sendingBack.delete(key);
            this.announce("done", `Sent “${item.title}” back with your comment.`);
          } catch (error) {
            this.fail(key, problem(error, "Couldn’t send this back."));
          }
        });
      });
      parts.push(form);
    }
    parts.push(this.message(key));
    return parts;
  }

  private dismissButton(item: TodayNeed, label: string, done: string): HTMLButtonElement {
    return button(label, false, () => { void this.run([item.key], async () => {
      try {
        await this.api.dismiss(item.key);
        this.announce("done", done);
      } catch (error) {
        this.fail(item.key, problem(error, "Couldn’t dismiss this."));
      }
    }); });
  }

  /** Files a script produced: keep them in the Library, or dismiss them. */
  scriptOutput(item: TodayNeed): HTMLElement[] {
    const key = item.key;
    const approve = button("Approve", true, () => { void this.run([key], async () => {
      try {
        const result = await this.api.approve(key);
        this.announce("done", result.closed
          ? `Added the files from “${item.title}” to your Library.`
          : `Added the files from “${item.title}” to your Library, but the item couldn’t be closed yet.`);
      } catch (error) {
        this.fail(key, problem(error, "Couldn’t add these files to your Library, so they are still waiting for you."));
      }
    }); });
    return [
      this.files(key, "Files the script made:"),
      this.actions(key, approve, this.dismissButton(item, "Dismiss", `Dismissed the files from “${item.title}”.`)),
      this.message(key),
    ];
  }

  /** A request to another system: shown in full, sent once only when approved. */
  gatewayDraft(item: TodayNeed): HTMLElement[] {
    const key = item.key;
    const request = this.fill(key, element("div", "approval-detail"), (detail) => {
      if (detail.kind !== "gateway_draft") return [];
      const target = element("p", "approval-request");
      const method = element("strong", undefined, detail.method);
      target.append(method, ` ${detail.url}`);
      const parts: Node[] = [];
      if (detail.connection !== "") parts.push(element("p", undefined, `Through your “${detail.connection}” connection:`));
      parts.push(target);
      parts.push(detail.body === null
        ? element("p", undefined, "No body is sent with this request.")
        : element("pre", "approval-text", detail.body));
      return parts;
    });
    const approve = button("Approve and send", true, () => { void this.run([key], async () => {
      let detail: ApprovalDetail;
      try {
        detail = await this.detail(key);
      } catch (error) {
        this.fail(key, problem(error, "Couldn’t load the request, so nothing was sent."));
        return;
      }
      if (detail.kind !== "gateway_draft") return;
      try {
        const result = await this.api.send(key, detail.sha256);
        if (result.outcome === "outcome_unknown") {
          // Not a failure: the request went out and no reply came back. Never retried.
          this.messages.set(key, { tone: "info", text: "The request went out but no reply came back, so we can’t tell whether it arrived. Check the other system before doing anything else; it won’t be sent again." });
          this.announce("info", `“${item.title}” went out, but no reply came back. Check the other system.`);
        } else {
          this.announce("done", result.status === null
            ? `Sent “${item.title}”.`
            : `Sent “${item.title}”. The other system answered with status ${result.status}.`);
        }
      } catch (error) {
        const stale = error instanceof ApiError && error.reason === "draft_changed";
        this.fail(key, problem(error, "Couldn’t send this request."), stale);
      }
    }); });
    return [
      request,
      this.actions(key, approve, this.dismissButton(item, "Dismiss", `Dismissed “${item.title}”. Nothing was sent.`)),
      this.message(key),
    ];
  }

  /** One proposal on its own card. */
  proposal(item: TodayNeed): HTMLElement[] {
    const key = item.key;
    const accept = button("Accept", true, () => { void this.run([key], async () => {
      try {
        const detail = await this.detail(key);
        if (detail.kind !== "proposal") return;
        await this.api.accept(key, detail.sha256);
        this.announce("done", `Accepted “${item.title}”.`);
      } catch (error) {
        const stale = error instanceof ApiError && error.reason === "proposal_payload_changed";
        this.fail(key, problem(error, "Couldn’t accept this suggestion."), stale);
      }
    }); });
    return [
      this.preview(key, "Review the change"),
      this.actions(key, accept, this.dismissButton(item, "Dismiss", `Dismissed “${item.title}”.`)),
      this.message(key),
    ];
  }

  /**
   * Several proposals on one card, each reviewable on its own. Accept all
   * accepts each in turn; a refusal leaves the others accepted and names it.
   */
  proposalGroup(groupKey: string, items: readonly TodayNeed[]): HTMLElement[] {
    const keys = items.map((item) => item.key);
    const acceptAll = button("Accept all", true, () => { void this.run([groupKey, ...keys], async () => {
      const ready: AcceptRequest[] = [];
      const refused: string[] = [];
      for (const item of items) {
        try {
          const detail = await this.detail(item.key);
          if (detail.kind === "proposal") ready.push({ key: item.key, sha256: detail.sha256 });
          else refused.push(item.title);
        } catch {
          refused.push(item.title);
        }
      }
      let accepted = 0;
      let notPermitted = false;
      if (ready.length > 0) {
        try {
          const { results } = await this.api.acceptAll(ready);
          for (const result of results) {
            if (result.accepted) { accepted += 1; continue; }
            const item = items.find((candidate) => candidate.key === result.key);
            if (item !== undefined) refused.push(item.title);
            if (result.error === "proposal_payload_changed") this.details.delete(result.key);
            if (result.error === "not_permitted") notPermitted = true;
          }
        } catch (error) {
          this.fail(groupKey, problem(error, "Couldn’t accept these pages."));
          return;
        }
      }
      if (refused.length === 0) {
        this.announce("done", `Accepted all ${accepted} ${accepted === 1 ? "page" : "pages"}.`);
        return;
      }
      const names = refused.map((title) => `“${title}”`).join(", ");
      const why = notPermitted ? ` ${NOT_PERMITTED}` : "";
      this.fail(groupKey, `Accepted ${accepted} of ${items.length} pages. ${names} couldn’t be accepted and ${refused.length === 1 ? "is" : "are"} still here to review.${why}`);
    }); });
    const pages = element("ul", "approval-pages");
    pages.setAttribute("aria-label", "Pages");
    for (const item of items) {
      const page = element("li", "approval-page");
      page.dataset.key = item.key;
      const title = element("h4", undefined, item.title);
      const accept = button("Accept", false, () => { void this.run([item.key], async () => {
        try {
          const detail = await this.detail(item.key);
          if (detail.kind !== "proposal") return;
          await this.api.accept(item.key, detail.sha256);
          this.announce("done", `Accepted “${item.title}”.`);
        } catch (error) {
          const stale = error instanceof ApiError && error.reason === "proposal_payload_changed";
          this.fail(item.key, problem(error, `Couldn’t accept “${item.title}”.`), stale);
        }
      }); });
      page.append(
        title,
        this.preview(item.key, "Review this page"),
        this.actions(item.key, accept, this.dismissButton(item, "Dismiss", `Dismissed “${item.title}”.`)),
        this.message(item.key),
      );
      pages.append(page);
    }
    const all = this.actions(groupKey, acceptAll);
    for (const control of all.querySelectorAll("button")) control.disabled = keys.some((key) => this.busy.has(key)) || this.busy.has(groupKey);
    return [pages, all, this.message(groupKey)];
  }

  /**
   * The shared library's card: colleagues' updates to accept or dismiss,
   * pages changed on both sides to settle with **Use theirs** or **Keep
   * mine**, and the documents the last sync left out.
   */
  sharedGroup(items: readonly TodayNeed[], skipped: readonly SharedSkipped[], dismissSkipped: () => Promise<void>): HTMLElement[] {
    const incoming = items.filter((item) => item.shared !== "conflict");
    const conflicts = items.filter((item) => item.shared === "conflict");
    const parts: HTMLElement[] = [element("p", undefined, [
      incoming.length > 0 ? `${incoming.length} ${incoming.length === 1 ? "update" : "updates"} from colleagues` : "",
      conflicts.length > 0 ? `${conflicts.length} ${conflicts.length === 1 ? "page" : "pages"} changed here and in the shared library` : "",
      skipped.length > 0 ? `${skipped.length} ${skipped.length === 1 ? "document" : "documents"} left out of the last sync` : "",
    ].filter(Boolean).join(", ") + ".")];
    if (incoming.length > 0) {
      const list = element("ul", "approval-pages");
      list.setAttribute("aria-label", "Updates from colleagues");
      for (const item of incoming) {
        const accept = button("Accept", true, () => { void this.run([item.key], async () => {
          try {
            const detail = await this.detail(item.key);
            if (detail.kind !== "proposal") return;
            await this.api.accept(item.key, detail.sha256);
            this.announce("done", `Accepted the update to “${item.title}”.`);
          } catch (error) {
            const stale = error instanceof ApiError && error.reason === "proposal_payload_changed";
            this.fail(item.key, problem(error, `Couldn’t accept the update to “${item.title}”.`), stale);
          }
        }); });
        const page = element("li", "approval-page");
        page.dataset.key = item.key;
        page.append(
          element("h4", undefined, item.title),
          this.preview(item.key, "Review the update"),
          this.actions(item.key, accept, this.dismissButton(item, "Dismiss", `Dismissed the update to “${item.title}”.`)),
          this.message(item.key),
        );
        list.append(page);
      }
      parts.push(element("h4", "approval-shared-heading", "Updates from colleagues"), list);
    }
    if (conflicts.length > 0) {
      const list = element("ul", "approval-pages");
      list.setAttribute("aria-label", "Changed on both sides");
      for (const item of conflicts) {
        const settle = (choice: ConflictChoice, label: string, done: string): HTMLButtonElement =>
          button(label, choice === "theirs", () => { void this.run([item.key], async () => {
            try {
              const detail = await this.detail(item.key);
              if (detail.kind !== "proposal") return;
              await this.api.accept(item.key, detail.sha256, choice);
              this.announce("done", done);
            } catch (error) {
              const stale = error instanceof ApiError && error.reason === "proposal_payload_changed";
              this.fail(item.key, problem(error, `Couldn’t settle “${item.title}”.`), stale);
            }
          }); });
        const page = element("li", "approval-page");
        page.dataset.key = item.key;
        page.append(
          element("h4", undefined, item.title),
          element("p", undefined, "You and a colleague both changed this page. Keep one version; the other stays in the shared library’s history."),
          this.preview(item.key, "Compare their version"),
          this.actions(item.key,
            settle("theirs", "Use theirs", `Used the shared version of “${item.title}”.`),
            settle("mine", "Keep mine", `Kept your version of “${item.title}”. It goes to the team on your next sync.`)),
          this.message(item.key),
        );
        list.append(page);
      }
      parts.push(element("h4", "approval-shared-heading", "Changed on both sides"), list);
    }
    if (skipped.length > 0) {
      const key = "shared-library-skipped";
      const list = element("ul", "approval-skipped");
      list.setAttribute("aria-label", "Left out of the last sync");
      for (const entry of skipped) {
        const row = element("li");
        row.append(element("strong", undefined, entry.name), ` ${entry.phase === "push" ? "wasn’t shared" : "wasn’t received"}. ${skippedReason(entry.reason)}`);
        list.append(row);
      }
      const done = button("Got it", false, () => { void this.run([key], async () => {
        try {
          await dismissSkipped();
        } catch (error) {
          this.fail(key, problem(error, "Couldn’t clear this list."));
        }
      }); });
      parts.push(element("h4", "approval-shared-heading", "Left out of the last sync"), list, this.actions(key, done), this.message(key));
    }
    return parts;
  }
}
