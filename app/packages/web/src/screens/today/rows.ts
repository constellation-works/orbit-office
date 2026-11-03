/**
 * Today's row-list pattern (specs/web-app.md, "Look"): a header bar with a
 * count over hairline rows, each with a kind icon, its title, a short meta
 * and its actions inline. Styles are the `.today-*` rules in static/style.css.
 */
import { sourceHref } from "../../api/source-location.js";
import { monoMeta } from "../../ui/index.js";

/** What a row's icon says: a need's kind, a group of pages, or a digest line. */
export type Glyph =
  | "todo" | "approval" | "script_output" | "proposal" | "gateway_draft" | "group" | "shared"
  | "contradiction" | "decided" | "shipped" | "chevron" | "plus";

const SVG = "http://www.w3.org/2000/svg";

/** [element, attributes] pairs drawn in a 16×16 box. */
const GLYPHS: Record<Glyph, ReadonlyArray<[string, Record<string, string>]>> = {
  todo: [["circle", { cx: "8", cy: "8", r: "5.5" }]],
  approval: [["circle", { cx: "8", cy: "8", r: "5.5" }], ["path", { d: "M8 2.5a5.5 5.5 0 0 1 0 11z", fill: "currentColor", stroke: "none" }]],
  script_output: [["circle", { cx: "8", cy: "8", r: "5.5" }], ["path", { d: "M8 2.5a5.5 5.5 0 0 1 0 11z", fill: "currentColor", stroke: "none" }]],
  proposal: [["circle", { cx: "8", cy: "8", r: "5.5", "stroke-dasharray": "2.4 2.2" }]],
  group: [["circle", { cx: "8", cy: "8", r: "5.5", "stroke-dasharray": "2.4 2.2" }]],
  shared: [["circle", { cx: "8", cy: "8", r: "5.5", "stroke-dasharray": "2.4 2.2" }]],
  gateway_draft: [["path", { d: "M5 11l6-6M6 5h5v5" }]],
  contradiction: [["path", { d: "M2 5h10l-2.5-2.5M14 11H4l2.5 2.5" }]],
  decided: [["path", { d: "M3.5 8.5l3 3 6-7" }]],
  shipped: [["circle", { cx: "5", cy: "4", r: "1.5" }], ["circle", { cx: "5", cy: "12", r: "1.5" }], ["circle", { cx: "11", cy: "8", r: "1.5" }], ["path", { d: "M5 5.5v5M5 8h4.5" }]],
  chevron: [["path", { d: "M6 4.5l3 3.5-3 3.5" }]],
  plus: [["path", { d: "M8 3v10M3 8h10" }]],
};

export function glyph(name: Glyph, className = "today-kind"): HTMLElement | SVGSVGElement {
  const svg = document.createElementNS(SVG, "svg");
  svg.setAttribute("viewBox", "0 0 16 16");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "1.6");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  for (const [tag, attributes] of GLYPHS[name]) {
    const shape = document.createElementNS(SVG, tag);
    for (const [key, value] of Object.entries(attributes)) shape.setAttribute(key, value);
    svg.append(shape);
  }
  if (className === "") return svg;
  // The wrapper carries the kind, so the stylesheet colours it.
  const wrap = document.createElement("span");
  wrap.className = className;
  wrap.dataset.kind = name;
  wrap.append(svg);
  return wrap;
}

/** A state dot leading a row: green for finished, cyan for an agent on it. */
export function dot(tone: "done" | "working"): HTMLElement {
  const element = document.createElement("span");
  element.className = "today-dot";
  element.dataset.tone = tone;
  return element;
}

export function text(tag: "p" | "span" | "div", value: string, className?: string): HTMLElement {
  const element = document.createElement(tag);
  element.textContent = value;
  if (className !== undefined) element.className = className;
  return element;
}

/** One quiet line where a list has nothing to show. */
export function quiet(value: string): HTMLElement {
  return text("p", value, "today-empty");
}

/** A list's header bar: its h2 (named by `id`) and a count. */
export function listHead(title: string, id: string, count: number | null): HTMLElement {
  const head = document.createElement("div");
  head.className = "today-list-head";
  const heading = document.createElement("h2");
  heading.id = id;
  heading.textContent = title;
  head.append(heading);
  if (count !== null) head.append(text("span", String(count), "today-count"));
  return head;
}

/** A section labelled by its header bar's h2, so tests and agents find it by role and name. */
export function listSection(id: string): HTMLElement {
  const section = document.createElement("section");
  section.className = "today-list";
  section.setAttribute("aria-labelledby", id);
  return section;
}

export function rowList(): HTMLUListElement {
  const list = document.createElement("ul");
  list.className = "today-rows";
  return list;
}

/** A short meta beside a row's title; amber when it needs attention. */
export function meta(value: string, attention = false): HTMLElement {
  const element = text("span", value, "today-meta");
  if (attention) element.dataset.tone = "attention";
  return element;
}

/** A citation such as `src-…#t=00:02:14` as a chip linking to that span; a task id as a plain chip. */
export function cite(value: string): HTMLElement {
  const match = /^(src-[a-z0-9-]+)(#.+)?$/.exec(value);
  if (match?.[1] === undefined) {
    const chip = monoMeta(value);
    chip.classList.add("today-cite");
    return chip;
  }
  const label = `${match[1].replace(/^src-\d{8}-/, "")}${match[2] === undefined ? "" : ` ${match[2]}`}`;
  const link = monoMeta(label, sourceHref(match[1], match[2] ?? "#source"));
  link.classList.add("today-cite");
  link.title = value;
  return link;
}

export function cites(values: readonly string[]): HTMLElement {
  const line = document.createElement("div");
  line.className = "today-cites";
  for (const value of values) line.append(cite(value));
  return line;
}
