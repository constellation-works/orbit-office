# orbit-office app

The local web app (TypeScript), installable as a PWA so it opens in its
own window: a local server plus a single-page UI in
plain words — Today, To-dos, Routines, Library, Insights, Review. No ids,
crews, runs or raw statuses are ever shown. There is no Chat screen:
conversation happens in the user's agent CLI.

**Status: scaffold.** The server is a placeholder that names the wiring to
build; the web entry lists the screens. Nothing here talks to Orbit yet.

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
npm run check   # tsc --noEmit (strict)
npm run dev     # tsx watch on the server entry
npm run build   # emit dist/ for `npm start`
```
