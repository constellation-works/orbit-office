/**
 * UI entry: draw the navigation from the built screens and render the
 * one the location hash names (Today by default). Plain words only: no
 * ids, crews, runs or raw statuses. There is no Chat screen; conversation
 * happens in the user's agent CLI.
 */
import type { AgentWork, SetupResult, SetupStatus, Today } from "@orbit-office/contracts";

import { screenHash } from "./api/source-location.js";
import { getCalendar, getJson, getPrivateRoute, postJson } from "./api/client.js";
import { drawRail, setRailCount } from "./nav.js";
import { registerServiceWorker } from "./pwa/register.js";
import { navigableScreens, screenForHash } from "./router.js";
import { meetingsWithoutNotes, startOfDay } from "./screens/calendar/model.js";
import { PRIVATE_ROUTE_EVENT, RAIL_REFRESH_EVENT, applyFooter, card, requestRailRefresh } from "./ui/index.js";

registerServiceWorker();

function drawNav(nav: HTMLElement): void {
  drawRail(nav, navigableScreens(), screenHash(window.location.search, window.location.hash).slice(1));
  void loadCounts(nav);
}

/**
 * Counts beside the rail entries: how many routines are on, how many cards
 * wait on you today, how many of today's past meetings have no notes yet,
 * and how many tasks are in Working now (both plain counts, not alerts).
 * A failed read hides the count; it never shows 0.
 */
async function loadCounts(nav: HTMLElement): Promise<void> {
  await Promise.all([
    getJson<unknown>("/api/routines")
      .then((data) => {
        const routines = typeof data === "object" && data !== null && "routines" in data ? data.routines : undefined;
        if (!Array.isArray(routines)) throw new Error("malformed routines");
        const on = routines.filter((routine: unknown) => typeof routine === "object" && routine !== null && "enabled" in routine && routine.enabled === true).length;
        setRailCount(nav, "routines", `${on}/${routines.length}`);
      })
      .catch(() => setRailCount(nav, "routines", null)),
    getJson<Partial<Today>>("/api/today")
      .then(({ needs_you }) => {
        if (!Array.isArray(needs_you)) throw new Error("malformed today");
        // Grouped proposals share one card, so count cards, as Today does.
        const cards = new Set(needs_you.map((item) => item.group?.key ?? `item:${item.key}`)).size;
        setRailCount(nav, "today", cards > 0 ? String(cards) : null, true);
      })
      .catch(() => setRailCount(nav, "today", null)),
    (async () => {
      const now = new Date();
      const day = startOfDay(now);
      const view = await getCalendar(day, new Date(day.getFullYear(), day.getMonth(), day.getDate() + 1));
      if (!Array.isArray(view.meetings)) throw new Error("malformed calendar");
      const count = meetingsWithoutNotes(view.meetings, now);
      setRailCount(nav, "calendar", count > 0 ? String(count) : null);
    })().catch(() => setRailCount(nav, "calendar", null)),
    getJson<AgentWork>("/api/agents")
      .then((data) => {
        if (!Array.isArray(data.working_now)) throw new Error("malformed agents");
        setRailCount(nav, "agents", data.working_now.length > 0 ? String(data.working_now.length) : null);
      })
      .catch(() => setRailCount(nav, "agents", null)),
  ]);
}

let screenGeneration = 0;
async function drawScreen(root: HTMLElement): Promise<void> {
  const generation = ++screenGeneration;
  const screen = screenForHash(screenHash(window.location.search, window.location.hash));
  if (screen?.render === undefined) {
    const note = document.createElement("p");
    note.textContent = "Nothing to show yet.";
    root.replaceChildren(note);
    return;
  }
  let setup: SetupStatus;
  try { setup = await getJson<SetupStatus>("/api/setup"); }
  catch { setup = { state: "unavailable", reason: "Office could not check your Library.", fix: "Run orbit-office init in your workspace." }; }
  if (generation !== screenGeneration) return;
  if (setup.state === "ready") { await screen.render(root); return; }
  if (screen.id !== "today") {
    const section = document.createElement("section");
    const heading = document.createElement("h1");
    heading.textContent = setup.state === "missing" ? "Your Library isn’t set up yet" : "Your Library is unavailable";
    const link = document.createElement("a");
    link.href = "#today";
    link.textContent = "Go to Today";
    section.append(heading, link);
    root.replaceChildren(section);
    return;
  }
  const view = document.createElement("div");
  view.className = "today";
  const heading = document.createElement("h1");
  heading.textContent = "Today";
  const content = document.createElement("div");
  content.className = "card-content";
  const description = document.createElement("p");
  description.textContent = "Create your Library’s folders, configuration, templates and index.";
  content.append(description);
  const message = document.createElement("p");
  message.setAttribute("role", "status");
  if (setup.reason) message.textContent = `${setup.reason} ${setup.fix ?? ""}`;
  content.append(message);
  let button: HTMLButtonElement | undefined;
  if (setup.state === "missing") {
    button = document.createElement("button");
    button.className = "primary-action";
    button.type = "button";
    button.textContent = "Set up";
    button.addEventListener("click", () => {
      if (button === undefined || button.disabled) return;
      button.disabled = true;
      message.textContent = "Setting up your Library…";
      void postJson<SetupResult | SetupStatus>("/api/setup", {}).then((result) => {
        if (generation !== screenGeneration) return;
        if ("already_initialized" in result) {
          requestRailRefresh();
          void drawScreen(root);
        } else {
          message.textContent = `${result.reason ?? "Setup could not finish."} ${result.fix ?? "Run orbit-office init in your workspace."}`;
          if (button !== undefined && result.state !== "plugin_disabled" && result.state !== "workspace_unregistered") button.disabled = false;
          else button?.remove();
        }
      }).catch(() => {
        if (generation !== screenGeneration) return;
        message.textContent = "Setup could not finish. Run orbit-office init in your workspace, then try again.";
        if (button !== undefined) button.disabled = false;
      });
    });
  }
  view.append(heading, card("Set up your Library", content, button, { label: "Get started", tone: "attention" }));
  root.replaceChildren(view);
}

const nav = document.getElementById("nav");
const root = document.getElementById("app");
const footer = document.getElementById("site-footer");
if (nav !== null && root !== null) {
  drawNav(nav);
  void drawScreen(root);
  window.addEventListener("hashchange", () => {
    drawNav(nav);
    void drawScreen(root);
  });
  window.addEventListener(RAIL_REFRESH_EVENT, () => { void loadCounts(nav); });
}
if (footer !== null) {
  // A save can land while this read is in flight. The newer generation wins,
  // so a stale "not configured" cannot hide the line after the endpoint is saved.
  let footerGeneration = 0;
  const loadFooter = (): void => {
    const generation = footerGeneration;
    void getPrivateRoute()
      .then((data) => {
        if (generation !== footerGeneration) return;
        applyFooter(footer, data.route !== null);
      })
      .catch(() => {
        if (generation !== footerGeneration) return;
        applyFooter(footer, false);
      });
  };
  loadFooter();
  window.addEventListener(PRIVATE_ROUTE_EVENT, (event: Event) => {
    footerGeneration += 1;
    const detail: unknown = event instanceof CustomEvent ? event.detail : undefined;
    const configured = typeof detail === "object" && detail !== null && "configured" in detail && detail.configured === true;
    applyFooter(footer, configured);
  });
}
