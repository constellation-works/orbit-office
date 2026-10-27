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
| **Today** | Library setup card when missing; otherwise Next meeting (time and prep, hidden when the day's meetings are over), Needs you (proposals, with the onboarding set as one "Set up your Library" card; approvals, including routine script outputs and drafted API writes; bootstrap proposals grouped by subject; the user's to-dos), Agents working on (one line each, live), Done today (openable results), and a drop zone |
| **Source view** | The original (video or audio player, or image) beside the anchorable text; links to the tasks and pages that cite it. A video adds the chapter list with tangents folded and gotchas shown, and keyframe thumbnails on the timeline; video citation links open the player at their second ([kt-videos.md](./kt-videos.md#app)) |
| **Calendar** | Week view (day view on a phone) of the user's meetings, read only from Outlook or an ICS feed, with each meeting's prep and capture state in words and the routines' runs as margin marks; a meeting opens its attendees, agenda, cited prep brief and **Add notes or transcript** ([calendar.md](./calendar.md#app)) |
| **To-dos** | You and Agents columns; each to-do shows its source and attached knowledge |
| **Routines** | The next hour as one track (a mark per scheduled run, hollow where a paused routine skips its slot), then On and Paused tables: a switch, the routine and what it does, when it runs (plain words, with the cron line beneath), when it runs next (worked out from the schedule in the computer's time zone) and how its last time went (from `last_fire` in `orbit.auto_task.list`: when, and its task's state in words; task ids stay on the server). Retention settings below. Full run history and produced outputs await an Orbit listing change; scheduled background jobs are managed in Orbit. |
| **Library** | **Library setup** (the four bootstrap phases with progress and computed gaps, each gap with one action; [library-bootstrap.md](./library-bootstrap.md#setup-progress)); search; About pages grouped by template (You, People, Clients, Projects, Topics, Glossary and the rest) and How-to pages; one-per-Library pages not written yet shown as "Not set up yet"; Outputs by kind and series; a page view with citations, refs and back-links (a person's meeting sources and other Library documents; task links are deferred until a page-citation read exists); history for every page and artifact |
| **Settings** | Library settings; **Private route** (choose the endpoint kind, URL, model and label; the token goes straight to `orbit plugin secret set`; the check's capabilities in plain words; "Built with Muse Spark Open"; [models-and-privacy.md](./models-and-privacy.md#the-private-route)); **Connections** (add, edit or disable an API connection, and enter its secret, which goes straight to `orbit plugin secret set` and is never shown again); **API calls** (the gateway audit, newest first) ([gateway.md](./gateway.md)) |
| **Insights** | Automation ratio line, metric tiles, "what got automated" |
| **Review** | Generated reviews with their citations, and a regenerate button |

There is no Chat screen: conversation happens in the user's agent CLI.
Citations the agent prints are `http://localhost` links carrying the
source anchor; source links open the Source screen using `/?source=<id>#t=hh:mm:ss` (or
`#L`, `#frame=` and `#chapter=` anchors).

### Review implementation

Review searches the quarterly and yearly review series, reads each returned artifact body, and orders the returned reviews by approval time. Search is bounded to 100 hits per series and matches indexed text, so it is not a complete review listing. At most 100 reviews are shown, with a notice at the limit. Review citation links retain their source references; the Source screen accepts anchored localhost URLs. Regenerate requests the review auto-task and explains when that request is refused.

### Library setup

`GET /api/setup` reads `office.status` Library health. When the Library is missing, Today replaces Needs you with a Set up your Library card describing the scaffold. Set up shows progress and calls `POST /api/setup` once; the server calls `office.init` as the user with full output. Created and already initialized both lead to normal Today and refreshed rail counts. Failures show a reason and fix without automatic retries. If the plugin is disabled or the workspace unregistered, the card directs the user to `office init` instead of offering the action. Other screens link back to Today while setup is needed.

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
  recorded task description as provenance on every item. The Source screen also provides direct anchored navigation.

Empty sections have their own messages. A failed or malformed Orbit read
refuses the whole snapshot; the UI reports that failure and retains previous
data when available. A capped listing and completed work without a usable
completion date are explicitly reported. Limits may omit older unfinished
work or a result whose record was subsequently edited; this is a bounded
snapshot, not a complete history. The view uses the shared dark theme.
An empty `drop-zone` slot reserves the upload integration point; uploading
remains a separate feature. Each Needs-you card except the user's own to-dos
carries its `approval-actions` (below).

### Source implementation

Source is built. It accepts files, folder drops and pasted text; single uploads
call `office.capture` once and open the resulting source. Folder capture adds
`bootstrap:<phase>` to each supported file, skips unsupported extensions, and
then previews that phase's whole pending batch (including sources captured
earlier). A partial upload reports how many files were saved; it never retries
a capture automatically.

The app stages each upload in a random owner-only directory inside the configured
`ORBIT_OFFICE_WORKSPACE`, which is within the plugin's existing upload grant.
The filename must be a basename with a supported extension. The server streams
at most 256 MiB per file and removes its staging file and directory on success
or failure. JSON paste input is bounded to 1 MiB. Request reads have a 30-second
idle timeout. No browser-supplied filesystem path is accepted.

A video calls `office.video-plan` for a plan-only preview after capture. A folder
calls `office.batch-plan`. Neither starts model processing. The preview shows
minutes, frames, model calls and the recorded estimate, or “Rates unset”. Process
confirms once with `confirm: true` and the exact digest shown. Marking a preview
private fetches a new preview and disables the old confirmation. Privacy can
only be raised in this UI. Confirmation failures offer a fresh preview rather
than retrying a potentially completed mutation.

The original and text appear side by side on desktop and stack on phones.
`/?source=<id>#t=hh:mm:ss`, `#L<n>-<m>`, `#frame=hh:mm:ss` and `#chapter=<n>`
select the Source screen through the app entry point, highlight and scroll the
span, and seek audio/video after metadata loads. Ordinary navigation hashes
still select their named screens. Chapter tangents start folded, while their
gotchas remain visible. Keyframe links seek the same player. Citing tasks and
Library pages open their recorded text through the server; unavailable or capped
task citations are identified. Route badges come from `derived_by`; a recorded
private route uses the configured endpoint label. Mark private calls
`office.source-set-sensitivity` for subsequent processing.

Source JSON is served by `GET /api/sources?id=…`, originals and frames by
`GET /api/sources/media?id=…[&frame=0000.png]`, and citing documents by
`GET /api/sources/citation?id=…&kind=…`. Mutations use POST at
`/api/sources/upload`, `/paste`, `/private` and `/plan` (each below
`/api/sources`). Media stays behind `office.source-show` and its privacy checks;
byte ranges allow seeking, but the tool's existing **512 KiB** media cap still
applies. Larger, expired or refused originals show an unavailable message while
the source text remains readable. Source uses the shared dark theme.

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

## Look

One dark theme, taken from Orbit's dashboard and written fresh for the app:
Geist (bundled under the OFL in `web/static/fonts/`), a side rail that
stacks above the content on a phone, and panels drawn with a border on the
edge and hairlines inside. `web/static/style.css` defines the colour tokens
and the shared patterns. Screens style themselves with `var(--…)` tokens,
never literal colours:

- **Card**: an optional kicker (a short coloured label naming the kind of
  item), the title, the content, then actions.
- **Pill**: a short label or chip; its tone says needs you (amber), an agent
  is on it (cyan), finished (green), private (violet) or failed (red).
- **Tile**: one number and its label, in a wrapping row (Today's counts,
  Insights).
- **Section label**: small upper-case heading over a panel.
- **Rail**: screens in labelled groups (Work, Knowledge, Observe, Manage),
  each with an icon; a screen can nest under a parent (Library › Source,
  Insights › Review, Automation › Routines). Counts sit at the right: routines
  on of total, and Today's waiting cards as an amber pill. A failed read hides
  a count rather than showing 0. The layout lives in `web/src/nav.ts`.
- **Table rows** (Routines): a header row of column labels over hairline
  rows; on a phone each row stacks and every cell carries its label.

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
