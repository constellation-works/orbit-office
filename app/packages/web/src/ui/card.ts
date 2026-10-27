import type { Tone } from "./pill.js";

/** A short label above the title naming what kind of item this is. */
export interface CardKicker {
  label: string;
  tone?: Tone;
}

/** A container for one item and its details. */
export function card(title: string, content: HTMLElement, action?: HTMLButtonElement, kicker?: CardKicker): HTMLElement {
  const article = document.createElement("article");
  article.className = "card";
  if (kicker !== undefined) {
    const label = document.createElement("p");
    label.className = "card-kicker";
    label.textContent = kicker.label;
    if (kicker.tone !== undefined && kicker.tone !== "neutral") label.dataset.tone = kicker.tone;
    article.append(label);
  }
  const heading = document.createElement("h3");
  heading.textContent = title;
  article.append(heading, content);
  if (action !== undefined) article.append(action);
  return article;
}
