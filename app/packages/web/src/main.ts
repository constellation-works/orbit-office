/**
 * UI entry: draw the navigation from the built screens and render the
 * one the location hash names (Today by default). Plain words only: no
 * ids, crews, runs or raw statuses. There is no Chat screen; conversation
 * happens in the user's agent CLI.
 */
import { navigableScreens, screenForHash } from "./router.js";

function drawNav(nav: HTMLElement): void {
  nav.replaceChildren(
    ...navigableScreens().map((screen) => {
      const link = document.createElement("a");
      link.href = `#${screen.id}`;
      link.textContent = screen.title;
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
if (nav !== null && root !== null) {
  drawNav(nav);
  void drawScreen(root);
  window.addEventListener("hashchange", () => void drawScreen(root));
}
