/**
 * The Tags editor (specs/scribe.md, "Configuration"): create, edit and
 * delete custom and default tags (name, scope, instruction, sink, mode),
 * and Restore defaults. It reads the effective tags and saves through
 * `office.config-set`. While Orbit refuses the app that tool
 * (ORB-20363), a save comes back `not_permitted`: the editor then shows
 * the refusal and the operator command, claims no success, and turns
 * read-only.
 */
import type { ScribeTagDef, ScribeTagMode, ScribeTagSave, ScribeTagSaved, ScribeTags } from "@orbit-office/contracts";

import { ApiError, getJson, postJson } from "../../api/client.js";
import { copy, el } from "./dom.js";
import { blankTag, defaultModeForSink, messageFor, modesForSink, TAG_MODES, TAG_SCOPES, TAG_SINKS } from "./model.js";

const codeOf = (error: unknown): string => error instanceof ApiError ? error.code : "unknown";

function labelled(caption: string, control: HTMLElement): HTMLLabelElement {
  const label = el("label", "", "scribe-tag-field");
  label.append(el("span", caption), control);
  return label;
}

function selectOf<T extends string>(options: ReadonlyArray<[T, string]>, value: T): HTMLSelectElement {
  const select = el("select");
  for (const [name, title] of options) {
    const option = el("option", title);
    option.value = name;
    select.append(option);
  }
  select.value = value;
  return select;
}

