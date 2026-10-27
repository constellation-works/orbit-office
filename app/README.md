# orbit-office app

The local web app (TypeScript), installable as a PWA so it opens in its
own window: a local server plus a single-page UI in plain words — Today,
To-dos, Routines, Library, Insights, Review. No ids, crews, runs or raw
statuses are ever shown. There is no Chat screen: conversation happens in
the user's agent CLI.

**Status: scaffold.** Every server feature and web screen exists as a
planned stub: planned features mount no routes, and planned screens are
left out of the navigation. `/healthz` lists the built features.

## Layout

npm workspaces, one TypeScript project per package, joined by project
references (`tsc -b`). No framework, no bundler. The import boundaries are
checked by `scripts/check-app-boundaries.py` (`make structure`); the full
layout is in `../ARCHITECTURE.md` ("Web app").

- `packages/contracts/` — types shared by server and web.
- `packages/server/` — `main.ts` (composition), `http/` (router, feature
  contract), `orbit/` (the `orbit tool run --json` client), `dispatch/`
  (the 30-second pass), `features/<f>/`.
- `packages/web/` — `static/index.html`, `src/router.ts`, `src/screen.ts`,
  `src/api/`, `src/ui/`, `src/screens/<s>/`.

## Wiring to build

- The server is the only process the browser talks to.
- It acts as the user: it calls Orbit with `orbit tool run --json` for
  both `orbit.*` and `office.*` tools, including the tools hidden from
  MCP that only the app may call. It never reads `.orbit/` directly, and
  never through Orbit's dashboard HTTP API.
- It owns the dispatch pass: every 30 seconds it lists `backlog` tasks
  tagged `owner:agent` and submits them by id with `orbit run ship <ids>`
  (review-only). The office workspace never runs an auto drain, so
  `owner:you` to-dos are never submitted.
- Live status comes from polling Orbit every few seconds; there is no event bus.

## Develop

```bash
npm install
npm run check   # tsc -b: every package, strict
npm test        # server HTTP behavior tests
npm run dev     # build, then tsx watch on the server entry
npm start       # run the built server
```
