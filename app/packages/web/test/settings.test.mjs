import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { audit } from "../../server/test/settings-fixture.mjs";
import { auditList, screen } from "../dist/screens/settings/index.js";

const original = globalThis.document;
afterEach(() => { globalThis.document = original; });

class Element {
  constructor(tag) { this.tagName = tag; this.children = []; this._text = ""; }
  append(...nodes) { this.children.push(...nodes); }
  set textContent(value) { this._text = value; this.children = []; }
  get textContent() { return this._text + this.children.map((child) => child.textContent).join(" "); }
}

test("Settings renders real gateway audit fixture rows newest first and only public caller labels", () => {
  globalThis.document = { createElement: (tag) => new Element(tag) };
  assert.equal(screen.status, "built");
  assert.equal(screen.id, "settings");
  const rendered = auditList(audit);
  const rows = rendered.children[0].children;
  assert.deepEqual(rows.map((row) => row.children[1].textContent), ["GET deals", "GET accounts", "GET contacts"]);
  assert.match(rows[0].textContent, /No HTTP response/);
  assert.match(rows[1].textContent, /HTTP 200/);
  assert.match(rows[1].textContent, /218 ms/);
  assert.match(rows[1].textContent, /Called by Codex/);
  assert.ok(!rendered.textContent.includes("private-task-id"));
  assert.ok(!rendered.textContent.includes("private-session"));
  assert.ok(!rendered.textContent.includes("gateway.http-get"));
  assert.ok(!rendered.textContent.includes("body_sha256"));
  const empty = auditList({ records: [], skipped: 2 });
  assert.match(empty.textContent, /No API calls yet/);
  assert.match(empty.textContent, /2 unreadable audit entries/);
});
