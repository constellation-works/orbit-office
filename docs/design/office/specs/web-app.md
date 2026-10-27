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
  (`office.knowledge-write`, `office.proposal-accept`,
  `office.artifact-promote`, `office.selection-put`, `office.config-set`,
  `office.reindex`). Settings changed in the app, such as retention on the
  Routines screen, are written with `office.config-set` ([ORB-18711]).
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
| **Today** | Needs you (proposals, with the onboarding set as one "Set up your Library" card; approvals, including routine script outputs and drafted API writes; bootstrap proposals grouped by subject; the user's to-dos), Agents working on (one line each, live), Done today (openable results), and a drop zone |
| **Source view** | The original (video or audio player, or image) beside the anchorable text; links to the tasks and pages that cite it. A video adds the chapter list with tangents folded and gotchas shown, and keyframe thumbnails on the timeline; once the planned source screen ships, every video citation will open the player at its second ([kt-videos.md](./kt-videos.md#app)) |
| **To-dos** | You and Agents columns; each to-do shows its source and attached knowledge |
| **Routines** | Plain-sentence cards ("Every 15 minutes: process the KT videos you confirmed") with an on/off switch, last run, what it produced, and retention settings |
| **Library** | **Library setup** (the four bootstrap phases with progress and computed gaps, each gap with one action; [library-bootstrap.md](./library-bootstrap.md#setup-progress)); search; About pages grouped by template (You, People, Clients, Projects, Topics, Glossary and the rest) and How-to pages; one-per-Library pages not written yet shown as "Not set up yet"; Outputs by kind and series; a page view with citations, refs and back-links (a person's meetings and tasks); history for every page and artifact |
| **Settings** | Library settings; **Private route** (choose the endpoint kind, URL, model and label; the token goes straight to `orbit plugin secret set`; the check's capabilities in plain words; "Built with Muse Spark Open"; [models-and-privacy.md](./models-and-privacy.md#the-private-route)); **Connections** (add, edit or disable an API connection, and enter its secret, which goes straight to `orbit plugin secret set` and is never shown again); **API calls** (the gateway audit, newest first) ([gateway.md](./gateway.md)) |
| **Insights** | Automation ratio line, metric tiles, "what got automated" |
| **Review** | Generated reviews with their citations, and a regenerate button |

There is no Chat screen: conversation happens in the user's agent CLI.
Citations the agent prints are `http://localhost` links carrying the
source anchor; they will open the app's source or page view once the
planned source screen ships.

### Today implementation

Today and `GET /api/today` are built ([ORB-18727]). The endpoint polls `orbit.task.list`
through a shared Orbit client, with separate active and completed listings
(200 items each). Its shared contract is `Today` in `app/packages/contracts`.
The browser polls the endpoint five seconds after the preceding read settles.
Navigating away stops polling and prevents a late response replacing the new
screen. Open results, background panels and keyboard focus survive refreshes.

- Needs you selects `proposed` tasks tagged `proposal`, all `review` tasks,
  then unfinished `owner:you` tasks. This order also determines the label
  when a task matches more than one kind.
- Agents working on selects `in-progress` tasks tagged `owner:agent`,
  excluding `owner:you`, and shows one line per task.
- Done today selects `done` tasks whose latest transition to `done` falls
  between local midnight and the snapshot time. Last-edit timestamps do not
  establish completion. Each result opens its recorded execution summary;
  missing summaries are explicitly identified. “About this work” opens the
  recorded task description as provenance on every item. Source-specific
  navigation will arrive with the Source and Library screens.

Empty sections have their own messages. A failed or malformed Orbit read
refuses the whole snapshot; the UI reports that failure and retains previous
data when available. A capped listing and completed work without a usable
completion date are explicitly reported. Limits may omit older unfinished
work or a result whose record was subsequently edited; this is a bounded
snapshot, not a complete history. The view supports the shared light theme.
Empty `drop-zone` and per-item `approval-actions` slots reserve integration
points; uploading and approval mutations remain separate features.

### To-dos implementation

To-dos are built ([ORB-18728]). `GET /api/todos` lists You and Agents
columns; each to-do shows its source and attached knowledge, and the user
can close their own. An agent to-do has no close action.

## Rules

- Each item has one primary action (approve, edit or dismiss) and a provenance link on every agent output.
- The **selection tray** shows what the user has shared with the agent ("Shared with agent" pill, one-click clear) — [selection-sharing.md](./selection-sharing.md).
- Setup is one [install script](./install.md) that does everything automatically. No terminal is needed during use.

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

The installable shell (manifest and service worker) and the agent-CLI
settings read have landed ([ORB-18735]). Connections and the API calls
view are the rest of Settings ([ORB-18798]).

## Task References

- [ORB-18711] — builds `office.config-set` for Library settings.
- [ORB-18727] — built the Today screen and `GET /api/today`.
- [ORB-18728] — built the To-dos screen and `GET /api/todos`.
- [ORB-18735] — built the PWA shell and the settings feature's agent-CLI read.
- [ORB-18798] — builds the Settings connections and API calls views.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.
