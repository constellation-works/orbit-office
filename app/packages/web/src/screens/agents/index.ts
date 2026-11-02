/** Agent work: the Orbit tasks agents are carrying, across the workspaces you follow. Read only. */
import type { AgentWork, AgentWorkTask } from "@orbit-office/contracts";

import { getJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { sourceHref } from "../../api/source-location.js";
import { dot, emptyState, errorState, monoMeta, pill, screenHead, tile, tileRow, type DotTone } from "../../ui/index.js";
import { styles } from "./style.js";

/** Rows shown before "Show N more" in the collapsed groups. */
const COLLAPSED = 3;

interface Group {
  key: "working_now" | "in_review" | "queued" | "done_today";
  title: string;
  anchor: string;
  collapsible: boolean;
}

const GROUPS: readonly Group[] = [
  { key: "working_now", title: "Working now", anchor: "agents-working-now", collapsible: false },
  { key: "in_review", title: "In review", anchor: "agents-in-review", collapsible: false },
  { key: "queued", title: "Queued", anchor: "agents-queued", collapsible: true },
  { key: "done_today", title: "Done today", anchor: "agents-done-today", collapsible: true },
];

function paragraph(text: string): HTMLParagraphElement {
  const element = document.createElement("p");
  element.textContent = text;
  return element;
}

/** Time since the last update, in plain words. */
export function sinceUpdate(updatedAt: string, now: Date): string {
  const then = Date.parse(updatedAt);
  if (!Number.isFinite(then)) return "";
  const seconds = Math.max(0, Math.floor((now.getTime() - then) / 1000));
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

const DOTS: Record<Group["key"], DotTone> = { working_now: "working", in_review: "attention", queued: "neutral", done_today: "done" };

/** One task: a dot, the title and a mono line (workspace, id, last update, source), then crew and PR. */
function row(task: AgentWorkTask, now: Date, key: Group["key"]): HTMLElement {
  const article = document.createElement("article");
  article.className = "agent-row";
  article.dataset.workspace = task.workspace;
  const main = document.createElement("div");
  main.className = "agent-main";
  const title = document.createElement("h3");
  title.textContent = task.title;
  const ago = sinceUpdate(task.updated_at, now);
  const meta = paragraph([task.workspace, task.id, ago === "" ? "" : `updated ${ago}`].filter(Boolean).join(" · "));
  meta.className = "agent-meta mono-meta";
  if (task.source !== undefined && task.source !== null) {
    meta.append(document.createTextNode(" · from "), monoMeta(task.source, sourceHref(task.source)));
  }
  const detail = document.createElement("div");
  detail.className = "agent-detail";
  if (task.type !== "") detail.append(pill(task.type));
  if (task.priority === "high" || task.priority === "critical") detail.append(pill(task.priority, "danger"));
  main.append(title, meta, detail);
  const trail = document.createElement("div");
  trail.className = "agent-trail";
  if (task.crew !== null && task.crew !== "") {
    const crew = monoMeta(task.crew);
    crew.classList.add("agent-crew");
    trail.append(crew);
  }
  if (task.pr !== undefined && task.pr !== null) {
    const pr = monoMeta(task.pr.merged ? `PR #${task.pr.number} · merged` : `PR #${task.pr.number}`, task.pr.url ?? undefined);
    if (pr instanceof HTMLAnchorElement) { pr.target = "_blank"; pr.rel = "noopener noreferrer"; }
    trail.append(pr);
  }
  article.append(dot(DOTS[key]), main, trail);
  return article;
}

export interface AgentsScreenOptions {
  load?: () => Promise<AgentWork>;
  now?: () => Date;
  pollIntervalMs?: number;
}

/** The production screen and browser tests use the same renderer. */
export function createAgentsScreen({
  load = () => getJson<AgentWork>("/api/agents"),
  now = () => new Date(),
  pollIntervalMs = 5_000,
}: AgentsScreenOptions = {}): Screen {
  return {
    id: "agents",
    title: "Agent work",
    status: "built",
    async render(root) {
      const view = document.createElement("div");
      view.className = "agents";
      const style = document.createElement("style");
      style.textContent = styles;
      const headerMeta = document.createElement("div");
      headerMeta.className = "agents-header-meta";
      const updated = document.createElement("span");
      updated.className = "agents-updated";
      const orbitLink = document.createElement("a");
      orbitLink.textContent = "Open in Orbit";
      orbitLink.href = "#agents";
      headerMeta.append(updated, orbitLink);
      const header = screenHead("Agent work", "Orbit tasks agents carry for you, across the workspaces you follow. Every crew runs a Meta model through Muse Code.", headerMeta);
      const feedback = document.createElement("div");
      feedback.setAttribute("aria-live", "polite");
      const content = document.createElement("div");
      content.append(paragraph("Gathering agent work…"));
      content.setAttribute("aria-busy", "true");
      view.append(style, header, feedback, content);
      root.replaceChildren(view);

      let selected = "all";
      const expanded = new Set<string>();
      let shown: AgentWork | undefined;
      let previous: string | undefined;

      function filtered(snapshot: AgentWork, key: Group["key"]): AgentWorkTask[] {
        const rows = snapshot[key];
        return selected === "all" ? rows : rows.filter((task) => task.workspace === selected);
      }

      function draw(snapshot: AgentWork): void {
        if (!snapshot.workspaces.includes(selected)) selected = "all";
        const current = now();
        updated.textContent = `Updated ${new Date(snapshot.updated_at).toLocaleTimeString()}`;
        const body = document.createElement("div");

        // Summary counts follow the workspace filter, like the rows.
        const counts = GROUPS.map((group) => ({ group, rows: filtered(snapshot, group.key) }));
        const summary = tileRow(counts.map(({ group, rows }) => {
          const link = document.createElement("a");
          link.href = `#${group.anchor}`;
          link.className = "agents-summary-link";
          link.append(tile(group.title, String(rows.length), group.key === "working_now" ? "working" : group.key === "done_today" ? "done" : group.key === "in_review" ? "attention" : "neutral"));
          link.addEventListener("click", (event) => {
            event.preventDefault();
            content.querySelector(`#${group.anchor}`)?.scrollIntoView();
          });
          return link;
        }), "Agent work at a glance");
        body.append(summary);

        // Workspace filter.
        const filter = document.createElement("div");
        filter.className = "agents-filter";
        filter.setAttribute("role", "group");
        filter.setAttribute("aria-label", "Workspace filter");
        for (const workspace of ["all", ...snapshot.workspaces]) {
          const chip = document.createElement("button");
          chip.type = "button";
          chip.className = "agents-chip";
          chip.textContent = workspace === "all" ? "All workspaces" : workspace;
          chip.dataset.workspace = workspace;
          chip.setAttribute("aria-pressed", String(selected === workspace));
          chip.addEventListener("click", () => {
            selected = workspace;
            draw(snapshot);
          });
          filter.append(chip);
        }
        body.append(filter);

        if (snapshot.unavailable.length > 0) {
          const notice = paragraph(`Some workspaces couldn't be read: ${snapshot.unavailable.join(", ")}.`);
          notice.className = "intro";
          body.append(notice);
        }

        const total = counts.reduce((sum, { rows }) => sum + rows.length, 0);
        if (total === 0) {
          body.append(emptyState("No agent work right now", "When agents pick up Orbit tasks in the workspaces you follow, they will show up here."));
        } else {
          for (const { group, rows } of counts) {
            if (rows.length === 0) continue;
            const section = document.createElement("section");
            section.className = "agents-group panel";
            section.id = group.anchor;
            section.setAttribute("aria-label", group.title);
            const groupHead = document.createElement("div");
            groupHead.className = "agents-group-head";
            const groupHeading = document.createElement("h2");
            groupHeading.textContent = group.title;
            groupHead.append(groupHeading, pill(String(rows.length)));
            section.append(groupHead);
            const list = document.createElement("div");
            list.className = "agents-list";
            const isExpanded = expanded.has(group.key);
            const visible = group.collapsible && !isExpanded ? rows.slice(0, COLLAPSED) : rows;
            list.append(...visible.map((task) => row(task, current, group.key)));
            section.append(list);
            if (group.collapsible && rows.length > COLLAPSED) {
              const toggle = document.createElement("button");
              toggle.type = "button";
              toggle.className = "agents-show-more";
              toggle.textContent = isExpanded ? "Show fewer" : `Show ${rows.length - COLLAPSED} more`;
              toggle.addEventListener("click", () => {
                if (isExpanded) expanded.delete(group.key);
                else expanded.add(group.key);
                draw(snapshot);
              });
              section.append(toggle);
            }
            body.append(section);
          }
        }

        const todayNote = paragraph("Proposed tasks waiting for approval are on ");
        const todayLink = document.createElement("a");
        todayLink.textContent = "Today";
        todayLink.href = "#today";
        todayNote.append(todayLink, document.createTextNode("."));
        body.append(todayNote);

        content.replaceChildren(body);
      }

      async function refresh(): Promise<void> {
        if (!root.contains(view) || !view.isConnected) return;
        try {
          const snapshot = await load();
          if (!root.contains(view) || !view.isConnected) return;
          const serialized = JSON.stringify(snapshot);
          if (serialized !== previous) {
            draw(snapshot);
            shown = snapshot;
            previous = serialized;
          }
          feedback.replaceChildren();
        } catch {
          if (!root.contains(view) || !view.isConnected) return;
          feedback.replaceChildren(errorState("Couldn't refresh agent work", previous === undefined
            ? "Please try again. We'll keep checking for agent work."
            : "The work below may have changed. We'll try again shortly."));
          if (previous === undefined) content.replaceChildren();
        } finally {
          content.setAttribute("aria-busy", "false");
          if (root.contains(view) && view.isConnected) window.setTimeout(() => { void refresh(); }, pollIntervalMs);
        }
      }
      void shown;
      await refresh();
    },
  };
}

export const screen: Screen = createAgentsScreen();
