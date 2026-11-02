/**
 * Registers the service worker for PWA installation; browsers without support
 * continue loading the app over the network.
 */
export function registerServiceWorker(): void {
  const scope = globalThis.navigator;
  if (typeof scope === "undefined" || !("serviceWorker" in scope)) return;
  // The first install also claims the page. Only a replacement controller
  // means an already open window is now running an older shell.
  let controlled = scope.serviceWorker.controller !== null;
  scope.serviceWorker.addEventListener("controllerchange", () => {
    if (controlled) showUpdateNotice();
    controlled = true;
  });
  scope.serviceWorker.register("/pwa/sw.js", { scope: "/", type: "module" }).catch(() => {
    // Registration failure only means the app cannot install as a PWA;
    // the app itself keeps working from the network.
  });
}

function showUpdateNotice(): void {
  if (document.getElementById("office-update-notice") !== null) return;
  const notice = document.createElement("div");
  notice.id = "office-update-notice";
  notice.setAttribute("role", "status");
  notice.style.cssText = "position:fixed;bottom:20px;left:16px;right:16px;margin:auto;max-width:420px;z-index:1000;padding:16px;border:1px solid currentColor;border-radius:12px;background:var(--bg,#181818);color:var(--fg,#fff);display:flex;align-items:center;gap:16px;box-shadow:0 4px 20px #0006";
  const message = document.createElement("span");
  message.textContent = "A new version of Office is ready.";
  const reload = document.createElement("button");
  reload.type = "button";
  reload.textContent = "Reload";
  reload.addEventListener("click", () => globalThis.location.reload());
  notice.append(message, reload);
  document.body.append(notice);
}
