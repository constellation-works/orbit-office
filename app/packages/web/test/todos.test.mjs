import assert from "node:assert/strict";
import { test } from "node:test";

import { navigableScreens } from "../dist/router.js";
import { screen } from "../dist/screens/todos/index.js";
import { drawTodoScreen, modelFromPayload } from "../dist/screens/todos/view.js";

class Node {
  constructor() {
    this.childNodes = [];
    this.parentNode = null;
    this.connected = false;
  }

  get isConnected() {
    let node = this;
    while (node !== null) {
      if (node.connected === true) return true;
      node = node.parentNode;
    }
    return false;
  }

  append(...nodes) {
    this._text = null;
    for (const node of nodes) {
      if (node.parentNode !== null) {
        node.parentNode.childNodes = node.parentNode.childNodes.filter((child) => child !== node);
      }
      node.parentNode = this;
      this.childNodes.push(node);
    }
  }

  replaceChildren(...nodes) {
    for (const child of this.childNodes) child.parentNode = null;
    this.childNodes = [];
    if (nodes.length > 0) this.append(...nodes);
  }
}

class Element extends Node {
  constructor(tag) {
    super();
    this.tagName = tag.toUpperCase();
    this._className = "";
    this._text = null;
    this.attrs = {};
    this.listeners = {};
    this.disabled = false;
    this.type = "";
    this.href = "";
  }

  set className(value) {
    this._className = value;
  }

  get className() {
    return this._className;
  }

  set textContent(value) {
    this._text = String(value);
    for (const child of this.childNodes) child.parentNode = null;
    this.childNodes = [];
  }

  get textContent() {
    if (this.childNodes.length > 0) {
      return this.childNodes.map((child) => child.textContent ?? "").join("");
    }
    return this._text ?? "";
  }

  setAttribute(name, value) {
    this.attrs[name] = String(value);
  }

  addEventListener(type, fn) {
    (this.listeners[type] ??= []).push(fn);
  }

  click() {
    for (const fn of this.listeners.click ?? []) fn();
  }
}

globalThis.document = {
  createElement(tag) {
    return new Element(tag);
  },
};

function connectedRoot() {
  const root = document.createElement("div");
  root.connected = true;
  return root;
}

function collect(node, predicate, found = []) {
  if (predicate(node)) found.push(node);
  for (const child of node.childNodes ?? []) collect(child, predicate, found);
  return found;
}

function byClass(root, className) {
  return collect(root, (node) => typeof node.className === "string" && node.className.split(" ").includes(className));
}

const MODEL = {
  you: [
    {
      id: "ORB-YOU-1",
      title: "Send the weekly follow-up",
      source: "Customer Call",
      knowledge: ["Process New Meetings", "Pricing Notes"],
      closable: true,
      scheduled: null,
      schedulable: false,
    },
    {
      id: "ORB-YOU-2",
      title: "File last month's receipts",
      source: null,
      knowledge: ["Receipt Checklist"],
      closable: true,
      scheduled: null,
      schedulable: false,
    },
  ],
  agents: [
    {
      id: "ORB-AG-1",
      title: "Draft the pricing reply",
      source: "Customer Call",
      knowledge: ["Pricing Notes"],
      closable: false,
      scheduled: null,
      schedulable: false,
    },
    {
      id: "ORB-AG-2",
      title: "Summarize the Monday standup",
      source: "Monday Standup",
      knowledge: [],
      closable: false,
      scheduled: null,
      schedulable: false,
    },
  ],
  more: false,
};

test("the built To-dos screen is in the navigation", () => {
  assert.equal(screen.status, "built");
  assert.equal(screen.title, "To-dos");
  assert.ok(navigableScreens().some((item) => item.id === "todos" && item.render !== undefined));
});

test("you can close your to-dos and an agent to-do offers no close action", () => {
  const closed = [];
  const root = connectedRoot();
  drawTodoScreen(root, MODEL, { onClose: (id) => closed.push(id) });

  const you = byClass(root, "todos-column--you")[0];
  const agents = byClass(root, "todos-column--agents")[0];
  const youButtons = collect(you, (node) => node.tagName === "BUTTON");
  const agentButtons = collect(agents, (node) => node.tagName === "BUTTON");
  assert.deepEqual(youButtons.map((button) => button.textContent), ["Close", "Close"]);
  assert.equal(agentButtons.length, 0);
  assert.equal(agents.textContent.includes("Close"), false);
  assert.match(you.textContent, /Send the weekly follow-up/);
  assert.match(you.textContent, /Customer Call/);
  assert.match(you.textContent, /Process New Meetings/);
  assert.match(you.textContent, /No source attached/);
  assert.match(agents.textContent, /Draft the pricing reply/);
  assert.match(agents.textContent, /No knowledge attached/);

  const visible = root.textContent;
  for (const hidden of ["ORB-YOU-1", "ORB-YOU-2", "ORB-AG-1", "ORB-AG-2", "owner:you", "owner:agent", "backlog", "in-progress"]) {
    assert.equal(visible.includes(hidden), false, hidden);
  }
  const links = collect(root, (node) => node.tagName === "A");
  assert.ok(links.length >= 2);
  for (const link of links) assert.equal(link.href, "#source");
  for (const node of collect(root, () => true)) {
    for (const value of Object.values(node.attrs ?? {})) {
      assert.equal(String(value).includes("ORB-"), false);
    }
  }

  youButtons[0].click();
  assert.deepEqual(closed, ["ORB-YOU-1"]);
  assert.equal(youButtons[0].disabled, true);
  assert.equal(youButtons[0].textContent, "Closing");
});

