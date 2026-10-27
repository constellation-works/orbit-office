import { card, emptyState, errorState, pill, primaryAction, provenanceLink } from "./index.js";

const root = document.getElementById("demo");
if (root !== null) {
  const intro = document.createElement("p");
  intro.className = "intro";
  intro.textContent = "A few examples of the pieces that bring your work together.";
  const heading = document.createElement("h1");
  heading.textContent = "A place for your work";
  const grid = document.createElement("div");
  grid.className = "card-grid";
  for (const [title, kind, label] of [
    ["A decision to make", "approve", "Ready for you"],
    ["A note to shape", "edit", "In progress"],
    ["A suggestion to consider", "dismiss", "For later"],
  ] as const) {
    const content = document.createElement("div");
    content.className = "card-content";
    const detail = document.createElement("p");
    detail.textContent = "Keep the next step clear and close at hand.";
    content.append(pill(label), detail, provenanceLink("See where this came from", "#source"));
    const action = primaryAction(kind, () => { action.textContent = "Done"; action.disabled = true; });
    grid.append(card(title, content, action));
  }
  const states = document.createElement("div");
  states.className = "state-grid";
  states.append(emptyState("All clear", "There is nothing here yet."), errorState("Something went wrong", "Please try again in a moment."));
  root.append(heading, intro, grid, states);
}
