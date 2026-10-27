/**
 * The app's service worker: makes the app installable as a PWA by caching
 * the app shell only (docs/design/office/specs/web-app.md#delivery).
 *
 * The cache is written once, at install time, from `SHELL_ASSETS` below;
 * no fetch handler ever stores a response, so `/api/*` and `/healthz`
 * always come live from the local server and are never cached. Same-origin
 * navigations fall back to the cached shell when the network fails; there
 * is no offline data mode.
 *
 * Self-contained on purpose: the shell list and the live-data check live
 * here (no imports to resolve at install time) and are covered by driving
 * this exact file with stubbed worker globals (`web/test/pwa.test.mjs`).
 * It is registered as a module worker (`pwa/register.ts`).
 *
 * The web project compiles without the WebWorker lib, so the few
 * worker-scope shapes below are declared locally instead of imported.
 */

/** The app shell: the only URLs the worker ever stores. */
const SHELL_ASSETS: readonly string[] = [
  "/",
  "/style.css",
  "/main.js",
  "/manifest.webmanifest",
  "/icons/icon-192.png",
  "/icons/icon-512.png",
  "/icons/icon-maskable-512.png",
];

/** Cache name for the shell. Bump when the shell's bytes change. */
const SHELL_CACHE = "office-shell-v1";

/** True for live-data paths, which the worker must never cache. */
function isApiRequest(pathname: string): boolean {
  return pathname === "/api" || pathname.startsWith("/api/") || pathname === "/healthz";
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
