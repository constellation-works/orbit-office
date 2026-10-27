/**
 * Register the app's service worker so the browser offers to install the
 * app as a PWA. The worker is registered as a module because the TypeScript
 * build emits it as one (`export {}` under the package's NodeNext
 * `type: "module"`). Webview-safe: browsers without service workers simply
 * skip registration and the app keeps working from the network.
 */
export function registerServiceWorker(): void {
  const scope = globalThis.navigator;
  if (typeof scope === "undefined" || !("serviceWorker" in scope)) return;
  scope.serviceWorker.register("/pwa/sw.js", { scope: "/", type: "module" }).catch(() => {
    // Registration failure only means the app cannot install as a PWA;
    // the app itself keeps working from the network.
  });
}
