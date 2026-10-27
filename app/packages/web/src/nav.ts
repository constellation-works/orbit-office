/**
 * The side rail: screens in labelled groups, each with an icon, some nested
 * under a parent, and a count beside the ones that have one. Shell-owned, like
 * `main.ts`; no screen depends on it.
 */
import type { ScreenId } from "@orbit-office/contracts";

import type { Screen } from "./screen.js";

type Icon = "today" | "todos" | "library" | "insights" | "automation" | "settings";

interface RailItem {
  /** The screen the item opens. A parent without its own screen opens its first child. */
  screen?: ScreenId;
  title?: string;
  icon: Icon;
  children?: readonly ScreenId[];
}

interface RailGroup {
  label: string;
  items: readonly RailItem[];
}

const RAIL: readonly RailGroup[] = [
  { label: "Work", items: [{ screen: "today", icon: "today" }, { screen: "todos", icon: "todos" }] },
  { label: "Knowledge", items: [{ screen: "library", icon: "library", children: ["source"] }] },
  { label: "Observe", items: [{ screen: "insights", icon: "insights", children: ["review"] }] },
  {
    label: "Manage",
    items: [
      { title: "Automation", icon: "automation", children: ["routines"] },
      { screen: "settings", icon: "settings" },
    ],
  },
];

const ICON_PATHS: Record<Icon, readonly string[]> = {
  today: ["M10 6.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z", "M10 2.5v1.5M10 16v1.5M2.5 10H4M16 10h1.5M4.7 4.7l1 1M14.3 14.3l1 1M4.7 15.3l1-1M14.3 5.7l1-1"],
  todos: ["M3.5 5.5l1.5 1.5 2.5-2.5M10.5 6h6M3.5 12.5l1.5 1.5 2.5-2.5M10.5 13h6"],
  library: ["M4 4h5a2 2 0 0 1 2 2v10a2 2 0 0 0-2-2H4z", "M16 4h-5a2 2 0 0 0-2 2v10a2 2 0 0 1 2-2h5z"],
  insights: ["M5 16v-6M10 16V4M15 16V8", "M2.5 16.5h15"],
  automation: ["M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14z", "M10 6v4l3 2"],
  settings: ["M4 6h12M4 10h12M4 14h12", "M8 4.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zM13 8.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2zM7 12.4a1.6 1.6 0 1 0 0 3.2 1.6 1.6 0 0 0 0-3.2z"],
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

/** A rail link. `own` is false for a parent that only opens its first child: it has no count and is never current. */
function link(id: ScreenId, title: string, className: string, current: string, own = true): HTMLAnchorElement {
  const anchor = document.createElement("a");
  anchor.className = className;
  anchor.href = `#${id}`;
  const label = document.createElement("span");
  label.className = "rail-label";
  label.textContent = title;
  anchor.append(label);
  if (!own) return anchor;
  anchor.dataset.screen = id;
  const count = document.createElement("span");
  count.className = "rail-count";
  count.hidden = true;
  anchor.append(count);
  if (id === current) anchor.setAttribute("aria-current", "page");
  return anchor;
}

/** Draw the rail for the built screens; `current` is the shown screen's id. */
export function drawRail(nav: HTMLElement, screens: readonly Screen[], current: string): void {
  const byId = new Map(screens.map((screen) => [screen.id, screen]));
  const placed = new Set<string>();
  const groups: HTMLElement[] = [];
  for (const group of RAIL) {
    const element = document.createElement("div");
    element.className = "rail-group";
    const heading = document.createElement("p");
    heading.className = "rail-group-label";
    heading.textContent = group.label;
    element.append(heading);
    for (const item of group.items) {
      const children = (item.children ?? []).flatMap((id) => {
        const screen = byId.get(id);
        return screen === undefined ? [] : [screen];
      });
      const own = item.screen === undefined ? undefined : byId.get(item.screen);
      const target = own ?? children[0];
      if (target === undefined) continue;
      const top = link(target.id, item.title ?? own?.title ?? target.title, "rail-item", current, own !== undefined);
      top.prepend(icon(item.icon));
      if (target.id === current || children.some((child) => child.id === current)) top.classList.add("rail-item--open");
      element.append(top);
      if (own !== undefined) placed.add(own.id);
      if (children.length > 0) {
        const list = document.createElement("div");
        list.className = "rail-children";
        for (const child of children) {
          list.append(link(child.id, child.title, "rail-child", current));
          placed.add(child.id);
        }
        element.append(list);
      }
    }
    if (element.children.length > 1) groups.push(element);
  }
  // A built screen the layout does not name yet still gets a way in.
  const rest = screens.filter((screen) => !placed.has(screen.id));
  if (rest.length > 0) {
    const element = document.createElement("div");
    element.className = "rail-group";
    for (const screen of rest) element.append(link(screen.id, screen.title, "rail-item", current));
    groups.push(element);
  }
  nav.replaceChildren(...groups);
}

/**
 * Show `text` beside a screen's rail entries, or hide the count when it is
 * null. An alert count (work waiting on you) is drawn as a pill.
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
