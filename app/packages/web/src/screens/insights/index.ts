/** Weekly automation and the work it has made possible. */
import { getJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { card, emptyState, errorState } from "../../ui/index.js";

interface Share { ratio: number | null }
interface Metrics {
  automation: { count_ratio: number | null; minutes_ratio: number | null; automated_count: number; total_count: number };
  hours_returned: number;
  first_pass_acceptance: Share;
  knowledge_coverage: Share;
  proposal_acceptance: Share;
  open_load: unknown[];
  conversions: { name: string; proposal_title: string }[];
}
interface Insights { totals: Metrics; weeks: { since: string; metrics: Metrics }[]; simulated_until?: string }

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.textContent = text; return node;
}
function percent(value: number | null): string { return value === null ? "Not enough data" : `${Math.round(value * 100)}%`; }

export function drawInsights(data: Insights): HTMLElement {
  const root = document.createElement("div");
  const style = document.createElement("style");
  style.textContent = `.insights-tiles{display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:1rem}.insights-tiles article{min-width:0;padding:1rem}.insights-tiles h3{font-size:1rem}.insights-value{font-size:1.5rem;font-weight:700}.insights-chart{width:100%;height:auto;max-height:260px}.insights-weeks{display:flex;flex-wrap:wrap;gap:.5rem 1rem;padding:0;list-style:none}.insights-weeks li{font-size:.85rem}.insights-list li{margin:.6rem 0}`;
  root.append(style);
  const tiles = document.createElement("div"); tiles.className = "insights-tiles";
  const metrics = data.totals;
  const tileValues: [string, string][] = [
    ["Work automated", percent(metrics.automation.count_ratio)],
    ["Time automated", percent(metrics.automation.minutes_ratio)],
    ["Hours returned", metrics.hours_returned.toLocaleString(undefined, { maximumFractionDigits: 1 })],
    ["Accepted first time", percent(metrics.first_pass_acceptance.ratio)],
    ["Work using knowledge", percent(metrics.knowledge_coverage.ratio)],
    ["Suggestions accepted", percent(metrics.proposal_acceptance.ratio)],
    ["Open to-dos", String(metrics.open_load.length)],
  ];
  for (const [label, value] of tileValues) {
    const number = element("p", value); number.className = "insights-value";
    tiles.append(card(label, number));
  }
  root.append(tiles, element("h2", "Automation over time"), element("p", "The share of completed work done for you each week."));
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 600 200"); svg.setAttribute("role", "img"); svg.setAttribute("aria-label", "Weekly share of work automated; details follow"); svg.classList.add("insights-chart");
  let path = ""; let connected = false;
  data.weeks.forEach((week, index) => {
    const ratio = week.metrics.automation.count_ratio;
    if (ratio === null) { connected = false; return; }
    const x = data.weeks.length === 1 ? 300 : 20 + index * 560 / (data.weeks.length - 1);
    const y = 180 - Math.max(0, Math.min(1, ratio)) * 160;
    path += `${connected ? "L" : "M"}${x} ${y} `; connected = true;
    const circle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    circle.setAttribute("cx", String(x)); circle.setAttribute("cy", String(y)); circle.setAttribute("r", "4"); circle.setAttribute("fill", "currentColor"); svg.append(circle);
  });
  const line = document.createElementNS("http://www.w3.org/2000/svg", "path");
  line.setAttribute("d", path); line.setAttribute("fill", "none"); line.setAttribute("stroke", "currentColor"); line.setAttribute("stroke-width", "3"); svg.prepend(line); root.append(svg);
  const weeks = document.createElement("ul"); weeks.className = "insights-weeks";
  for (const week of data.weeks) {
    const simulated = data.simulated_until !== undefined && week.since <= data.simulated_until && week.metrics.automation.total_count > 0;
    weeks.append(element("li", `${week.since}: ${percent(week.metrics.automation.count_ratio)}${simulated ? " · simulated" : ""}`));
  }
  root.append(weeks, element("h2", "What got automated"));
  if (metrics.conversions.length === 0) root.append(emptyState("Nothing automated yet.", "Accepted suggestions will appear here."));
  else {
    const list = document.createElement("ul"); list.className = "insights-list";
    for (const conversion of metrics.conversions) list.append(element("li", `${conversion.name} — ${conversion.proposal_title}`));
    root.append(list);
  }
  return root;
}

export function createInsightsScreen(load = () => getJson<Insights>("/api/insights")): Screen {
  return { id: "insights", title: "Insights", status: "built", async render(root) {
    const view = document.createElement("div"); view.append(element("h1", "Insights"), element("p", "See how your work is changing."));
    const content = document.createElement("div"); content.append(element("p", "Gathering your insights…")); view.append(content); root.replaceChildren(view);
    try { const data = await load(); if (root.contains(view)) content.replaceChildren(drawInsights(data)); }
    catch { if (root.contains(view)) content.replaceChildren(errorState("Couldn’t load your insights", "Please try again later.")); }
  } };
}
export const screen: Screen = createInsightsScreen();
