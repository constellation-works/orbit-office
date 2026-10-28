/**
 * The app's service worker: makes the app installable as a PWA by caching
 * the app shell only (docs/design/office/specs/web-app.md#delivery).
 *
 * The cache is written once, at install time, from `SHELL_ASSETS` below;
 * no fetch handler ever stores a response, so live-data `/api/*` endpoints
 * and `/healthz` always come live from the local server and are never
 * cached. Same-origin navigations fall back to the cached shell when the
 * network fails; there is no offline data mode. The static modules under
 * the `/api/` prefix (the compiled client and source-location modules) are
 * part of the shell (see `isApiRequest`), not live data.
 *
 * Self-contained on purpose: the shell list and the live-data check live
 * here (no imports to resolve at install time) and are covered by driving
 * this exact file with stubbed worker globals (`web/test/pwa.test.mjs`).
 * It is registered as a module worker (`pwa/register.ts`).
 *
 * The web project compiles without the WebWorker lib, so the few
 * worker-scope shapes below are declared locally instead of imported.
 */

/**
 * The app shell: the only URLs the worker ever stores. Beyond the shell
 * documents, this lists every JS module statically reachable from
 * `/main.js`, so an offline navigation fallback can actually boot:
 * `main.js` imports `pwa/register.js` and `router.js`, the router imports
 * every screen, and screens import `api/client.js` and `ui/*.js`.
 * `pwa.test.mjs` derives that graph from the compiled `main.js` and
 * fails when a reachable module is missing here.
 */
const SHELL_ASSETS: readonly string[] = [
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
  "/ui/schedule.js",
  "/ui/when.js",
  "/screens/calendar/index.js",
  "/screens/calendar/model.js",
  "/screens/calendar/panel.js",
  "/screens/calendar/style.js",
  "/screens/calendar/task.js",
  "/screens/insights/index.js",
  "/screens/library/index.js",
  "/screens/review/index.js",
  "/screens/routines/index.js",
  "/screens/routines/style.js",
  "/screens/settings/index.js",
  "/screens/settings/editor.js",
  "/screens/settings/style.js",
  "/screens/settings/private.js",
  "/screens/settings/signin.js",
  "/screens/source/index.js",
  "/screens/source/model.js",
  "/screens/source/style.js",
  "/screens/today/approvals.js",
  "/screens/today/index.js",
  "/screens/today/next-meeting.js",
  "/screens/todos/index.js",
  "/screens/todos/view.js",
];

/** Cache name for the shell. Bump when the shell's bytes change. */
const SHELL_CACHE = "office-shell-v8";

/**
 * True for live-data paths, which the worker must never cache. A `.js`
 * path under `/api/` is the compiled client module (`dist/api/client.js`,
 * served as a static file), not a live endpoint, so it stays cacheable.
 */
function isApiRequest(pathname: string): boolean {
  if (pathname === "/healthz") return true;
  if (pathname === "/api" || pathname.startsWith("/api/")) return !pathname.endsWith(".js");
  return false;
}

/** `ExtendableEvent` subset the worker uses. */
interface SWExtendableEvent extends Event {
  waitUntil(promise: Promise<unknown>): void;
}

/** `FetchEvent` subset the worker uses. */
interface SWFetchEvent extends SWExtendableEvent {
  readonly request: Request;
  respondWith(response: Response | Promise<Response>): void;
}

/** Worker global-scope subset the worker uses. */
interface SWGlobal {
  readonly location: { readonly origin: string };
  skipWaiting(): Promise<void>;
  readonly clients: { claim(): Promise<void> };
  addEventListener(type: "install" | "activate", listener: (event: SWExtendableEvent) => void): void;
  addEventListener(type: "fetch", listener: (event: SWFetchEvent) => void): void;
}

declare const self: SWGlobal;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(SHELL_CACHE)
      .then((cache) => cache.addAll([...SHELL_ASSETS]))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((key) => key !== SHELL_CACHE).map((key) => caches.delete(key))))
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (isApiRequest(url.pathname)) return;
  if (request.mode === "navigate") {
    event.respondWith(
      fetch(request).catch(() => caches.match("/").then((cached) => cached ?? Promise.reject(new Error("offline")))),
    );
    return;
  }
  event.respondWith(caches.match(request).then((cached) => cached ?? fetch(request)));
});