/** The Tags tab: `load` reads the effective tags and draws the editor. */
export function createTagsView(): {
  load: () => Promise<(panel: HTMLElement, stated: HTMLElement) => void>;
} {
  let readOnly = false;

  const load = async (): Promise<(panel: HTMLElement, stated: HTMLElement) => void> => {
    let current = await getJson<ScribeTags>("/api/scribe/tags");
    return (panel, stated) => {
      const status = el("p", "", "scribe-status");
      status.setAttribute("role", "status");
      const list = el("div", "", "scribe-tag-list");
      const form = el("form", "", "scribe-tag-form");
      form.setAttribute("aria-label", "Tag editor");
      let draft: ScribeTagDef = blankTag();
      let editing: string | null = null;

      const name = el("input");
      name.placeholder = "e.g. incident";
      name.maxLength = 32;
      const scope = selectOf(TAG_SCOPES, draft.scope);
      const instruction = el("textarea");
      instruction.placeholder = "What the scribe should record for this tag.";
      instruction.maxLength = 2000;
      const sink = selectOf(TAG_SINKS, draft.sink);
      const mode = el("select");
      const capture = selectOf([["markers", "Markers only"], ["markers+inferred", "Markers, plus suggested ones"]] as const, draft.capture);
      const workspace = el("input");
      workspace.placeholder = "Orbit workspace for the proposed task";
      const workspaceField = labelled("Workspace", workspace);
      const saveButton = el("button", "Save tag");
      saveButton.type = "submit";
      const cancelButton = el("button", "Cancel edit");
      cancelButton.type = "button";
      const restoreButton = el("button", "Restore defaults");
      restoreButton.type = "button";

      const fillModes = (chosen: ScribeTagMode): void => {
        const allowed = modesForSink(draft.sink);
        mode.replaceChildren(...TAG_MODES.filter(([value]) => allowed.includes(value)).map(([value, title]) => {
          const option = el("option", title);
          option.value = value;
          return option;
        }));
        mode.value = allowed.includes(chosen) ? chosen : defaultModeForSink(draft.sink);
      };
      const syncSink = (): void => {
        workspaceField.hidden = draft.sink !== "orbit_task";
        fillModes(draft.mode);
      };
      sink.addEventListener("change", () => {
        draft = { ...draft, sink: sink.value as ScribeTagDef["sink"] };
        draft.mode = defaultModeForSink(draft.sink);
        syncSink();
      });
      const fillForm = (): void => {
        name.value = draft.name;
        name.disabled = readOnly || editing !== null;
        scope.value = draft.scope;
        instruction.value = draft.instruction;
        sink.value = draft.sink;
        capture.value = draft.capture;
        workspace.value = draft.workspace ?? "";
        syncSink();
        saveButton.textContent = editing === null ? "Save tag" : `Save ${editing}`;
        cancelButton.hidden = editing === null;
        for (const control of [scope, instruction, sink, mode, capture, workspace, saveButton, cancelButton, restoreButton]) {
          control.disabled = readOnly;
        }
        if (!readOnly) name.disabled = editing !== null;
      };
      const drawList = (): void => {
        stated.textContent = current.tags_customized
          ? "Your tags. Restore defaults brings back preferences and decisions as shipped."
          : "The two shipped tags. Editing or deleting one keeps the other as shipped.";
        list.replaceChildren(...current.tags.map((tag) => {
          const row = el("article", "", "scribe-entry scribe-tag-row");
          row.dataset.name = tag.name;
          const head = el("div", "", "scribe-entry-head");
          head.append(el("strong", `#${tag.name}`), el("span", `${tag.scope} · ${tag.sink} · ${tag.mode}`));
          const actions = el("div", "", "scribe-actions");
          const edit = el("button", "Edit");
          edit.type = "button";
          edit.disabled = readOnly;
          edit.addEventListener("click", () => { draft = { ...tag }; editing = tag.name; fillForm(); });
          const remove = el("button", "Delete");
          remove.type = "button";
          remove.disabled = readOnly;
          remove.addEventListener("click", () => { void save({ action: "delete", name: tag.name }); });
          actions.append(edit, remove);
          row.append(head, el("p", tag.instruction, "scribe-card-summary"), actions);
          return row;
        }));
      };
      async function save(action: ScribeTagSave): Promise<void> {
        status.textContent = "Saving.";
        try {
          const saved = await postJson<ScribeTagSaved>("/api/scribe/tags", action);
          current = saved.tags;
          draft = blankTag();
          editing = null;
          status.textContent = "Saved.";
          drawList();
          fillForm();
        } catch (error) {
          const code = codeOf(error);
          if (code === "not_permitted") {
            readOnly = true;
            const command = error instanceof ApiError ? error.command : undefined;
            const parts: HTMLElement[] = [el("p", messageFor(code), "scribe-status")];
            if (command) {
              const row = el("div", "", "scribe-cite");
              const button = el("button", "Copy command");
              button.type = "button";
              button.addEventListener("click", () => { void copy(command, button); });
              row.append(el("code", command), button);
              parts.push(row);
            }
            status.replaceChildren(...parts);
            drawList();
            fillForm();
            return;
          }
          status.textContent = messageFor(code);
        }
      }
      form.addEventListener("submit", (event) => {
        event.preventDefault();
        const tag: ScribeTagDef = {
          name: name.value.trim(),
          scope: scope.value as ScribeTagDef["scope"],
          instruction: instruction.value.trim(),
          sink: sink.value as ScribeTagDef["sink"],
          mode: mode.value as ScribeTagMode,
          capture: capture.value as ScribeTagDef["capture"],
          ...(sink.value === "orbit_task" && workspace.value.trim() ? { workspace: workspace.value.trim() } : {}),
        };
        if (!tag.name || !tag.instruction) { status.textContent = messageFor("invalid_input"); return; }
        void save({ action: "upsert", tag });
      });
      cancelButton.addEventListener("click", () => { draft = blankTag(); editing = null; fillForm(); });
      restoreButton.addEventListener("click", () => { void save({ action: "restore" }); });

      const buttons = el("div", "", "scribe-actions");
      buttons.append(saveButton, cancelButton, restoreButton);
      form.append(
        el("h3", "Add or edit a tag"),
        labelled("Name", name),
        labelled("Scope", scope),
        labelled("Instruction", instruction),
        labelled("Entries go to", sink),
        labelled("Mode", mode),
        labelled("Capture", capture),
        workspaceField,
        buttons,
        status,
      );
      if (readOnly) {
        status.textContent = messageFor("not_permitted");
      }
      drawList();
      fillForm();
      panel.replaceChildren(list, form);
    };
  };
  return { load };
}
