# orbit-office app

The local web app (TypeScript), installable as a PWA so it opens in its
own window: a local server plus a single-page UI in plain words — Today,
Calendar, To-dos, Routines, Library, Insights, Review. No ids, crews, runs or raw
statuses are ever shown. There is no Chat screen: conversation happens in
the user's agent CLI.

**Status: Today, Calendar, To-dos, Routines, Library, Insights, Review, Source and Settings are built, as is the app server.** Today is the landing screen, backed by `GET /api/today`. To-dos shows your open to-dos and the agents' in two
columns. Each one names its source and its attached knowledge, in plain words.
Closing one of yours marks it done through Orbit; an agent's to-do has no close
action. Planned server features mount no routes; every listed screen is now built.
`/healthz` lists the built features. The server itself — its Orbit client, static
serving with loopback Host/Origin guards, and the dispatch pass — is
built (below).

Settings has a built screen with Connections, Private route and API calls
sections, and it is in the main navigation. The screen's browser suite mounts
the production renderer directly for validation.

Connections lists API access and secret presence, with add, edit, disable and
enable controls. The server calls `gateway.connection-list` and
`gateway.connection-put`; the secret goes only to
`orbit plugin secret set gateway <slot>` on stdin. Its child output is discarded,
and HTTP errors contain fixed codes rather than child diagnostics. Password
inputs start empty and clear before a save awaits its response. A failed secret
write reports that the connection itself was saved. An origin change removes
the old slot value with `orbit plugin secret rm gateway <slot>`.

The editor offers every gateway auth scheme. For a secret address
(`secret_path`) it takes the feed's full address and saves its origin as the
base URL and its path and query as the secret. A sign-in-with-a-code
connection (`oauth2_device`) has no secret field. Its card offers "Sign in"
(or "Sign in again" once the gateway reports it `expired`), which shows the
user code and the verification address and polls until the connection is
connected, declined or the code expires. The server calls
`gateway.oauth-device-start` and `gateway.oauth-device-poll` and passes on
only the code, the address and the state. The refresh token reaches the slot
inside the gateway call and never crosses HTTP.

API calls reads `gateway.audit-list`, showing up to 100 recent requests newest
first, with time, connection, method, path, HTTP status, size, duration and caller
labels. It omits response bodies and internal caller identifiers. Both sections
show failures and empty states separately, and use the shared dark theme
at desktop and phone widths. The Settings endpoints are
`GET /api/settings/connections`, `POST /api/settings/connections/put`,
`POST /api/settings/connections/sign-in`, `POST /api/settings/connections/sign-in/poll`,
`GET /api/settings/private-route`, `POST /api/settings/private-route`,
`GET /api/settings/shared-library`, `POST /api/settings/shared-library`,
`POST /api/settings/shared-library/routine`, `POST /api/settings/shared-library/sync`,
`GET /api/settings/shared-library/last-sync`, `POST /api/settings/shared-library/dismiss-skipped` and
`GET /api/settings/api-calls`; `GET /api/settings` still reports the agent CLI.
Shared library saves the `shared` block with `office.config-set`, naming a
refused field but never its value, offers to turn on the hourly
`office-shared-sync` routine, and runs **Sync now** as one `office.library-sync`
(counts in documents, failures in plain words). The latest sync the app ran is
kept in server memory only; `?settings=shared-library#settings` opens the
section (docs/design/office/specs/web-app.md, "Shared library implementation").
Private route saves the `open_weights` block with `office.config-set`, then runs
`office.private-route-check` once. Its token goes only to
`orbit plugin secret set office open_weights_token` on stdin. The footer and the
section show "Built with Muse Spark Open" only when an endpoint is configured.
Connection reads and full writes carry the optional `idempotency_header` gateway
field; the Settings editor and enable/disable controls preserve existing values.
An HTTP client can change it by supplying a new header name or remove it by
omitting the field from a full write.

## Layout

npm workspaces, one TypeScript project per package, joined by project
references (`tsc -b`). No framework, no bundler. The import boundaries are
checked by `scripts/check-app-boundaries.py` (`make structure`); the full
layout is in `../ARCHITECTURE.md` ("Web app").

- `packages/contracts/` — types shared by server and web.
- `packages/server/` — `main.ts` (composition), `http/` (`router.ts`,
  `guard.ts`, `static.ts`, `types.ts`), `orbit/` (the `orbit tool run
  --json` client and the shipped routine names), `dispatch/` (the 30-second
  pass), `features/<f>/`.
