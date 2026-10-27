import assert from "node:assert/strict";
import { test } from "node:test";
import { screen } from "../dist/screens/review/index.js";
class Element {
  constructor(tag) { this.tagName = tag.toUpperCase(); this.children = []; this.listeners = {}; this.isConnected = true; this._text = ""; }
  set textContent(value) { this._text = String(value); this.children = []; }
  get textContent() { return this._text + this.children.map((c) => c.textContent).join(""); }
  append(...children) { this.children.push(...children); }
  replaceChildren(...children) { this.children = children; }
  setAttribute() {}
  addEventListener(name, fn) { this.listeners[name] = fn; }
  click() { this.listeners.click(); }
}
globalThis.document = { createElement: (tag) => new Element(tag) };
function collect(node, tag) { return [...(node.tagName === tag ? [node] : []), ...node.children.flatMap((child) => collect(child, tag))]; }
test("reviews render citation anchors and a plain refusal", async () => {
  const calls = [];
  globalThis.fetch = async (path, options) => { calls.push([path, options]); return path === "/api/review" ? new Response(JSON.stringify({ latest_100: true, reviews: [{ title: "Quarterly review", date: "2026-10-01T00:00:00Z", body: "Outcome [src-20261001-call#L1]\nDecision [ORB-123]", truncated: false }] })) : new Response("{}", { status: 403 }); };
  const root = new Element("div"); await screen.render(root);
  const links = collect(root, "A"); assert.equal(links.length, 2);
  assert.match(links[0].href, /src-20261001-call#L1/); assert.match(links[1].href, /ORB-123/);
  assert.match(root.textContent, /Showing the latest 100/); assert.doesNotMatch(root.textContent, /ORB-123/);
  collect(root, "BUTTON")[0].click(); await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(root.textContent, /cannot be requested here/); assert.equal(calls[1][0], "/api/review/regenerate");
});
