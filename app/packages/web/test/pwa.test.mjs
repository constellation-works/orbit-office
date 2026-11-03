import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { cp, mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterEach, test } from "node:test";
import { fileURLToPath } from "node:url";

import { getSettings } from "../dist/api/client.js";
import { registerServiceWorker } from "../dist/pwa/register.js";

const originalFetch = globalThis.fetch;
const originalNavigator = Object.getOwnPropertyDescriptor(globalThis, "navigator");
const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
const originalCaches = Object.getOwnPropertyDescriptor(globalThis, "caches");
afterEach(() => {
  globalThis.fetch = originalFetch;
  if (originalNavigator !== undefined) {
    Object.defineProperty(globalThis, "navigator", originalNavigator);
  }
  for (const [key, descriptor] of [
    ["self", originalSelf],
    ["caches", originalCaches],
  ]) {
    if (descriptor !== undefined) Object.defineProperty(globalThis, key, descriptor);
    else delete globalThis[key];
  }
});

/**
 * Drive the compiled worker (`dist/pwa/sw.js`, the exact file the browser
 * installs) with stubbed worker globals. The module registers its
 * listeners once at import; every test then swaps in a fresh stub
 * environment (`useWorkerEnv`) because the handlers resolve `self`,
 * `caches` and `fetch` through globals at event time.
 */
const workerListeners = {};
let shellCache;
Object.defineProperty(globalThis, "self", {
  value: {
    location: { origin: "http://office.test" },
    skipWaiting: async () => {},
    clients: { claim: async () => {} },
    addEventListener: (type, listener) => {
      workerListeners[type] = listener;
    },
  },
  configurable: true,
});
await import("../dist/pwa/sw.js");
Object.defineProperty(globalThis, "caches", { configurable: true, value: {
  open: async (name) => { shellCache = name; return { addAll: async () => {} }; },
} });
let installation;
workerListeners.install({ waitUntil: (promise) => { installation = promise; } });
await installation;

/** Fresh stub environment; `seed` pre-fills the shell cache like an install. */
function useWorkerEnv({ seed = [], fetchImpl } = {}) {
  const addedAll = [];
  const stores = new Map([[shellCache, new Map(seed.map((url) => [url, `cached:${url}`]))]]);
  Object.defineProperty(globalThis, "self", {
    value: {
      location: { origin: "http://office.test" },
      skipWaiting: async () => {},
      clients: { claim: async () => {} },
      addEventListener: (type, listener) => {
        workerListeners[type] = listener;
      },
    },
    configurable: true,
  });
  Object.defineProperty(globalThis, "caches", {
    value: {
      open: async (name) => {
        if (!stores.has(name)) stores.set(name, new Map());
        const store = stores.get(name);
        return {
          addAll: async (urls) => {
            addedAll.push(...urls);
            for (const url of urls) {
              if (!store.has(url)) store.set(url, `cached:${url}`);
            }
          },
        };
      },
      keys: async () => [...stores.keys()],
      delete: async (name) => stores.delete(name),
      match: async (request) => {
        const raw = typeof request === "string" ? request : request.url;
          const path = new URL(raw, "http://office.test").pathname;
        for (const store of stores.values()) {
          if (store.has(path)) return new Response(store.get(path));
        }
        return undefined;
      },
    },
    configurable: true,
  });
  if (fetchImpl !== undefined) globalThis.fetch = fetchImpl;
  return { addedAll, stores };
}

function fetchEvent(url, { method = "GET", mode = "cors" } = {}) {
  let answered = null;
  return {
    event: {
      request: { method, url, mode },
      respondWith: (response) => {
        answered = response;
      },
    },
    answered: () => answered,
  };
}

