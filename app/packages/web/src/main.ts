/**
 * UI entry: draw the navigation from the built screens and render the
 * one the location hash names (Today by default). Plain words only: no
 * ids, crews, runs or raw statuses. There is no Chat screen; conversation
 * happens in the user's agent CLI.
 */
import type { Today } from "@orbit-office/contracts";

import { screenHash } from "./api/source-location.js";
import { getJson, getPrivateRoute } from "./api/client.js";
import { drawRail, setRailCount } from "./nav.js";
import { registerServiceWorker } from "./pwa/register.js";
import { navigableScreens, screenForHash } from "./router.js";
import { PRIVATE_ROUTE_EVENT, RAIL_REFRESH_EVENT, applyFooter } from "./ui/index.js";

registerServiceWorker();

function drawNav(nav: HTMLElement): void {
  drawRail(nav, navigableScreens(), screenHash(window.location.search, window.location.hash).slice(1));
  void loadCounts(nav);
}

/**
 * Counts beside the rail entries: how many routines are on, and how many
 * cards wait on you today. A failed read hides the count; it never shows 0.
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
  ]);
}

async function drawScreen(root: HTMLElement): Promise<void> {
  const screen = screenForHash(screenHash(window.location.search, window.location.hash));
  if (screen?.render === undefined) {
    const note = document.createElement("p");
    note.textContent = "Nothing to show yet.";
    root.replaceChildren(note);
    return;
  }
  await screen.render(root);
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
