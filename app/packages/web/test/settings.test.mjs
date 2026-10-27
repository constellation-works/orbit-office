import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { audit } from "../../server/test/settings-fixture.mjs";
import { auditList, screen } from "../dist/screens/settings/index.js";
import { PRIVATE_ROUTE_WORDING, capabilityLine, sentencesForCapabilities } from "../dist/screens/settings/private.js";
import { MUSE_SPARK_OPEN_ATTRIBUTION, applyFooter, footerText } from "../dist/ui/attribution.js";

const original = globalThis.document;
afterEach(() => { globalThis.document = original; });

class Element {
  constructor(tag) { this.tagName = tag; this.children = []; this._text = ""; }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; this._text = ""; }
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

test("private route capability notes and the attribution follow a configured endpoint", () => {
  const audio = "this server doesn't take audio, so private videos can't be transcribed";
  assert.equal(PRIVATE_ROUTE_WORDING, "Private means not sent to Meta's hosted API, sent only to the endpoint you chose");
  assert.equal(MUSE_SPARK_OPEN_ATTRIBUTION, "Built with Muse Spark Open");
  assert.equal(capabilityLine({ capability: "audio", accepted: false, note: audio }), audio);
  assert.equal(capabilityLine({ capability: "audio", accepted: false, note: null }), audio);
  assert.equal(capabilityLine({ capability: "audio", accepted: false, note: "" }), audio);
  assert.match(capabilityLine({ capability: "text", accepted: true, note: null }), /text/);
  assert.deepEqual(sentencesForCapabilities(null), ["This endpoint has not been checked yet."]);
  assert.ok(sentencesForCapabilities([]).includes(audio));
  assert.ok(sentencesForCapabilities(["text"]).includes(audio));
  assert.equal(footerText(false), "");
  assert.equal(footerText(true), "Built with Muse Spark Open");
  globalThis.document = { createElement: (tag) => new Element(tag) };
  const footer = new Element("footer");
  applyFooter(footer, false);
  assert.equal(footer.textContent, "");
  applyFooter(footer, true);
  assert.equal(footer.textContent, "Built with Muse Spark Open");
  applyFooter(footer, false);
  assert.equal(footer.children.length, 0);
});
