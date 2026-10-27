/**
 * UI entry: draw the navigation from the built screens and render the
 * one the location hash names (Today by default). Plain words only: no
 * ids, crews, runs or raw statuses. There is no Chat screen; conversation
 * happens in the user's agent CLI.
 */
import { getPrivateRoute } from "./api/client.js";
import { registerServiceWorker } from "./pwa/register.js";
import { navigableScreens, screenForHash } from "./router.js";
import { PRIVATE_ROUTE_EVENT, applyFooter } from "./ui/index.js";

registerServiceWorker();

function drawNav(nav: HTMLElement): void {
  nav.replaceChildren(
    ...navigableScreens().map((screen) => {
      const link = document.createElement("a");
      link.href = `#${screen.id}`;
      link.textContent = screen.title;
      if (screen.id === (window.location.hash.slice(1) || "today")) {
        link.setAttribute("aria-current", "page");
      }
      return link;
    }),
  );
}

async function drawScreen(root: HTMLElement): Promise<void> {
  const screen = screenForHash(window.location.hash || "#today");
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
