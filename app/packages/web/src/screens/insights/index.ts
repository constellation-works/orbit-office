/** Weekly automation and the work it has made possible. */
import { getJson } from "../../api/client.js";
import type { Screen } from "../../screen.js";
import { emptyState, errorState, panel, panelBody, pill, row, rows, screenHead, tile, tileRow } from "../../ui/index.js";

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

const STYLE = `
.insights { display:flex; flex-direction:column; gap:20px; min-width:0; }
.insights .tile-row { margin:0; grid-template-columns:repeat(auto-fit, minmax(min(100%, 150px), 1fr)); }
.insights > .panel + .panel { margin-top:0; }
.insights-chart { display:block; width:100%; max-width:900px; height:auto; color:var(--done); }
.insights-grid { stroke:var(--hair); stroke-width:1; }
.insights-grid--base { stroke:var(--border); }
.insights-axis { fill:var(--fg-mute); font-family:var(--font-sans); font-size:10px; }
.insights-weeks { display:flex; flex-wrap:wrap; gap:6px; padding:0; margin:0; list-style:none; }
.insights-weeks li { font-family:var(--font-mono); font-size:12px; padding:3px 9px; border-radius:999px; background:var(--bg-hover); color:var(--fg-dim); }
.insights-automated .panel-body { padding-top:4px; padding-bottom:4px; }
`;

function line(x1: number, y1: number, x2: number, y2: number, className: string): SVGElement {
  const node = document.createElementNS("http://www.w3.org/2000/svg", "line");
  node.setAttribute("x1", String(x1)); node.setAttribute("y1", String(y1)); node.setAttribute("x2", String(x2)); node.setAttribute("y2", String(y2));
  node.setAttribute("class", className);
  return node;
}
function label(x: number, y: number, text: string): SVGElement {
  const node = document.createElementNS("http://www.w3.org/2000/svg", "text");
  node.setAttribute("x", String(x)); node.setAttribute("y", String(y)); node.setAttribute("class", "insights-axis");
  node.textContent = text;
  return node;
}

export function drawInsights(data: Insights): HTMLElement {
  const root = document.createElement("div"); root.className = "insights";
  const style = document.createElement("style");
  style.textContent = STYLE;
  root.append(style);
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
  root.append(tileRow(tileValues.map(([name, value]) => tile(name, value)), "Totals"));

  const chart = panel("Automation over time", { meta: "Share of completed work per week" });
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 600 200"); svg.setAttribute("role", "img"); svg.setAttribute("aria-label", "Weekly share of work automated; details follow"); svg.setAttribute("class", "insights-chart");
  // Gridlines at 0%, 50% and 100%, with their labels, under the line.
  svg.append(line(40, 180, 590, 180, "insights-grid insights-grid--base"), line(40, 100, 590, 100, "insights-grid"), line(40, 20, 590, 20, "insights-grid"),
    label(0, 184, "0%"), label(0, 104, "50%"), label(0, 24, "100%"));
  let path = ""; let connected = false; const points: SVGElement[] = [];
  data.weeks.forEach((week, index) => {
    const ratio = week.metrics.automation.count_ratio;
    if (ratio === null) { connected = false; return; }
    const x = data.weeks.length === 1 ? 315 : 60 + index * 520 / (data.weeks.length - 1);
    const y = 180 - Math.max(0, Math.min(1, ratio)) * 160;
    path += `${connected ? "L" : "M"}${x} ${y} `; connected = true;
    const circle = document.createElementNS("http://www.w3.org/2000/svg", "circle");
    circle.setAttribute("cx", String(x)); circle.setAttribute("cy", String(y)); circle.setAttribute("r", "4"); circle.setAttribute("fill", "currentColor"); points.push(circle);
  });
  const trend = document.createElementNS("http://www.w3.org/2000/svg", "path");
  trend.setAttribute("d", path); trend.setAttribute("fill", "none"); trend.setAttribute("stroke", "currentColor"); trend.setAttribute("stroke-width", "2.5"); trend.setAttribute("stroke-linejoin", "round"); svg.append(trend, ...points);
  const weeks = document.createElement("ul"); weeks.className = "insights-weeks";
  for (const week of data.weeks) {
    const simulated = data.simulated_until !== undefined && week.since <= data.simulated_until && week.metrics.automation.total_count > 0;
    weeks.append(element("li", `${week.since}: ${percent(week.metrics.automation.count_ratio)}${simulated ? " · simulated" : ""}`));
  }
  chart.append(panelBody(element("p", "The share of completed work done for you each week."), svg, weeks));
  root.append(chart);

  const automated = panel("What got automated", { variant: "plain" });
  automated.className = "panel panel--plain insights-automated";
  if (metrics.conversions.length === 0) automated.append(panelBody(emptyState("Nothing automated yet.", "Accepted suggestions will appear here.")));
  else automated.append(panelBody(rows(metrics.conversions.map((conversion) => row(`${conversion.name} — ${conversion.proposal_title}`)))));
  root.append(automated);
  return root;
}

export function createInsightsScreen(load = () => getJson<Insights>("/api/insights")): Screen {
  return { id: "insights", title: "Insights", status: "built", async render(root) {
    const badge = document.createElement("div");
    const view = document.createElement("div");
    view.append(screenHead("Insights", "See how your work is changing.", badge));
    const content = document.createElement("div"); content.append(element("p", "Gathering your insights…")); view.append(content); root.replaceChildren(view);
    try {
      const data = await load();
      if (!root.contains(view)) return;
      if (data.simulated_until !== undefined) badge.replaceChildren(pill(`Seeded history until ${data.simulated_until} · simulated`));
      content.replaceChildren(drawInsights(data));
    }
    catch { if (root.contains(view)) content.replaceChildren(errorState("Couldn’t load your insights", "Please try again later.")); }
  } };
}
export const screen: Screen = createInsightsScreen();
