# Synthetic demo decisions

## Ephemeral inference fixture and Origin

The loopback model fixture is an explicit deviation from `STD-05@1 §R17`:
its inference POST accepts a missing Origin header. It models the hosted
provider API used by the native Office model adapter, which sends no Origin.
The POST performs no external action and changes only in-memory request
counters. The fixture rejects an Origin that does not match its exact
loopback Host, checks Host on every implemented route, limits the request
body, and is stopped and joined before the seed command returns. It is
never exposed beyond loopback. The CRM has no usable write route: PATCH
always refuses, and the seed checks that drafting sends zero PATCH requests.

## Native document and frame helpers

The demo uses macOS AppKit for deterministic frame rendering, since some
FFmpeg builds omit drawtext. If Poppler is absent, the demo supplies a
private argv-compatible pdftotext adapter backed by PDFKit for its generated
PDF. This performs actual document extraction; it is not evidence about
Poppler. Neither helper is globally installed or used by the application.

## Rehearsal and hosted evidence

The default private endpoint is an authored loopback inference fixture,
labelled simulated and third party. It proves tool integration and route
provenance, not real inference quality or hosted-provider availability.
The explicit provider-config option processes only synthetic KT material
against a selected HTTPS provider. A rehearsal never satisfies the live
hosted-provider criterion, and the seed manifest records that distinction.