- `packages/web/` — `static/index.html`, `src/router.ts`, `src/nav.ts` (the rail), `src/screen.ts`,
  `src/api/`, `src/ui/` (including `schedule.ts`, the schedule reader Routines
  and Calendar share), `src/screens/<s>/`.
- `tests/` — the root suite (`http-router.test.ts`, run by `npm test`). Its
  `tsconfig.json` is a project reference of the root, so `npm run check`
  type-checks it too.

## Wiring

- The server is the only process the browser talks to. It binds to
  `127.0.0.1` by default (`HOST` overrides the bind address) and refuses
  non-loopback `Host` headers and cross-site `Origin`s on every route
  (STD-05 loopback HTTP); `/` serves the UI from
  `packages/web/static` and `packages/web/dist`.
- It acts as the user: `packages/server/src/orbit/` calls Orbit with
  `orbit tool run <name> --json` for both `orbit.*` and `office.*`
  tools, including the tools hidden from MCP that only the app may call.
  It never reads `.orbit/` directly, and never through Orbit's dashboard
  HTTP API. Concurrent subprocesses are capped (configurable, default
  4). Further calls wait FIFO, at most 32 by default (configurable). A
  call past that limit is refused and is not queued. A queued call is
  cancelled if it is still waiting when its queue deadline expires
  (by default the same length as the execution timeout). Each running
  call has its own execution timeout. Failures are typed errors. `pollTool`
  shares one in-flight poll (and briefly caches its result) so many
  browser tabs polling together spawn one subprocess.
- It owns the dispatch pass (`packages/server/src/dispatch/`): every 30
  seconds, single-flight, it lists `backlog` tasks tagged `owner:agent`
  and submits them by id with `orbit run ship <ids>` (review-only). The
  office workspace never runs an auto drain, so `owner:you` and
  `proposed` tasks are never submitted. A malformed listing fails the pass
  and submits nothing; it never reads as an empty queue.
- Live status comes from polling Orbit every few seconds; there is no event bus.

The shared UI pieces and responsive styles can be viewed at `/demo.html`
when the app server is running. Screens can import the pieces from `web/src/ui/`
and use `web/src/api/` for server requests.

Today shows suggestions awaiting a decision, work ready for review, your
unfinished to-dos, active agent work, and results finished in the server's
local calendar day. Open a result to read its recorded outcome; “About this
work” opens its recorded background. It refreshes every five seconds without
overlapping requests, retaining open results and showing refresh failures.
Lists are capped at 200 active and 200 completed items, with a visible notice
when limited. A missing completion date is reported rather than inferred from
the last edit. Uploads and approval controls have reserved slots for their
own features; Today currently reads work only.

The Today browser behavior suite uses the host's verified browser kit:

```bash
. "$ORBIT_BROWSER_KIT_ENV"
node --test app/packages/web/test/today.browser.mjs  # from the repository root, after the app build
```

Set `TODAY_SCREENSHOTS_DIR` to a directory under `.orbit/tmp/` to also capture
the populated and empty Today views at desktop and 375px widths. This suite
uses an in-memory Orbit client and starts and closes its own loopback server.

The Settings browser suite uses a fake Orbit executable and starts and closes
its own fixture server and browser, including add/edit/disable behavior:

```bash
. "$ORBIT_BROWSER_KIT_ENV"
SETTINGS_SCREENSHOTS_DIR="$PWD/.orbit/tmp/screenshots" node --test app/packages/web/test/settings.browser.mjs
```

Run this from the repository root after the app build. It captures Connections,
the connection editor, API calls, the private route (empty and configured), and
an expired sign-in, the device-code view, its editor and a declined sign-in, at
1440px and 375px in the dark theme, checking each for horizontal overflow. The app has no light theme. A before
shot of the private route was not captured: the base revision has no such section.

## Develop

```bash
npm install
npm run check   # tsc -b: every package and the root tests, strict
npm test        # node:test suites for every package (fake `orbit` on PATH)
npm run dev     # build, then tsx watch on the server entry
npm start       # run the built server
```

