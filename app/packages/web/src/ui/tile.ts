import type { Tone } from "./pill.js";

/** One number and what it counts. */
export function tile(label: string, value: string, tone: Tone = "neutral"): HTMLElement {
  const element = document.createElement("article");
  element.className = "tile";
  if (tone !== "neutral") element.dataset.tone = tone;
  const name = document.createElement("p");
  name.className = "tile-label";
  name.textContent = label;
  const number = document.createElement("p");
  number.className = "tile-value";
  number.textContent = value;
  element.append(name, number);
  return element;
}

/** A row of tiles that wraps on narrow screens. */
export function tileRow(tiles: readonly HTMLElement[], label: string): HTMLElement {
  const row = document.createElement("div");
  row.className = "tile-row";
  row.setAttribute("role", "group");
  row.setAttribute("aria-label", label);
  row.append(...tiles);
  return row;
}
