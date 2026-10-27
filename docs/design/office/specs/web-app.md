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
  unit), so dispatch continues while the browser is closed. A listing Orbit
  does not return in the expected shape fails the pass and submits nothing;
  it never reads as an empty queue.
- Live status comes from polling Orbit every few seconds. There is no event bus.

## Screens

| Screen | Content |
|---|---|
| **Today** | Needs you (proposals, with the onboarding set as one "Set up your Library" card; approvals, including routine script outputs and drafted API writes; bootstrap proposals grouped by subject; the user's to-dos), Agents working on (one line each, live), Done today (openable results), and a drop zone |
| **Source view** | The original (video or audio player, or image) beside the anchorable text; links to the tasks and pages that cite it. A video adds the chapter list with tangents folded and gotchas shown, and keyframe thumbnails on the timeline; once the planned source screen ships, every video citation will open the player at its second ([kt-videos.md](./kt-videos.md#app)) |
| **To-dos** | You and Agents columns; each to-do shows its source and attached knowledge |
| **Routines** | Plain-sentence cards ("Every 15 minutes: process the KT videos you confirmed") with an on/off switch, last run, what it produced, and retention settings |
| **Library** | **Library setup** (the four bootstrap phases with progress and computed gaps, each gap with one action; [library-bootstrap.md](./library-bootstrap.md#setup-progress)); search; About pages grouped by template (You, People, Clients, Projects, Topics, Glossary and the rest) and How-to pages; one-per-Library pages not written yet shown as "Not set up yet"; Outputs by kind and series; a page view with citations, refs and back-links (a person's meeting sources and other Library documents; task links are deferred until a page-citation read exists); history for every page and artifact |
| **Settings** | Library settings; **Private route** (choose the endpoint kind, URL, model and label; the token goes straight to `orbit plugin secret set`; the check's capabilities in plain words; "Built with Muse Spark Open"; [models-and-privacy.md](./models-and-privacy.md#the-private-route)); **Connections** (add, edit or disable an API connection, and enter its secret, which goes straight to `orbit plugin secret set` and is never shown again); **API calls** (the gateway audit, newest first) ([gateway.md](./gateway.md)) |
| **Insights** | Automation ratio line, metric tiles, "what got automated" |
| **Review** | Generated reviews with their citations, and a regenerate button |

There is no Chat screen: conversation happens in the user's agent CLI.
Citations the agent prints are `http://localhost` links carrying the
source anchor; they will open the app's source or page view once the
planned source screen ships.

### Review implementation

Review searches the quarterly and yearly review series, reads each returned artifact body, and orders the returned reviews by approval time. Search is bounded to 100 hits per series and matches indexed text, so it is not a complete review listing. At most 100 reviews are shown, with a notice at the limit. Citation links retain the source reference for the planned source or page view; those views do not yet resolve them. Regenerate requests the review auto-task and explains when that request is refused.

### Today implementation

Today and `GET /api/today` are built ([ORB-18727]). The endpoint polls `orbit.task.list`
through a shared Orbit client, with separate active and completed listings
(200 items each). Its shared contract is `Today` in `app/packages/contracts`.
The browser polls the endpoint five seconds after the preceding read settles.
Navigating away stops polling and prevents a late response replacing the new
screen. Open results, background panels and keyboard focus survive refreshes.

- Needs you selects `proposed` tasks tagged `gateway-draft`, then
  `script-output`, then `proposal`; all `review` tasks; then unfinished
  `owner:you` tasks. This order also determines the label when a task
  matches more than one kind. Proposals tagged `onboarding` share one
  "Set up your Library" card; proposals tagged `bootstrap:<phase>` share a
  card per `subject:<name>` tag, or per phase without one.
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
An empty `drop-zone` slot reserves the upload integration point; uploading
remains a separate feature. Each Needs-you card except the user's own to-dos
carries its `approval-actions` (below).

### Approvals implementation

Approvals are built ([ORB-18733]). The server feature calls Orbit as the
user; the Today screen shows the controls and says every outcome in plain
words (tool codes are never shown). `GET /api/approvals/detail?key=` returns
what a card shows before the user acts: a proposal's text and the SHA-256 of
its exact `proposal.json`, the output file names of reviewed work or script
output, or a gateway draft's connection, method, URL, body and digest.

- **Approve** reviewed work or script output calls `office.artifact-promote`,
  then walks the task to `done` with a comment naming who approved. There is
  no `verdict` object or `complete` flag: the installed app holds no operator
  authority. A refused promote changes nothing, so the task stays waiting
  for review and the card says why.
- **Send back** moves reviewed work to `backlog` with the user's comment.
- **Accept** calls `office.proposal-accept` once with the digest shown.
  **Accept all** on a grouped card accepts each page in turn; a refused page
  leaves the others accepted, and the card names it.
- **Dismiss** rejects a proposal, script output or gateway draft, with an
  optional reason.
- **Approve and send** on a gateway draft calls `gateway.http-send` with the
  draft id and the digest shown ([gateway.md](./gateway.md)). A lost reply
  is shown as `outcome_unknown`, never as a failure, and is never retried.
  Dismiss rejects the task and nothing is sent. No other approval sends
  anything: a Library `draft` for someone else is copied, never sent
  ([orchestrator.md](./orchestrator.md#guardrails)).

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
view are built ([ORB-18798]).

### Private route

The Private route section is built ([ORB-18841]). It asks for the endpoint
kind (your company's server, a hosted provider, or your own hardware), the
URL, the model and the label, and shows where that material goes. The section
always says "Private means not sent to Meta's hosted API, sent only to the
endpoint you chose". Saving writes the whole `open_weights` block with
`office.config-set`, then runs `office.private-route-check` once. A token, when
one was typed, goes only to `orbit plugin secret set office open_weights_token`
on stdin and is never shown again. The check's capabilities are shown in plain
words (a refused audio probe says that private videos can't be transcribed).
`GET /api/settings/private-route` reads the block from `office.status`
(`open_weights`, or null). The footer and the section show "Built with Muse
Spark Open" only when a block is configured.

## Task References

- [ORB-18711] — builds `office.config-set` for Library settings.
- [ORB-18727] — built the Today screen and `GET /api/today`.
- [ORB-18728] — built the To-dos screen and `GET /api/todos`.
- [ORB-18733] — built the Needs-you approval actions and `/api/approvals/*`.
- [ORB-18735] — built the PWA shell and the settings feature's agent-CLI read.
- [ORB-18798] — builds the Settings connections and API calls views.
- [ORB-18841] — builds the Settings private route and the Muse Spark Open attribution.

> Resolve any task above with `orbit task show <ID>` or `git log --grep=<ID>`.

The Library browses the newest 100 matching outputs from `office.artifact-list`; when more match, it says “Showing the latest 100”. Kind and exact series filters narrow the listing, so older outputs and series may not appear in an unfiltered view.
