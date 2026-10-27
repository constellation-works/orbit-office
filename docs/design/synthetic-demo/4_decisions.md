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
against a selected HTTPS provider. The task requires the loopback endpoint
configured as a hosted provider; a real-provider run remains the operator's
demo-time check. The seed manifest records that distinction.

## Subprocess containment in demo helpers

The Python helper reuses the gateway's bounded callback supervisor with
an 8 MiB cap per stream, a private stdin file and a new child process group.
The screenshot server also leads a process group, is signalled only while
its leader is live, and is reaped before the screenshot command returns.
On macOS Python without `waitid`, and in Node after its exit event, these
helpers adopt the repository's existing deviation from `STD-03@2 §R13`
(office decisions, "A leader exit with open pipes settles as a timeout"):
they never signal a group after its leader is reaped (`STD-03@2 §R14`).
They run only the named demo helpers and tool binaries in isolated roots;
the managed executor supplies outer descendant containment (`STD-03@2 §R21`).
They are foreground rehearsals, never persistent services.