test("a load failure and a close both go through the app server", async () => {
  const calls = [];
  globalThis.fetch = async (path, options = {}) => {
    calls.push({ path, options });
    if (path === "/api/todos" && options.method === undefined) {
      return new Response(JSON.stringify(MODEL));
    }
    if (path === "/api/todos/close") {
      return new Response(JSON.stringify({ ok: true }));
    }
    return new Response("no", { status: 500 });
  };
  const root = connectedRoot();
  await screen.render(root);
  const agents = byClass(root, "todos-column--agents")[0];
  assert.equal(collect(agents, (node) => node.tagName === "BUTTON").length, 0);
  const button = collect(byClass(root, "todos-column--you")[0], (node) => node.tagName === "BUTTON")[0];
  button.click();
  await new Promise((resolve) => setTimeout(resolve, 0));
  const post = calls.find((call) => call.path === "/api/todos/close");
  assert.equal(post.options.method, "POST");
  assert.deepEqual(JSON.parse(post.options.body), { id: "ORB-YOU-1" });
  root.replaceChildren();

  globalThis.fetch = async () => {
    throw new TypeError("offline");
  };
  const failed = connectedRoot();
  await screen.render(failed);
  assert.match(failed.textContent, /To-dos are unavailable/);
  failed.replaceChildren();
});

test("polling intervals are cleared after navigation away", async () => {
  const originalSetInterval = globalThis.setInterval;
  const originalClearInterval = globalThis.clearInterval;
  const intervals = new Map();
  const calls = [];
  globalThis.setInterval = (callback) => {
    const handle = { callback, unref() {} };
    intervals.set(handle, callback);
    return handle;
  };
  globalThis.clearInterval = (handle) => intervals.delete(handle);
  globalThis.fetch = async (path) => {
    calls.push(path);
    return new Response(JSON.stringify(MODEL));
  };

  try {
    const baseline = intervals.size;
    for (let visit = 0; visit < 3; visit += 1) {
      const root = connectedRoot();
      await screen.render(root);
      assert.equal(intervals.size, baseline + 1);

      const [[handle, callback]] = intervals.entries();
      const requestsBeforePoll = calls.length;
      callback();
      await new Promise((resolve) => setTimeout(resolve, 0));
      assert.equal(calls.length, requestsBeforePoll + 1);
      assert.equal(intervals.size, baseline + 1);

      const requestsBeforeNavigation = calls.length;
      root.replaceChildren();
      callback();

      assert.equal(intervals.has(handle), false);
      assert.equal(intervals.size, baseline);
      assert.equal(calls.length, requestsBeforeNavigation);
    }
  } finally {
    globalThis.setInterval = originalSetInterval;
    globalThis.clearInterval = originalClearInterval;
  }
});

// Local times, so the words hold in any time zone the suite runs in.
const FRIDAY_NINE = new Date(2026, 9, 30, 9, 0);
const WEDNESDAY = new Date(2026, 9, 28, 11, 30);
const SCHEDULING = {
  you: [{ ...MODEL.you[0], scheduled: FRIDAY_NINE.toISOString(), schedulable: true }],
  agents: [
    { ...MODEL.agents[0], scheduled: null, schedulable: true },
    { ...MODEL.agents[1], scheduled: new Date(2026, 9, 28, 9, 0).toISOString(), schedulable: false },
  ],
  more: false,
};
const buttonsIn = (node) => collect(node, (child) => child.tagName === "BUTTON");
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