const SHELL = [
  "/",
  "/style.css",
  "/fonts/geist-latin.woff2",
  "/fonts/geist-mono-latin.woff2",
  "/main.js",
  "/manifest.webmanifest",
  "/icons/icon-192.png",
  "/icons/icon-512.png",
  "/icons/icon-maskable-512.png",
  "/pwa/register.js",
  "/router.js",
  "/nav.js",
  "/api/client.js",
  "/api/source-location.js",
  "/ui/index.js",
  "/ui/card.js",
  "/ui/pill.js",
  "/ui/tile.js",
  "/ui/provenance.js",
  "/ui/action.js",
  "/ui/state.js",
  "/ui/attribution.js",
  "/ui/events.js",
  "/ui/handoff.js",
  "/ui/schedule.js",
  "/ui/when.js",
  "/ui/share.js",
  "/ui/panel.js",
  "/ui/markdown.js",
  "/screens/calendar/index.js",
  "/screens/calendar/editor.js",
  "/screens/calendar/model.js",
  "/screens/calendar/panel.js",
  "/screens/calendar/style.js",
  "/screens/calendar/task.js",
  "/screens/insights/index.js",
  "/screens/library/index.js",
  "/screens/library/style.js",
  "/screens/review/index.js",
  "/screens/routines/index.js",
  "/screens/routines/style.js",
  "/screens/scribe/contradictions.js",
  "/screens/scribe/dom.js",
  "/screens/scribe/feed.js",
  "/screens/scribe/index.js",
  "/screens/scribe/model.js",
  "/screens/scribe/panels.js",
  "/screens/scribe/style.js",
  "/screens/scribe/tags.js",
  "/screens/settings/index.js",
  "/screens/settings/calendar.js",
  "/screens/settings/editor.js",
  "/screens/settings/models.js",
  "/screens/settings/style.js",
  "/screens/settings/private.js",
  "/screens/settings/shared.js",
  "/screens/settings/signin.js",
  "/screens/source/index.js",
  "/screens/source/model.js",
  "/screens/source/style.js",
  "/screens/today/approvals.js",
  "/screens/today/digest.js",
  "/screens/today/index.js",
  "/screens/today/next-meeting.js",
  "/screens/today/rows.js",
  "/screens/todos/index.js",
  "/screens/todos/view.js",
];

const distDir = fileURLToPath(new URL("../dist/", import.meta.url));

