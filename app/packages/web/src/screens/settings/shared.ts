/**
 * Settings › Shared library (docs/design/office/specs/shared-library.md,
 * "App"): the remote, branch and member name, saved with `office.config-set`;
 * the offer to turn on hourly sync; **Sync now** with the counts or the
 * failure in words. A refused field is named, never its value.
 */
import type { SharedLibraryBlock, SharedLibraryStatus, SharedRoutineState, SharedSyncOutcome } from "@orbit-office/contracts";
import { ApiError, enableSharedSync, saveSharedLibrary, syncSharedLibrary } from "../../api/client.js";
import { syncCounts, syncFailure } from "../../ui/index.js";

export interface SharedLibraryMount {
  data: SharedLibraryStatus;
  busy(): boolean;
  setBusy(busy: boolean): void;
  alive(): boolean;
  /** Local time for a sync's timestamp; the browser's own locale by default. */
  formatTime?: (iso: string) => string;
}

type Field = keyof SharedLibraryBlock;

const FIELD_REFUSAL: Record<Field, string> = {
  remote: "Check the shared library address. Use an ssh:// address, user@host:path, an https:// address without a user name or password, or the full path to a repository on this computer.",
  branch: "Check the branch name. It must be a plain git branch name, such as main.",
  member: "Check your name in the shared library: lowercase letters, digits and single hyphens, up to 32 characters.",
};

/** A save refusal in plain words. The entered value is never repeated. */
export function saveMessage(error: unknown): string {
  if (error instanceof ApiError) {
    if (error.code === "invalid_input") {
      return error.field === "remote" || error.field === "branch" || error.field === "member"
        ? FIELD_REFUSAL[error.field] : "Check the address, branch and your name.";
    }
    if (error.code === "settings_busy") return "Another settings change is being saved. Please try again in a moment.";
    if (error.status === 0) return "The app server can't be reached. Nothing was saved.";
  }
  return "Could not confirm the save. Check that Orbit is available, then refresh before trying again.";
}

/** The latest sync the app ran, as one sentence. */
export function lastSyncWords(last: SharedSyncOutcome | null, formatTime: (iso: string) => string): string {
  if (last === null) return "Not synced from the app yet. The hourly sync files what comes in under Needs you.";
  if (last.ok) return `Last synced ${formatTime(last.sync.synced_at)}. ${syncCounts(last.sync)}.`;
  return `The sync at ${formatTime(last.at)} didn't finish. ${syncFailure(last.reason)}`;
}

function localTime(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? "at an unknown time" : date.toLocaleString();
}

