/**
 * Open contradictions between a meeting decision and the prior ledger
 * (`GET /api/scribe/contradictions`, `POST /api/scribe/contradictions/resolve`).
 *
 * The source is not installed yet: ORB-21340 adds meeting decisions to the
 * ledger and flags the contradictions. Until it lands the read answers
 * `available: false` with no contradictions, and a resolve is refused with
 * 501 `not_available`. Swap {@link readContradictions} and
 * {@link resolveContradiction} for the ORB-21340 tool calls; the routes and
 * the `Contradictions` contract stay as they are.
 */
import type { Contradictions } from "@orbit-office/contracts";

import { ScribeError } from "./model.js";

/** The open contradictions. ORB-21340 provides the source; until then none is available. */
export async function readContradictions(): Promise<Contradictions> {
  return { available: false, contradictions: [] };
}

/** Record which side stands. ORB-21340 provides the write; until then it is refused. */
export async function resolveContradiction(_body: unknown): Promise<{ ok: true }> {
  throw new ScribeError(501, "not_available");
}
