/**
 * Panels, rows and dots: the dashboard patterns from the approved mocks
 * (docs/design/office/mocks/). Styles are in static/style.css.
 */
import type { Tone } from "./pill.js";

/** A dot's colour: a state tone, or the accent for a decision. */
export type DotTone = Tone | "accent";

export interface PanelOptions {
  /** Small caps label above the title. */
  eyebrow?: string;
  /** Mono line on the right of the header. */
  meta?: string;
  /** `plain`: the title is itself the small caps label. `dashed`: a drop zone. */
  variant?: "plain" | "dashed";
  /** Heading id, for `aria-labelledby`. */
  id?: string;
}

/** A bordered section with a header. Append bodies with {@link panelBody}. */
export function panel(title: string, options: PanelOptions = {}): HTMLElement {
  const section = document.createElement("section");
  section.className = options.variant === undefined ? "panel" : `panel panel--${options.variant}`;
  const head = document.createElement("div");
  head.className = "panel-head";
  const titles = document.createElement("div");
  if (options.eyebrow !== undefined) titles.append(eyebrow(options.eyebrow));
  const heading = document.createElement("h2");
  heading.textContent = title;
  if (options.id !== undefined) {
    heading.id = options.id;
    section.setAttribute("aria-labelledby", options.id);
  }
  titles.append(heading);
  head.append(titles);
  if (options.meta !== undefined) {
    const meta = document.createElement("span");
    meta.className = "mono-meta";
    meta.textContent = options.meta;
    head.append(meta);
  }
  section.append(head);
  return section;
}

/** One padded body of a panel; `tone: "attention"` washes it amber. */
export function panelBody(...children: Node[]): HTMLElement {
  const body = document.createElement("div");
  body.className = "panel-body";
  body.append(...children);
  return body;
}

/** Panels in a wrapping grid. */
export function panelGrid(...panels: HTMLElement[]): HTMLElement {
  const grid = document.createElement("div");
  grid.className = "panel-grid";
  grid.append(...panels);
  return grid;
}

export function eyebrow(text: string): HTMLElement {
  const label = document.createElement("p");
  label.className = "eyebrow";
  label.textContent = text;
  return label;
}

export function dot(tone: DotTone = "neutral"): HTMLElement {
  const element = document.createElement("span");
  element.className = "dot";
  element.setAttribute("aria-hidden", "true");
  if (tone !== "neutral") element.dataset.tone = tone;
  return element;
}

/** Mono meta text, or a mono link when `href` is given. */
export function monoMeta(text: string, href?: string): HTMLElement {
  const element = document.createElement(href === undefined ? "span" : "a");
  element.className = "mono-meta";
  element.textContent = text;
  if (href !== undefined) (element as HTMLAnchorElement).href = href;
  return element;
}

export interface RowOptions {
  dot?: DotTone;
  /** Lines under the title (meta, citations). */
  details?: Node[];
  /** Pills, crews and links on the right. */
  trail?: Node[];
}

/** One hairline-separated row: an optional dot, a title with details, and a trail. */
export function row(title: string | Node, options: RowOptions = {}): HTMLLIElement {
  const item = document.createElement("li");
  item.className = "row";
  if (options.dot !== undefined) item.append(dot(options.dot));
  const main = document.createElement("div");
  main.className = "row-main";
  const heading = document.createElement("span");
  heading.className = "row-title";
  if (typeof title === "string") heading.textContent = title;
  else heading.append(title);
  main.append(heading, ...(options.details ?? []));
  item.append(main);
  if (options.trail !== undefined && options.trail.length > 0) {
    const trail = document.createElement("div");
    trail.className = "row-trail";
    trail.append(...options.trail);
    item.append(trail);
  }
  return item;
}

/** A list of {@link row}s. */
export function rows(items: readonly HTMLElement[], label?: string): HTMLUListElement {
  const list = document.createElement("ul");
  list.className = "rows";
  if (label !== undefined) list.setAttribute("aria-label", label);
  list.append(...items);
  return list;
}

/** A screen header: title and intro, with optional controls on the right. */
export function screenHead(title: string, intro: string, ...actions: HTMLElement[]): HTMLElement {
  const header = document.createElement("header");
  header.className = "screen-head";
  const left = document.createElement("div");
  const heading = document.createElement("h1");
  heading.textContent = title;
  const text = document.createElement("p");
  text.className = "intro";
  text.textContent = intro;
  left.append(heading, text);
  header.append(left);
  if (actions.length > 0) {
    const right = document.createElement("div");
    right.className = "screen-head-actions";
    right.append(...actions);
    header.append(right);
  }
  return header;
}
