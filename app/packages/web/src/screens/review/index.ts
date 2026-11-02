/** Generated reviews, citations, and a request for another review. */
import type { ReviewList } from "@orbit-office/contracts";
import type { Screen } from "../../screen.js";
import { getJson } from "../../api/client.js";
import { emptyState, errorState, panel, panelBody, renderMarkdown, screenHead } from "../../ui/index.js";

const STYLE = `
.review { display:flex; flex-direction:column; gap:20px; min-width:0; }
.review .screen-head { margin:0; }
.review-status:empty { display:none; }
.review-status { margin:0; font-size:14px; color:var(--fg-dim); }
.review-list { display:flex; flex-direction:column; gap:20px; min-width:0; }
.review-list > .panel + .panel { margin-top:0; }
.review-body { gap:10px; }
.review-line { margin:0; font-size:15px; line-height:1.6; color:var(--fg); overflow-wrap:anywhere; }
.review-line a.mono-meta { margin:0 4px; }
.review-note { margin:0; font-size:13px; color:var(--fg-mute); }
`;

function draw(root: HTMLElement, list: ReviewList): void {
  const style = document.createElement("style");
  style.textContent = STYLE;
  const page = document.createElement("section"); page.className = "review";
  const button = document.createElement("button"); button.type = "button"; button.className = "secondary-action review-action"; button.textContent = "Regenerate";
  const message = document.createElement("p"); message.className = "review-status"; message.setAttribute("role", "status");
  button.addEventListener("click", () => {
    button.disabled = true; message.textContent = "Requesting a review.";
    void fetch("/api/review/regenerate", { method: "POST" }).then((response) => {
      message.textContent = response.ok ? "A new review was requested." : response.status === 403 ? "A new review cannot be requested here. Ask your workspace owner for access." : "Could not request a review. Try again in a moment.";
    }).catch(() => { message.textContent = "Could not request a review. Try again in a moment."; }).finally(() => { button.disabled = false; });
  });
  page.append(screenHead("Review", "Your reviews and the sources behind them.", button), message);
  if (list.latest_100) { const notice = document.createElement("p"); notice.className = "mono-meta review-note"; notice.textContent = "Showing the latest 100"; page.append(notice); }
  const reviews = document.createElement("div"); reviews.className = "review-list";
  if (list.reviews.length === 0) reviews.append(emptyState("No reviews yet.", "Generated reviews appear here with their sources."));
  for (const review of list.reviews) {
    const body = panelBody(); body.className = "panel-body review-body";
    if (review.body === null) { const note = document.createElement("p"); note.className = "review-note"; note.textContent = "The review text is unavailable."; body.append(note); }
    else body.append(renderMarkdown(review.body, {
      className: "review-line",
      citePrefixes: ["src", "dom", "rb", "art", "ORB"],
      // Citations are rendered as links with plain labels; the target retains the anchor.
      cite: (citation) => {
        const link = document.createElement("a"); link.className = "mono-meta"; link.textContent = "View source";
        const [id, anchor] = citation.split("#", 2);
        link.href = `/?citation=${encodeURIComponent(id ?? "")}${anchor === undefined ? "" : `#${encodeURIComponent(anchor).replace(/%3D/gi, "=")}`}`;
        return link;
      },
    }));
    if (review.truncated) { const note = document.createElement("p"); note.className = "review-note"; note.textContent = "This review is shortened."; body.append(note); }
    const item = panel(review.title, { eyebrow: "Review", meta: new Date(review.date).toLocaleDateString() });
    item.append(body);
    reviews.append(item);
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
