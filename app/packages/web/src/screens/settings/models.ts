/**
 * Settings › Models: "Models". Each job, the model it calls (as configured),
 * where it runs and what the app could check (`GET /api/settings/models`);
 * a summary of the private route; and where meeting agent to-dos go
 * (`meetings.agent_workspace`, read through `GET /api/todos/agents`).
 */
import type { AgentTodos, ModelsView, PrivateRouteStatus } from "@orbit-office/contracts";

import { getAgentTodos, getModels, getPrivateRoute, saveAgentWorkspace } from "../../api/client.js";
import { element, panel, panelBody, panelGrid, pill, type Tone } from "../../ui/index.js";

const TONES: Record<ModelsView["jobs"][number]["check"]["state"], Tone> = { passed: "done", attention: "attention", off: "neutral" };

function modelsTable(view: ModelsView): HTMLElement {
  const passed = view.jobs.filter((job) => job.check.state === "passed").length;
  const at = new Date(view.checked_at);
  const time = Number.isFinite(at.getTime()) ? ` · ${at.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })}` : "";
  const panelEl = panel("Models", {
    id: "settings-models-heading",
    meta: `${passed} of ${view.jobs.length} checks passed${time}`,
  });
  const intro = element("p", "What each job calls, and where it runs. Checked when this page opens.", "settings-models-intro");
  const table = document.createElement("div");
  table.className = "settings-models";
  table.setAttribute("role", "table");
  table.setAttribute("aria-label", "Models by job");
  const head = document.createElement("div");
  head.className = "settings-models-row settings-models-head";
  head.setAttribute("role", "row");
  for (const label of ["Job", "Model", "Where it runs", "Check"]) {
    const cell = element("span", label);
    cell.setAttribute("role", "columnheader");
    head.append(cell);
  }
  table.append(head);
  for (const job of view.jobs) {
    const line = document.createElement("div");
    line.className = "settings-models-row";
    line.setAttribute("role", "row");
    const cells = [element("span", job.job), element("span", job.model, "settings-models-model"), element("span", job.where, "settings-models-where")];
    const check = document.createElement("span");
    check.append(pill(job.check.label, TONES[job.check.state]));
    for (const cell of [...cells, check]) {
      cell.setAttribute("role", "cell");
      line.append(cell);
    }
    table.append(line);
  }
  panelEl.append(panelBody(intro, table));
  return panelEl;
}

function privateSummary(route: PrivateRouteStatus | null, open: () => void): HTMLElement {
  const panelEl = panel("Private route", { id: "settings-private-summary" });
  const body = panelBody(element("p", "Sources marked private never leave your endpoint. Built with Muse Spark Open."));
  const block = route?.route ?? null;
  if (route === null) body.append(element("p", "Couldn’t read the private route.", "settings-meta"));
  else if (block === null) body.append(element("p", "Not set up. Private sources can’t be processed until it is.", "settings-meta"));
  else {
    body.append(element("p", `${block.label} · ${block.model}`, "settings-meta settings-url"));
    body.append(element("p", block.verified_at === null ? "Not checked yet." : `Last checked ${new Date(block.verified_at).toLocaleString()}.`, "settings-meta"));
  }
  const button = element("button", block === null ? "Set up the private route" : "Edit the private route");
  button.type = "button";
  button.addEventListener("click", open);
  body.append(button);
  panelEl.append(body);
  return panelEl;
}

function targetPanel(agents: AgentTodos | null, alive: () => boolean): HTMLElement {
  const panelEl = panel("Where agent to-dos go", { id: "settings-target-heading" });
  const body = panelBody(element("p", "Engineering to-dos from meetings become Orbit tasks in this workspace, for its crews to ship. With none, every to-do stays yours."));
  if (agents === null) {
    body.append(element("p", "Couldn’t read the target workspace.", "settings-meta"));
    panelEl.append(body);
    return panelEl;
  }
  const label = document.createElement("label");
  label.className = "settings-field";
  label.append(element("span", "Target workspace"));
  const select = document.createElement("select");
  for (const workspace of [...new Set([...(agents.target_workspace === null ? [] : [agents.target_workspace]), ...agents.followed])]) {
    const option = element("option", workspace);
    option.value = workspace;
    select.append(option);
  }
  const none = element("option", "None: keep every to-do as mine");
  none.value = "";
  select.append(none);
  select.value = agents.target_workspace ?? "";
  label.append(select);
  const status = element("p", agents.followed.length === 0 && agents.target_workspace === null
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
  panelEl.append(body);
  return panelEl;
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
