/** How a label reads at a glance: needs you, an agent is on it, finished, private, failed. */
export type Tone = "neutral" | "attention" | "working" | "done" | "private" | "danger";

/** A short, human-readable label. */
export function pill(label: string, tone: Tone = "neutral"): HTMLElement {
  const element = document.createElement("span");
  element.className = "pill";
  element.textContent = label;
  if (tone !== "neutral") element.dataset.tone = tone;
  return element;
}
