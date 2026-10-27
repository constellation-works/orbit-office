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
    },
    {
      id: "ORB-YOU-2",
      title: "File last month's receipts",
      source: null,
      knowledge: ["Receipt Checklist"],
      closable: true,
    },
  ],
  agents: [
    {
      id: "ORB-AG-1",
      title: "Draft the pricing reply",
      source: "Customer Call",
      knowledge: ["Pricing Notes"],
      closable: false,
    },
    {
      id: "ORB-AG-2",
      title: "Summarize the Monday standup",
      source: "Monday Standup",
      knowledge: [],
      closable: false,
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

test("a payload that is not a to-do list is refused", () => {
  assert.equal(modelFromPayload({ you: [], agents: [{ title: "Missing pieces" }] }), null);
  const root = connectedRoot();
  drawTodoScreen(root, { you: [], agents: [], more: true });
  assert.match(root.textContent, /All clear/);
  assert.match(root.textContent, /Nothing in progress/);
  assert.match(root.textContent, /Some to-dos are not listed yet/);
});
