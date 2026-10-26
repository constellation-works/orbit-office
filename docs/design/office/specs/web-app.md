---
type: design
summary: "Spec: web app — screens, rules, its authority as the user's console, and delivery as an installable PWA"
last_validated: 2026-10-26
---

# Spec: Web app

The web app is the user's console: everything except conversation happens
here. Plain words only — To-dos, Routines, Library, Needs you, Insights —
never ids, crews, runs, workflows or raw statuses.

Source: spec §13.1–§13.2 and §13.4.

## Authority

- The local app server is the only process the browser talks to.
- It acts **as the user**: it calls Orbit with `orbit tool run --json` for
  both `orbit.*` and `office.*` tools, including the tools hidden from MCP
  (`office.knowledge.write`, `office.proposal.accept`,
  `office.artifact.promote`, `office.selection.put`, `office.reindex`).
- It never reads `.orbit/` directly. Orbit's HTTP API is not used; it
  belongs to Orbit's own dashboard (spec §18.3).
- It owns the **dispatch pass**: every 30 seconds it lists `backlog` tasks
  tagged `owner:agent` and submits them by id with `orbit run ship <ids>`
  (review-only). It runs as a login service (launchd or a systemd user
  unit), so dispatch continues while the browser is closed.
- Live status comes from polling Orbit every few seconds. There is no event bus.

## Screens

| Screen | Content |
|---|---|
| **Today** | Needs you (proposals, approvals, the user's to-dos), Agents working on (one line each, live), Done today (openable results), and a drop zone |
| **Source view** | The original (audio player or image) beside the anchorable text; links to the tasks and pages that cite it |
| **To-dos** | You and Agents columns; each to-do shows its source and attached knowledge |
| **Routines** | Plain-sentence cards ("Every 15 minutes: process new meeting recordings") with an on/off switch, last run, what it produced, and retention settings |
| **Library** | Search; pages grouped by About and How-to; Outputs by kind and series; a page view with citations, refs and back-links; history for every page and artifact |
| **Insights** | Automation ratio line, metric tiles, "what got automated" |
| **Review** | Generated reviews with their citations, and a regenerate button |

There is no Chat screen: conversation happens in the user's agent CLI.
Citations the agent prints are `http://localhost` links that open the
app's source or page view.

## Rules

- Each item has one primary action (approve, edit or dismiss) and a provenance link on every agent output.
- The **selection tray** shows what the user has shared with the agent ("Shared with agent" pill, one-click clear) — [selection-sharing.md](./selection-sharing.md).
- Setup is one script: install Orbit, add and enable the plugin, create the workspace, register Orbit MCP with Muse Code, and install the app as a login service. No terminal is needed during use.

## Delivery

For the event the app ships as a local web app, not a desktop app
([decision](../4_decisions.md#a-local-web-app-installable-as-a-pwa-the-desktop-shell-waits)).

- **Installable PWA.** A web app manifest and a service worker make it
  installable: Chrome's or Edge's "Install app" (Safari: Add to Dock)
  opens it in its own window with a Dock or taskbar icon and no address
  bar, so the demo shows neither a terminal nor a browser. The service
  worker caches the app shell only; data always comes live from the local
  server, and there is no offline mode.
- **Webview-safe UI.** Relative URLs, and no browser-only API without a
  fallback, so a desktop shell can host the same UI later.
- **After the event: a Tauri shell** — one installer bundling the runtime
  (today the user needs Node 20+), a tray icon, native notifications for
  Needs you, and native drag-and-drop for recordings. Tauri does not
  bundle Node, so the server either ships as a sidecar or moves into the
  `office` binary.
