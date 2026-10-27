/** Generated reviews, citations, and a request for another review. */
import type { ReviewList } from "@orbit-office/contracts";
import type { Screen } from "../../screen.js";
import { getJson } from "../../api/client.js";
import { card, errorState } from "../../ui/index.js";

function draw(root: HTMLElement, list: ReviewList): void {
  const style = document.createElement("style");
  style.textContent = `.review-list { display: grid; gap: 16px; } .review-line { white-space: pre-wrap; overflow-wrap: anywhere; margin: .5rem 0; } .review-action { margin: 1rem 0; }`;
  const page = document.createElement("section");
  const title = document.createElement("h1"); title.textContent = "Review";
  const intro = document.createElement("p"); intro.className = "intro"; intro.textContent = "Your reviews and the sources behind them.";
  const button = document.createElement("button"); button.type = "button"; button.className = "primary-action review-action"; button.textContent = "Regenerate";
  const message = document.createElement("p"); message.setAttribute("role", "status");
  button.addEventListener("click", () => {
    button.disabled = true; message.textContent = "Requesting a review.";
    void fetch("/api/review/regenerate", { method: "POST" }).then((response) => {
      message.textContent = response.ok ? "A new review was requested." : response.status === 403 ? "A new review cannot be requested here. Ask your workspace owner for access." : "Could not request a review. Try again in a moment.";
    }).catch(() => { message.textContent = "Could not request a review. Try again in a moment."; }).finally(() => { button.disabled = false; });
  });
  page.append(title, intro, button, message);
  if (list.latest_100) { const notice = document.createElement("p"); notice.textContent = "Showing the latest 100"; page.append(notice); }
  const reviews = document.createElement("div"); reviews.className = "review-list";
  if (list.reviews.length === 0) { const empty = document.createElement("p"); empty.textContent = "No reviews yet."; reviews.append(empty); }
  for (const review of list.reviews) {
    const content = document.createElement("div"); content.className = "card-content";
    const date = document.createElement("p"); date.textContent = new Date(review.date).toLocaleDateString(); content.append(date);
    if (review.body === null) { const note = document.createElement("p"); note.textContent = "The review text is unavailable."; content.append(note); }
    else for (const line of review.body.split("\n")) {
      if (!line.trim()) continue;
      const row = document.createElement("p"); row.className = "review-line";
      // Citations are rendered as links with plain labels; the target retains the anchor.
      const pattern = /\[((?:src|dom|rb|art|ORB)-[^\]\s,;]+(?:\s*[,;]\s*(?:src|dom|rb|art|ORB)-[^\]\s,;]+)*)\]/g;
      let start = 0;
      for (const match of line.matchAll(pattern)) {
        const text = document.createElement("span"); text.textContent = line.slice(start, match.index); row.append(text);
        for (const citation of (match[1] ?? "").split(/\s*[,;]\s*/)) {
          const link = document.createElement("a"); link.textContent = "View source";
          const [id, anchor] = citation.split("#", 2);
          link.href = `/?citation=${encodeURIComponent(id ?? "")}${anchor === undefined ? "" : `#${encodeURIComponent(anchor).replace(/%3D/gi, "=")}`}`; row.append(link);
        }
        start = (match.index ?? 0) + match[0].length;
      }
      const rest = document.createElement("span"); rest.textContent = line.slice(start); row.append(rest); content.append(row);
    }
    if (review.truncated) { const note = document.createElement("p"); note.textContent = "This review is shortened."; content.append(note); }
    reviews.append(card(review.title, content));
  }
  page.append(reviews); root.replaceChildren(style, page);
}
async function render(root: HTMLElement): Promise<void> {
  const loading = document.createElement("p"); loading.textContent = "Loading reviews."; root.replaceChildren(loading);
  try {
    const list = await getJson<ReviewList>("/api/review");
    if (!Array.isArray(list.reviews) || typeof list.latest_100 !== "boolean" || list.reviews.some((item) => typeof item.title !== "string" || typeof item.date !== "string" || (item.body !== null && typeof item.body !== "string") || typeof item.truncated !== "boolean")) throw new Error("invalid response");
    if (loading.isConnected) draw(root, list);
  } catch { if (loading.isConnected) root.replaceChildren(errorState("Reviews are unavailable", "Try again in a moment.")); }
}
export const screen: Screen = { id: "review", title: "Review", status: "built", render };
