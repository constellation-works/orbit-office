/**
 * The side rail: one flat list of screens, each with an icon and a count
 * beside the ones that have one, and Routines and Settings pinned at the
 * bottom. Shell-owned, like `main.ts`; no screen depends on it.
 */
import type { ScreenId } from "@orbit-office/contracts";

import type { Screen } from "./screen.js";

type Icon = "today" | "calendar" | "todos" | "scribe" | "library" | "insights" | "routines" | "settings";

interface RailItem {
  screen: ScreenId;
  icon: Icon;
  /** Screens opened from this one (Source from Library, Review from Insights): the item reads as open while one is shown. */
  within?: readonly ScreenId[];
}

const TOP: readonly RailItem[] = [
  { screen: "today", icon: "today" },
  { screen: "calendar", icon: "calendar" },
  { screen: "todos", icon: "todos" },
  { screen: "scribe", icon: "scribe" },
  { screen: "library", icon: "library", within: ["source"] },
  { screen: "insights", icon: "insights", within: ["review"] },
];

const BOTTOM: readonly RailItem[] = [
  { screen: "routines", icon: "routines" },
  { screen: "settings", icon: "settings" },
];

const ICON_PATHS: Record<Icon, readonly string[]> = {
  today: ["M10 6.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z", "M10 2.5v1.5M10 16v1.5M2.5 10H4M16 10h1.5"],
  calendar: ["M4 5h12v11.5H4z", "M4 8.5h12M7.5 3v3.5M12.5 3v3.5"],
  todos: ["M3.5 5.5l1.5 1.5 2.5-2.5M10.5 6h6M3.5 12.5l1.5 1.5 2.5-2.5M10.5 13h6"],
  scribe: ["M5 3.5h10v13H5z", "M7.5 7h5M7.5 10h5M7.5 13h3"],
  library: ["M4 4h5a2 2 0 0 1 2 2v10a2 2 0 0 0-2-2H4z", "M16 4h-5a2 2 0 0 0-2 2v10a2 2 0 0 1 2-2h5z"],
  insights: ["M5 16v-6M10 16V4M15 16V8"],
  routines: ["M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14z", "M10 6v4l3 2"],
  settings: ["M4 6h12M4 14h12", "M8 4.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zM12 12.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z"],
};

const SVG = "http://www.w3.org/2000/svg";

function icon(name: Icon): SVGSVGElement {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("class", "rail-icon");
  svg.setAttribute("viewBox", "0 0 20 20");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.5");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const d of ICON_PATHS[name]) {
    const path = document.createElementNS(SVG, "path");
    path.setAttribute("d", d);
    svg.append(path);
  }
  return svg;
}

/** A rail link with its (hidden until set) count. */
function link(screen: Screen, current: string): HTMLAnchorElement {
  const anchor = document.createElement("a");
  anchor.className = "rail-item";
  anchor.href = `#${screen.id}`;
  anchor.dataset.screen = screen.id;
  const label = document.createElement("span");
  label.className = "rail-label";
  label.textContent = screen.title;
  const count = document.createElement("span");
  count.className = "rail-count";
  count.hidden = true;
  anchor.append(label, count);
  if (screen.id === current) anchor.setAttribute("aria-current", "page");
  return anchor;
}

function list(className: string, links: readonly HTMLAnchorElement[]): HTMLElement | null {
  if (links.length === 0) return null;
  const element = document.createElement("div");
  element.className = className;
  element.append(...links);
  return element;
}

/** Draw the rail for the built screens; `current` is the shown screen's id. */
export function drawRail(nav: HTMLElement, screens: readonly Screen[], current: string): void {
  const byId = new Map(screens.map((screen) => [screen.id, screen]));
  const placed = new Set<string>();
  const draw = (items: readonly RailItem[]): HTMLAnchorElement[] => items.flatMap((item) => {
    const screen = byId.get(item.screen);
    if (screen === undefined) return [];
    placed.add(screen.id);
    const anchor = link(screen, current);
    anchor.prepend(icon(item.icon));
    const within = (item.within ?? []).filter((id) => byId.has(id));
    for (const id of within) placed.add(id);
    if (within.includes(current as ScreenId)) anchor.classList.add("rail-item--open");
    return [anchor];
  });
  const top = draw(TOP);
  const bottom = draw(BOTTOM);
  // A built screen the layout does not name yet still gets a way in.
  const rest = screens.filter((screen) => !placed.has(screen.id)).map((screen) => link(screen, current));
  nav.replaceChildren(...[list("rail-list", [...top, ...rest]), list("rail-list rail-list--end", bottom)].filter((element) => element !== null));
}

/**
 * Show `text` beside a screen's rail entry, or hide the count when it is
 * null. An alert count (work waiting on you) is drawn in amber.
 */
export function setRailCount(nav: HTMLElement, id: ScreenId, text: string | null, alert = false): void {
  for (const anchor of nav.querySelectorAll<HTMLAnchorElement>(`a[data-screen="${id}"]`)) {
    const count = anchor.querySelector<HTMLElement>(".rail-count");
    if (count === null) continue;
    count.textContent = text ?? "";
    count.hidden = text === null;
    count.classList.toggle("rail-count--alert", alert);
  }
}