test("a scheduled to-do reads its time in words, with Schedule… on yours and an agent's alike", () => {
  const root = connectedRoot();
  drawTodoScreen(root, SCHEDULING, { now: WEDNESDAY, onSchedule: async () => undefined, onEdit: () => undefined });
  const you = byClass(root, "todos-column--you")[0];
  const agents = byClass(root, "todos-column--agents")[0];
  assert.deepEqual(byClass(you, "todos-when").map((node) => node.textContent), [`Scheduled ${FRIDAY_NINE.toLocaleDateString(undefined, { weekday: "short" })} 09:00`]);
  assert.deepEqual(buttonsIn(you).map((button) => button.textContent), ["Reschedule…", "Clear schedule"]);
  assert.deepEqual(buttonsIn(agents).map((button) => button.textContent), ["Schedule…"], "a running agent to-do keeps its time and offers no schedule action");
  assert.equal(byClass(agents, "todos-when").length, 1);
  assert.equal(root.textContent.includes("ORB-"), false);
  const later = new Date(2026, 10, 13, 9, 0);
  const far = connectedRoot();
  drawTodoScreen(far, { you: [{ ...SCHEDULING.you[0], scheduled: later.toISOString() }], agents: [], more: false }, { now: WEDNESDAY });
  assert.match(byClass(far, "todos-when")[0].textContent, /^Scheduled \S+ 13 Nov 09:00$|^Scheduled \S+ Nov 13 09:00$/, "further off, the date is said too");
});

test("Schedule… opens a picker in this computer's time zone, holds the poll, and saves and clears through the app server", async () => {
  const originalSetInterval = globalThis.setInterval;
  const originalClearInterval = globalThis.clearInterval;
  let poll;
  globalThis.setInterval = (callback) => {
    poll = callback;
    return { unref() {} };
  };
  globalThis.clearInterval = () => undefined;
  const calls = [];
  globalThis.fetch = async (path, options = {}) => {
    calls.push({ path, options });
    if (path === "/api/todos/schedule") return new Response(JSON.stringify({ ok: true, scheduled: null }));
    return new Response(JSON.stringify(SCHEDULING));
  };
  try {
    const root = connectedRoot();
    await screen.render(root);
    const agents = () => byClass(root, "todos-column--agents")[0];
    buttonsIn(agents())[0].click();
    const form = byClass(agents(), "schedule-picker")[0];
    assert.ok(form, "the picker opens in the card");
    const input = collect(form, (node) => node.tagName === "INPUT")[0];
    assert.equal(input.type, "datetime-local");
    assert.match(byClass(form, "schedule-picker-note")[0].textContent, /^In this computer's time zone/);

    const gets = calls.length;
    poll();
    await flush();
    assert.equal(calls.length, gets, "the poll leaves an open picker alone");

    input.value = "2099-10-30T09:00";
    form.listeners.submit[0]({ preventDefault() {} });
    await flush();
    await flush();
    const set = calls.find((call) => call.path === "/api/todos/schedule");
    assert.equal(set.options.method, "POST");
    assert.deepEqual(JSON.parse(set.options.body), { id: "ORB-AG-1", not_before: new Date(2099, 9, 30, 9, 0).toISOString().replace(".000Z", "Z") });
    assert.equal(byClass(root, "schedule-picker").length, 0, "the picker closes once saved");

    const clear = buttonsIn(byClass(root, "todos-column--you")[0]).find((button) => button.textContent === "Clear schedule");
    clear.click();
    await flush();
    const cleared = calls.filter((call) => call.path === "/api/todos/schedule").at(-1);
    assert.deepEqual(JSON.parse(cleared.options.body), { id: "ORB-YOU-1", not_before: null });
    root.replaceChildren();
  } finally {
    globalThis.setInterval = originalSetInterval;
    globalThis.clearInterval = originalClearInterval;
  }
});

test("the picker refuses an empty or past time without writing", async () => {
  let saved = 0;
  const root = connectedRoot();
  drawTodoScreen(root, SCHEDULING, { now: WEDNESDAY, editing: "ORB-AG-1", onSchedule: async () => { saved += 1; }, onEdit: () => undefined });
  const form = byClass(root, "schedule-picker")[0];
  const input = collect(form, (node) => node.tagName === "INPUT")[0];
  const status = byClass(form, "schedule-picker-status")[0];
  input.value = "";
  form.listeners.submit[0]({ preventDefault() {} });
  assert.equal(status.textContent, "Choose a date and a time.");
  input.value = "2026-10-27T09:00";
  form.listeners.submit[0]({ preventDefault() {} });
  assert.equal(status.textContent, "Choose a time that is still to come.");
  assert.equal(saved, 0);
});

test("a payload that is not a to-do list is refused", () => {
  assert.equal(modelFromPayload({ you: [], agents: [{ title: "Missing pieces" }] }), null);
  assert.equal(modelFromPayload({ you: [{ ...MODEL.you[0], scheduled: "Friday" }], agents: [] }), null);
  assert.equal(modelFromPayload({ you: [{ ...MODEL.you[0], schedulable: undefined }], agents: [] }), null);
  const root = connectedRoot();
  drawTodoScreen(root, { you: [], agents: [], more: true });
  assert.match(root.textContent, /All clear/);
  assert.match(root.textContent, /Nothing in progress/);
  assert.match(root.textContent, /Some to-dos are not listed yet/);
});
