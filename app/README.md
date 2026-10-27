# orbit-office app

The local web app (TypeScript), installable as a PWA so it opens in its
own window: a local server plus a single-page UI in plain words — Today,
To-dos, Routines, Library, Insights, Review. No ids, crews, runs or raw
statuses are ever shown. There is no Chat screen: conversation happens in
the user's agent CLI.

**Status: The Today and Settings server features are built, as is the app
server.** Today is the landing screen, backed by `GET /api/today`. Other
features and screens remain planned:
planned features mount no routes, and planned screens stay out of navigation.
`/healthz` lists the built features. The server itself — its Orbit client, static
serving with loopback Host/Origin guards, and the dispatch pass — is
built (below).

## Layout

npm workspaces, one TypeScript project per package, joined by project
references (`tsc -b`). No framework, no bundler. The import boundaries are
checked by `scripts/check-app-boundaries.py` (`make structure`); the full
layout is in `../ARCHITECTURE.md` ("Web app").

- `packages/contracts/` — types shared by server and web.
- `packages/server/` — `main.ts` (composition), `http/` (`router.ts`,
  `guard.ts`, `static.ts`, `types.ts`), `orbit/` (the `orbit tool run
  --json` client), `dispatch/` (the 30-second pass), `features/<f>/`.
- `packages/web/` — `static/index.html`, `src/router.ts`, `src/screen.ts`,
  `src/api/`, `src/ui/`, `src/screens/<s>/`.

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
  4) with a FIFO queue, a per-call timeout and typed errors; `pollTool`
  shares one in-flight poll (and briefly caches its result) so many
  browser tabs polling together spawn one subprocess.
- It owns the dispatch pass (`packages/server/src/dispatch/`): every 30
  seconds, single-flight, it lists `backlog` tasks tagged `owner:agent`
  and submits them by id with `orbit run ship <ids>` (review-only). The
  office workspace never runs an auto drain, so `owner:you` and
  `proposed` tasks are never submitted.
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

## Develop

```bash
npm install
npm run check   # tsc -b: every package, strict
npm test        # node:test suites for every package (fake `orbit` on PATH)
npm run dev     # build, then tsx watch on the server entry
npm start       # run the built server
```
