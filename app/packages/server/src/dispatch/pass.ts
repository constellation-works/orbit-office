/**
 * The dispatch pass (planned): every 30 seconds, list `backlog` tasks
 * tagged `owner:agent` and submit them by id with `orbit run ship <ids>`
 * (review-only). Single-flight: a pass never starts while the last one is
 * running. The office workspace never runs an auto drain, so `owner:you`
 * to-dos are never submitted (docs/design/office/specs/web-app.md).
 */

/** How often the pass runs. */
export const DISPATCH_INTERVAL_MS = 30_000;