export function mountSharedLibrary(content: HTMLElement, mount: SharedLibraryMount): void {
  const formatTime = mount.formatTime ?? localTime;
  let configured = mount.data.shared;
  const form = document.createElement("form");
  form.className = "settings-form";
  form.setAttribute("aria-label", "Shared library");
  form.noValidate = true;
  const fields = document.createElement("div");
  fields.className = "settings-fields";
  const remote = field(fields, "Shared library address", "remote", configured?.remote ?? "", true);
  remote.placeholder = "git@github.com:your-team/team-library.git";
  remote.classList.add("settings-url");
  remote.spellcheck = false;
  hint(remote, "The team's git repository. Your own git sign-in reaches it; Office stores no password.");
  const branch = field(fields, "Branch", "branch", configured?.branch ?? "main");
  branch.spellcheck = false;
  const member = field(fields, "Your name in the shared library", "member", configured?.member ?? "");
  member.placeholder = "daniel";
  member.maxLength = 32;
  member.spellcheck = false;
  hint(member, "Lowercase letters, digits and hyphens. Colleagues see it on what you share.");
  const inputs: Record<Field, HTMLInputElement> = { remote, branch, member };

  const feedback = text("p", configured === null
    ? "No shared library is connected. Nothing leaves this computer until you connect one and share a document."
    : "");
  feedback.className = "settings-feedback";
  feedback.setAttribute("role", "status");
  const offer = document.createElement("div");
  offer.className = "settings-actions settings-offer";
  const actions = document.createElement("div");
  actions.className = "settings-actions";
  const submit = text("button", "Save shared library");
  submit.type = "submit";
  actions.append(submit);
  form.append(fields, feedback, offer, actions);

  const sync = document.createElement("section");
  sync.className = "settings-form settings-sync";
  sync.setAttribute("aria-label", "Sync");
  const syncStatus = text("p", "");
  syncStatus.className = "settings-sync-status";
  syncStatus.setAttribute("role", "status");
  const syncNow = text("button", "Sync now");
  syncNow.type = "button";
  const syncHint = text("p", "");
  syncHint.className = "settings-hint";
  const syncActions = document.createElement("div");
  syncActions.className = "settings-actions";
  syncActions.append(syncNow);
  sync.append(text("h3", "Sync"), syncStatus, syncActions, syncHint);

  function drawSync(): void {
    syncNow.disabled = configured === null || mount.busy();
    syncHint.textContent = configured === null
      ? "Save a shared library first."
      : "Sends what you share and brings in what colleagues shared. Updates and conflicts wait for you under Needs you.";
  }
  syncStatus.textContent = lastSyncWords(mount.data.last_sync, formatTime);
  drawSync();

  const heading = document.createElement("div");
  heading.className = "settings-heading";
  heading.append(text("h2", "Shared library"));
  content.replaceChildren(
    heading,
    text("p", "Connect one git repository your team shares. Only documents you switch to Share with the team are sent."),
    form,
    sync,
  );

  function setBusy(value: boolean): void {
    mount.setBusy(value);
    submit.disabled = value;
    for (const control of offer.querySelectorAll("button")) control.disabled = value;
    drawSync();
  }

  function offerRoutine(state: SharedRoutineState): void {
    offer.replaceChildren();
    if (state === "on") { offer.append(text("p", "Hourly sync is on.")); return; }
    if (state === "unavailable") {
      offer.append(text("p", "Hourly sync can't be turned on from here. Use Sync now, or check Routines."));
      return;
    }
    const turnOn = text("button", "Turn on hourly sync");
    turnOn.type = "button";
    offer.append(text("p", "Sync with the team every hour?"), turnOn);
    turnOn.addEventListener("click", () => {
      if (mount.busy()) return;
      setBusy(true);
      void (async () => {
        try {
          await enableSharedSync();
          if (mount.alive()) offerRoutine("on");
        } catch {
          if (!mount.alive()) return;
          offer.replaceChildren(text("p", "Hourly sync couldn't be turned on. Try again, or turn it on in Routines."), turnOn);
        } finally {
          setBusy(false);
        }
      })();
    });
    turnOn.focus();
  }

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (mount.busy()) return;
    const body: SharedLibraryBlock = { remote: remote.value.trim(), branch: branch.value.trim(), member: member.value.trim() };
    for (const input of Object.values(inputs)) input.removeAttribute("aria-invalid");
    offer.replaceChildren();
    setBusy(true);
    void (async () => {
      try {
        const saved = await saveSharedLibrary(body);
        if (!mount.alive()) return;
        configured = saved.shared;
        feedback.setAttribute("role", "status");
        feedback.textContent = "Shared library saved.";
        offerRoutine(saved.routine);
      } catch (error) {
        if (!mount.alive()) return;
        if (error instanceof ApiError && (error.field === "remote" || error.field === "branch" || error.field === "member")) {
          inputs[error.field].setAttribute("aria-invalid", "true");
          inputs[error.field].focus();
        }
        feedback.setAttribute("role", "alert");
        feedback.textContent = saveMessage(error);
      } finally {
        setBusy(false);
      }
    })();
  });

  syncNow.addEventListener("click", () => {
    if (mount.busy() || configured === null) return;
    setBusy(true);
    syncStatus.textContent = "Syncing with the shared library. This can take a few minutes.";
    void (async () => {
      try {
        const result = await syncSharedLibrary();
        if (!mount.alive()) return;
        syncStatus.textContent = lastSyncWords({ ok: true, sync: result }, formatTime);
      } catch (error) {
        if (!mount.alive()) return;
        const reason = error instanceof ApiError && error.reason !== undefined ? error.reason
          : error instanceof ApiError && error.status === 0 ? "network" : "unknown";
        syncStatus.textContent = reason === "network"
          ? "The app server can't be reached, so the sync may not have run. Refresh to see the last sync."
          : `The sync didn't finish. ${syncFailure(reason)}`;
      } finally {
        setBusy(false);
      }
    })();
  });
}

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.textContent = content;
  return node;
}

function field(fields: HTMLElement, label: string, name: Field, value: string, wide = false): HTMLInputElement {
  const wrap = text("label", label);
  wrap.className = "settings-field";
  if (wide) wrap.classList.add("settings-wide");
  const input = document.createElement("input");
  input.name = name;
  input.type = "text";
  input.value = value;
  input.required = true;
  input.autocomplete = "off";
  wrap.append(input);
  fields.append(wrap);
  return input;
}

function hint(input: HTMLInputElement, words: string): void {
  const node = text("span", words);
  node.className = "settings-hint";
  input.parentElement?.append(node);
}