/** Every JS module statically reachable from the compiled `/main.js`. */
function reachableModules(entry = "/main.js") {
  const seen = new Set();
  const queue = [entry];
  while (queue.length > 0) {
    const url = queue.shift();
    if (seen.has(url)) continue;
    seen.add(url);
    const source = readFileSync(path.join(distDir, `.${url}`), "utf8");
    for (const match of source.matchAll(/(?:import|export)\s+(?:[^'"]*?from\s+)?["'](\.[^"']+)["']/g)) {
      queue.push(new URL(match[1], `http://office.test${url}`).pathname);
    }
  }
  return [...seen].sort();
}

test("install precaches the app shell only", async () => {
  const { addedAll } = useWorkerEnv();
  const listeners = workerListeners;
  let waited = null;
  listeners.install({ waitUntil: (promise) => { waited = promise; } });
  await waited;
  assert.deepEqual([...addedAll].sort(), [...SHELL].sort());
  // Only the two compiled static modules below use the API prefix.
  for (const url of addedAll) {
    assert.notEqual(url, "/healthz");
    assert.ok(["/api/client.js", "/api/source-location.js"].includes(url) || !url.startsWith("/api/"), `${url} is live data and must not be precached`);
  }
});

test("two stamped builds install different shell caches", async (t) => {
  const root = await mkdtemp(path.join(tmpdir(), "office-build-cache-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const web = path.join(root, "packages/web");
  await mkdir(web, { recursive: true });
  await cp(distDir, path.join(web, "dist"), { recursive: true });
  const script = fileURLToPath(new URL("../../../install/app_install.py", import.meta.url));
  const names = [];
  for (const commit of ["a".repeat(40), "b".repeat(40)]) {
    execFileSync("python3", ["-B", script, "stamp", root, commit], {
      env: { PATH: process.env.PATH, HOME: root, TMPDIR: root }, timeout: 10000,
    });
    const listeners = {};
    Object.defineProperty(globalThis, "self", { configurable: true, value: {
      addEventListener: (name, listener) => { listeners[name] = listener; },
      skipWaiting: async () => {},
    } });
    Object.defineProperty(globalThis, "caches", { configurable: true, value: {
      open: async (name) => { names.push(name); return { addAll: async () => {} }; },
    } });
    const bytes = await readFile(path.join(web, "dist/pwa/sw.js"));
    await import(`data:text/javascript;base64,${bytes.toString("base64")}`);
    let installed;
    listeners.install({ waitUntil: (promise) => { installed = promise; } });
    await installed;
    const info = JSON.parse(await readFile(path.join(web, "dist/build-info.json"), "utf8"));
    assert.equal(info.commit, commit);
    assert.equal(names.at(-1), `office-shell-${info.build}`);
  }
  assert.notEqual(names[0], names[1]);
});

test("precache covers every JS module statically reachable from main.js", async () => {
  const { addedAll } = useWorkerEnv();
  let waited = null;
  workerListeners.install({ waitUntil: (promise) => { waited = promise; } });
  await waited;
  const modules = reachableModules();
  assert.ok(modules.length > 1, "the graph must reach beyond main.js");
  for (const url of modules) {
    assert.ok(addedAll.includes(url), `${url} reachable from /main.js must be precached`);
  }
  assert.ok(!addedAll.includes("/healthz"));
});

test("offline, every module reachable from main.js is answered from the precache", async () => {
  useWorkerEnv({
    seed: SHELL,
    fetchImpl: async () => { throw new Error("offline"); },
  });
  const listeners = workerListeners;
  for (const url of reachableModules()) {
    const { event, answered } = fetchEvent(`http://office.test${url}`);
    listeners.fetch(event);
    assert.ok(answered() !== null, `${url} must be answered from the precache offline`);
    assert.equal(await (await answered()).text(), `cached:${url}`);
  }
});

test("activate drops old caches and keeps the current shell", async () => {
  const { stores } = useWorkerEnv();
  const listeners = workerListeners;
  stores.set("office-shell-v0", new Map([["/", "stale"]]));
  let waited = null;
  listeners.activate({ waitUntil: (promise) => { waited = promise; } });
  await waited;
  assert.deepEqual([...stores.keys()], [shellCache]);
});

test("live-data requests are left to the network, never answered from cache", async () => {
  useWorkerEnv({ seed: SHELL });
  const listeners = workerListeners;
  let networked = 0;
  globalThis.fetch = async () => {
    networked += 1;
    return new Response("live");
  };
  for (const url of [
    "http://office.test/api/settings",
    "http://office.test/api/",
    "http://office.test/api",
    "http://office.test/healthz",
    "http://office.test/build-info.json",
  ]) {
    const { event, answered } = fetchEvent(url);
    listeners.fetch(event);
    assert.equal(answered(), null, `${url} must pass through to the network`);
  }
  assert.equal(networked, 0, "the worker itself must not fetch what it leaves alone");
});

test("shell requests are answered from the precache", async () => {
  useWorkerEnv({ seed: SHELL });
  const listeners = workerListeners;
  const { event, answered } = fetchEvent("http://office.test/main.js");
  listeners.fetch(event);
  assert.ok(answered() !== null);
  assert.equal(await (await answered()).text(), "cached:/main.js");
});

test("other same-origin requests go to the network and are not stored", async () => {
  const { stores } = useWorkerEnv({
    seed: SHELL,
    fetchImpl: async () => new Response("live"),
  });
  const listeners = workerListeners;
  const before = stores.get(shellCache).size;
  const { event, answered } = fetchEvent("http://office.test/demo.html");
  listeners.fetch(event);
  assert.equal(await (await answered()).text(), "live");
  assert.equal(stores.get(shellCache).size, before);
});

test("a failed navigation falls back to the cached shell", async () => {
  useWorkerEnv({
    seed: SHELL,
    fetchImpl: async () => { throw new Error("offline"); },
  });
  const listeners = workerListeners;
  const { event, answered } = fetchEvent("http://office.test/", { mode: "navigate" });
  listeners.fetch(event);
  assert.equal(await (await answered()).text(), "cached:/");
});

test("non-GET and cross-origin requests are left alone", async () => {
  useWorkerEnv({ seed: SHELL });
  const listeners = workerListeners;
  for (const [url, options] of [
    ["http://office.test/api/settings", { method: "POST" }],
    ["https://example.com/main.js", {}],
  ]) {
    const { event, answered } = fetchEvent(url, options);
    listeners.fetch(event);
    assert.equal(answered(), null, `${url} must pass through`);
  }
});

test("registration points the worker at the app scope", () => {
  let seen;
  Object.defineProperty(globalThis, "navigator", {
    value: {
      serviceWorker: {
        controller: null,
        addEventListener: () => {},
        register: async (url, options) => {
          seen = { url, options };
          return {};
        },
      },
    },
    configurable: true,
  });
  registerServiceWorker();
  assert.deepEqual(seen, { url: "/pwa/sw.js", options: { scope: "/", type: "module" } });
});

test("registration without worker support leaves the app alone", () => {
  Object.defineProperty(globalThis, "navigator", { value: {}, configurable: true });
  assert.doesNotThrow(registerServiceWorker);
});

test("settings requests return the agent CLI connection", async () => {
  globalThis.fetch = async (path, options) => {
    assert.equal(path, "/api/settings");
    assert.equal(options.headers.accept, "application/json");
    return new Response(JSON.stringify({ agentCli: { cli: "Muse Code", connected: true } }));
  };
  assert.deepEqual(await getSettings(), { agentCli: { cli: "Muse Code", connected: true } });
});
