import assert from "node:assert/strict";
import { test } from "node:test";
import { fixture } from "../../server/test/insights.test.mjs";
import { drawInsights, screen } from "../dist/screens/insights/index.js";

class Element {
  constructor(tag) { this.tag = tag; this.children = []; this.attributes = {}; this.classList = { add() {} }; this._text = ""; }
  append(...nodes) { this.children.push(...nodes); }
  prepend(...nodes) { this.children.unshift(...nodes); }
  setAttribute(key, value) { this.attributes[key] = value; }
  set textContent(value) { this._text = value; this.children = []; }
  get textContent() { return this._text + this.children.map((child) => child.textContent).join(" "); }
}
function all(node, tag) { return [...(node.tag === tag ? [node] : []), ...node.children.flatMap((child) => all(child, tag))]; }
test("Insights renders weekly ratios, gaps, simulation dates and plain proposal titles", () => {
  const original = globalThis.document;
  globalThis.document = { createElement: (tag) => new Element(tag), createElementNS: (_, tag) => new Element(tag) };
  try {
    assert.equal(screen.status, "built");
    const data = { ...fixture, totals: { ...fixture.totals, conversions: [{ name: "weekly reports", proposal_title: "Prepare weekly reports" }] } };
    const view = drawInsights(data);
    assert.match(view.textContent, /75%/); assert.match(view.textContent, /1.8/);
    assert.match(view.textContent, /weekly reports — Prepare weekly reports/);
    assert.equal(all(view, "a").length, 0);
    assert.equal(all(view, "li").filter((item) => item.textContent.includes("simulated")).length, 2);
    assert.equal(all(view, "circle").length, 2);
    assert.match(all(view, "path")[0].attributes.d, /^M[^M]+M/);
    const noDate = { ...data }; delete noDate.simulated_until;
    assert.equal(drawInsights(noDate).textContent.includes("simulated"), false);
  } finally { globalThis.document = original; }
});

test("Insights labels recorded seeded weeks but leaves empty and later weeks unlabelled", () => {
  const original = globalThis.document;
  globalThis.document = { createElement: (tag) => new Element(tag), createElementNS: (_, tag) => new Element(tag) };
  try {
    const recorded = fixture.totals;
    const empty = { ...recorded, automation: { ...recorded.automation, total_count: 0, automated_count: 0, count_ratio: null } };
    const view = drawInsights({ ...fixture, simulated_until: "2026-10-25", weeks: [
      { since: "2026-09-07", metrics: empty },
      { since: "2026-09-14", metrics: recorded },
      { since: "2026-10-26", metrics: recorded },
    ] });
    const weeks = all(view, "li").slice(0, 3);
    assert.deepEqual(weeks.map((item) => item.textContent.includes("simulated")), [false, true, false]);
  } finally { globalThis.document = original; }
});
