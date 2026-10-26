# orbit-office app

The local web app: a local server plus a single-page UI in plain words —
Today, Chat, To-dos, Routines, Library, Insights, Review. No ids, crews,
runs or raw statuses are ever shown.

**Status: scaffold.** The server is a placeholder that names the wiring to
build; the web entry lists the screens. Nothing here talks to Orbit yet.

## Wiring to build

- The server is the only process the browser talks to.
- It reads and writes Orbit through one long-lived `orbit mcp` stdio client,
  for both `orbit.*` and `office.*` tools — never by reading `.orbit/`
  directly, and never through Orbit's dashboard HTTP API.
- The orchestrator agent runs in the server: a Muse Spark tool-calling loop
  over `office.*` tools plus a curated subset of `orbit.*` tools.
- A dispatch pass every 30 seconds submits `backlog` tasks tagged
  `owner:agent` by id with `orbit.workflow.ship`. The office workspace never
  runs an auto drain, so `owner:you` to-dos are never submitted.
- Live status comes from polling Orbit every few seconds; there is no event bus.

## Develop

```bash
npm install
npm run typecheck
npm run dev
```
