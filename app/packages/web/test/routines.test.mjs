import assert from "node:assert/strict";
import { test } from "node:test";
import { screen, scheduleWords } from "../dist/screens/routines/index.js";
import { navigableScreens } from "../dist/router.js";
test("routines are navigable and schedules use plain language", () => {
  assert.ok(navigableScreens().includes(screen));
  assert.equal(scheduleWords({ every_minutes: 15 }), "Every 15 minutes");
  assert.equal(scheduleWords({ every_minutes: 4320 }), "Every 3 days");
  assert.equal(scheduleWords({ cron: "30 7 * * 1" }), "Every Monday at 07:30");
  assert.equal(scheduleWords({ cron: "0 9 1 * *" }), "Every month on day 1 at 09:00");
  assert.equal(scheduleWords({ cron: "*/15 * * * *" }), "Every 15 minutes");
  assert.equal(scheduleWords({ deliveries_landed: { threshold: 3, max_wait_minutes: 360 } }), "After 3 deliveries or up to 360 minutes");
  assert.equal(scheduleWords({ cron: "0 9 1,15 * *" }), "Scheduled for minute 0, hour 9, day of the month 1 or day of the month 15, every month, every day of the week");
});

test("screen displays effective state, skip reason and retention refusal", async () => {
  class Element {
    constructor(tag) { this.tag = tag; this.children = []; this.listeners = {}; this.value = ""; this.checked = false; this.disabled = false; this.text = ""; }
    set textContent(value) { this.text = value; this.children = []; }
    get textContent() { return this.text + this.children.map(child => child.textContent).join(""); }
    append(...children) { this.children.push(...children); }
    replaceChildren(...children) { this.text = ""; this.children = children; }
    setAttribute() {}
    addEventListener(name, fn) { this.listeners[name] = fn; }
  }
  const oldDocument = globalThis.document, oldFetch = globalThis.fetch;
  globalThis.document = { createElement: tag => new Element(tag) };
  const calls = []; let enabled = false;
  globalThis.fetch = async (path, options = {}) => {
    if (path === "/api/routines") return new Response(JSON.stringify({ hidden_count: 35, routines_unavailable: false, routines: [{ kind: "auto_task", name: "meeting-sweep", description: "process meetings", schedule: { every_minutes: 15 }, enabled, skipped_reason: "seeded by plugin:x, which is switched off in this workspace" }] }));
    calls.push([path, JSON.parse(options.body)]);
    if (path.endsWith("toggle")) { enabled = true; return new Response('{}'); }
    return new Response(JSON.stringify({ error: "refused", reason: "Retention must be positive." }), { status: 400 });
  };
  try {
    const root = new Element("main"); await screen.render(root);
    assert.match(root.textContent, /Meeting sweep/);
    assert.match(root.textContent, /process meetings/);
    assert.match(root.textContent, /Every 15 minutes/);
    assert.match(root.textContent, /35 Orbit developer routines hidden/);
    assert.match(root.textContent, /Its plugin is switched off in this workspace/);
    // No recorded fire reads as "Never", not as a run that happened.
    assert.match(root.textContent, /Never/);
    const all = node => [node, ...node.children.flatMap(all)];
    const checkbox = all(root).find(node => node.tag === "input" && node.type === "checkbox");
    assert.equal(checkbox.checked, false); checkbox.checked = true; checkbox.listeners.change();
    await new Promise(resolve => setImmediate(resolve));
    assert.equal(checkbox.checked, true);
    assert.deepEqual(calls[0], ["/api/routines/toggle", { kind: "auto_task", name: "meeting-sweep", enabled: true }]);
    assert.match(root.textContent, /35 Orbit developer routines hidden/);
    const form = all(root).find(node => node.tag === "form");
    const select = all(form).find(node => node.tag === "select"); select.value = "meeting";
    const days = all(form).find(node => node.tag === "input"); days.value = "-1";
    form.listeners.submit({ preventDefault() {} });
    await new Promise(resolve => setImmediate(resolve));
    assert.match(form.textContent, /Retention must be positive/);
    assert.deepEqual(calls[1], ["/api/routines/retention", { kind: "meeting", value: -1 }]);
  } finally { globalThis.document = oldDocument; globalThis.fetch = oldFetch; }
});