Library searches and browses pages, outputs and setup progress through Office read tools. Outputs show the latest 100 matches, with kind and exact series filters and a notice when more exist. Person pages show Library document back-links; task links await a supported citation read. Page and artifact history comes from the show tools.

Shared documents carry "Shared · from <member>" and the user's own shareable
ones "Shared". Page, output and source views of the user's own documents have
a **Share with the team** switch backed by `POST /api/library/share`
(`office.share-set`); it confirms before stopping and offers **Share these
too** for unshared cited documents. Library setup shows **Connect a shared
library** first while none is configured. Needs you groups the shared
library's updates, conflicts (**Use theirs**, **Keep mine**) and the last
sync's skipped documents into one card. The cross-screen browser suite answers
every Orbit call in memory and captures Settings › Shared library, a shared
page with the switch and the Needs-you card at 1440px and 375px (the app has
one dark theme):

```bash
. "$ORBIT_BROWSER_KIT_ENV"
SHARED_LIBRARY_SCREENSHOTS_DIR="$PWD/.orbit/tmp/screenshots" node --test app/packages/web/test/shared-library.browser.mjs
```


## Source capture and citation links

Source accepts file and folder drops, file pickers and pasted text. A video or
folder opens a processing preview with counts and cost before Process confirms
the shown digest. A preview can be raised to private; that obtains a fresh plan.
The source view shows original media, anchorable text, chapters, keyframes,
citing work and route badges. Use `/?source=<id>#t=hh:mm:ss`, `#L<n>-<m>`,
`#frame=hh:mm:ss` or `#chapter=<n>` for citations.

Uploads need the installed `ORBIT_OFFICE_WORKSPACE` binding. They are staged
inside that workspace with owner-only permissions, bounded to 256 MiB per file,
and cleaned after capture. Media reads use Office tools exclusively; their
512 KiB original/frame cap applies, and the screen explains unavailable media.
The view uses the shared dark theme and stacks at phone widths.

After building, run the Source browser suite from the repository root with the
verified browser kit:

```sh
. "$ORBIT_BROWSER_KIT_ENV"
SOURCE_SCREENSHOTS_DIR="$PWD/.orbit/tmp/screenshots" node --test app/packages/web/test/source.browser.mjs
```

This fixture suite exercises the production screen and HTTP feature without
calling a live Library or Orbit, and captures desktop and 375px views.

## Calendar

Calendar sits under Work between Today and To-dos. It shows the week from
Monday at desktop widths and one day at a time on a phone (a day picker starts
on today), with the hours 08:00–18:00 widened to the week's meetings, an
all-day strip and a current-time line. Each meeting block says its state in
words ("Prep ready", "Prep at HH:MM", "Add notes", "Summary saved",
"Cancelled" struck through, "Private"); routine runs are marks in each day's
margin, hollow when paused, and open Routines. A meeting opens a panel with
when and where, attendees linked to their person pages, the agenda as quoted
text, the cited prep brief and, after the meeting, the notes, summary and
to-dos and **Add notes or transcript** (paste, or pick or drop a `.vtt`,
`.txt`, `.md` or audio file). A calendar that never synced shows "Connect your
calendar", a failed sync "Last synced HH:MM, the latest sync failed" over the
last snapshot, and an empty week says so. The rail counts today's past meetings
without notes, and Today shows a Next meeting card while a meeting is left
today.

The server reads `office.calendar-list`, `orbit.auto_task.list` and the routine
listing for `GET /api/calendar?from&until`, calls `office.calendar-sync` for
`POST /api/calendar/sync`, stages a paste or upload and calls `office.capture`
with `event_key` for `POST /api/calendar/capture`, and reads a captured
meeting's summary and to-dos for `GET /api/calendar/meeting?source=<id>`. The
spec is `docs/design/office/specs/web-app.md` ("Calendar implementation").

After building, run the Calendar browser suite from the repository root with
the verified browser kit:

```sh
. "$ORBIT_BROWSER_KIT_ENV"
CALENDAR_SCREENSHOTS_DIR="$PWD/.orbit/tmp/screenshots" node --test app/packages/web/test/calendar.browser.mjs
```

It runs the production screen, calendar and source features on a fixture Orbit
client (`packages/server/test/calendar-fixture.mjs`, a synthetic week in UTC
with the clock fixed), covering the week, the day view, the meeting panel,
capture, the three states and Today's card at 1440px and 375px with no
horizontal overflow.
