/**
 * Settings › Models: "Every model is Meta's". Each job, its Meta model,
 * where it runs and what the app could check (`GET /api/settings/models`);
 * a summary of the private route; and where meeting agent to-dos go
 * (`meetings.agent_workspace`, read through `GET /api/todos/agents`).
 */
import type { AgentTodos, ModelsView, PrivateRouteStatus } from "@orbit-office/contracts";

import { getAgentTodos, getModels, getPrivateRoute, saveAgentWorkspace } from "../../api/client.js";
import { panel, panelBody, panelGrid, pill, type Tone } from "../../ui/index.js";

function text<K extends keyof HTMLElementTagNameMap>(tag: K, content: string, className?: string): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.textContent = content;
  if (className !== undefined) element.className = className;
  return element;
}

const TONES: Record<ModelsView["jobs"][number]["check"]["state"], Tone> = { passed: "done", attention: "attention", off: "neutral" };

function modelsTable(view: ModelsView): HTMLElement {
  const passed = view.jobs.filter((job) => job.check.state === "passed").length;
  const at = new Date(view.checked_at);
  const time = Number.isFinite(at.getTime()) ? ` · ${at.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}` : "";
  const element = panel("Every model is Meta’s", {
    id: "settings-models-heading",
    meta: `${passed} of ${view.jobs.length} checks passed${time}`,
  });
  const intro = text("p", "What each job calls, and where it runs. Checked when this page opens.", "settings-models-intro");
  const table = document.createElement("div");
  table.className = "settings-models";
  table.setAttribute("role", "table");
  table.setAttribute("aria-label", "Models by job");
  const head = document.createElement("div");
  head.className = "settings-models-row settings-models-head";
  head.setAttribute("role", "row");
  for (const label of ["Job", "Model", "Where it runs", "Check"]) {
    const cell = text("span", label);
    cell.setAttribute("role", "columnheader");
    head.append(cell);
  }
  table.append(head);
  for (const job of view.jobs) {
    const line = document.createElement("div");
    line.className = "settings-models-row";
    line.setAttribute("role", "row");
    const cells = [text("span", job.job), text("span", job.model, "settings-models-model"), text("span", job.where, "settings-models-where")];
    const check = document.createElement("span");
    check.append(pill(job.check.label, TONES[job.check.state]));
    for (const cell of [...cells, check]) {
      cell.setAttribute("role", "cell");
      line.append(cell);
    }
    table.append(line);
  }
  element.append(panelBody(intro, table));
  return element;
}

function privateSummary(route: PrivateRouteStatus | null, open: () => void): HTMLElement {
  const element = panel("Private route", { id: "settings-private-summary" });
  const body = panelBody(text("p", "Sources marked private never leave your endpoint. Built with Muse Spark Open."));
  const block = route?.route ?? null;
  if (route === null) body.append(text("p", "Couldn’t read the private route.", "settings-meta"));
  else if (block === null) body.append(text("p", "Not set up. Private sources can’t be processed until it is.", "settings-meta"));
  else {
    body.append(text("p", `${block.label} · ${block.model}`, "settings-meta settings-url"));
    body.append(text("p", block.verified_at === null ? "Not checked yet." : `Last checked ${new Date(block.verified_at).toLocaleString()}.`, "settings-meta"));
  }
  const button = text("button", block === null ? "Set up the private route" : "Edit the private route");
  button.type = "button";
  button.addEventListener("click", open);
  body.append(button);
  element.append(body);
  return element;
}

function targetPanel(agents: AgentTodos | null, alive: () => boolean): HTMLElement {
  const element = panel("Where agent to-dos go", { id: "settings-target-heading" });
  const body = panelBody(text("p", "Engineering to-dos from meetings become Orbit tasks in this workspace, for its crews to ship. With none, every to-do stays yours."));
  if (agents === null) {
    body.append(text("p", "Couldn’t read the target workspace.", "settings-meta"));
    element.append(body);
    return element;
  }
  const label = document.createElement("label");
  label.className = "settings-field";
  label.append(text("span", "Target workspace"));
  const select = document.createElement("select");
  for (const workspace of [...new Set([...(agents.target_workspace === null ? [] : [agents.target_workspace]), ...agents.followed])]) {
    const option = text("option", workspace);
    option.value = workspace;
    select.append(option);
  }
  const none = text("option", "None: keep every to-do as mine");
  none.value = "";
  select.append(none);
  select.value = agents.target_workspace ?? "";
  label.append(select);
  const status = text("p", agents.followed.length === 0 && agents.target_workspace === null
    ? "Only workspaces you follow (brief.task_workspaces) are offered here." : "", "settings-meta");
  status.setAttribute("role", "status");
  let saved = agents.target_workspace ?? "";
  select.addEventListener("change", () => {
    select.disabled = true;
    status.textContent = "Saving…";
    const value = select.value;
    void saveAgentWorkspace(value === "" ? null : value).then(() => {
      saved = value;
      if (alive()) status.textContent = "Saved.";
    }).catch(() => {
      select.value = saved;
      if (alive()) status.textContent = "Couldn’t save. Try again in a moment.";
    }).finally(() => { select.disabled = false; });
  });
  body.append(label, status);
  element.append(body);
  return element;
}

/** Draw the Models section. `openPrivate` switches to the Private route tab. */
export async function mountModels(content: HTMLElement, alive: () => boolean, openPrivate: () => void): Promise<void> {
  const [models, route, agents] = await Promise.all([
    getModels(),
    getPrivateRoute().catch(() => null),
    getAgentTodos().catch(() => null),
  ]);
  if (!alive()) return;
  content.replaceChildren(modelsTable(models), panelGrid(privateSummary(route, openPrivate), targetPanel(agents, alive)));
}
