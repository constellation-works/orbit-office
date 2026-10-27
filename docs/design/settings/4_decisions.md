# Settings implementation decisions

## Follow the gateway's shipped wire contract

The Settings HTTP feature uses `gateway.connection-list`,
`gateway.connection-put` and `gateway.audit-list`. These are the hyphenated
names required by the current gateway manifest and
[gateway spec](../office/specs/gateway.md), superseding the dotted names in the
original task brief. Disable sends the complete entered connection with
`disabled: true`. Secrets are excluded from that tool input. Only fixed
`conn_01` through `conn_16` slots from the put result can receive stdin.

The existing agent CLI status endpoint remains compatible. Connection and audit
HTTP outputs project known fields rather than forwarding raw gateway responses.
The app accepts at most one connection write at a time and refuses overlapping
writes with `settings_busy`, so a put and its subsequent secret operations do
not interleave between tabs in this server. Each request body is capped at
64 KiB and ten seconds. Reads remain independent.

Connection metadata and secret storage are separate Orbit operations. A secret
failure cannot roll back a successful connection put; the HTTP error explicitly
reports `connection_saved: true`. No write is automatically retried. An origin
change removes the released slot value before setting a replacement. Failed
removal may leave an unused slot occupied; Orbit refuses to allocate such a
slot to another connection.

## Direct Orbit secret subprocess and its lifecycle limit

For `STD-05@1 §R12`, the value goes only on stdin, without an added newline.
Both child output streams go to the null device: even a child that echoes the
value cannot put it in a server log or an HTTP response. Slot writes use the
trusted installed Orbit CLI with the user's environment, as the existing Orbit
tool client does; no shell or plugin program is invoked directly.

Deviation from `STD-03@2 §R12` and `STD-03@2 §R13`: the Node child-process API
reaps the leader before notifying JavaScript of exit. This feature cancels
timers on `exit` and waits for `close`, rather than signalling a process group
after its leader has been reaped (`STD-03@2 §R14`). It creates a process group,
sends SIGTERM after ten seconds, and escalates to SIGKILL after 250 ms while
the child remains live. It cannot prove that a deliberately orphaned descendant
has exited once the leader closes; this narrow helper is for the trusted
`orbit plugin secret set/rm` commands, not arbitrary workloads. A native shared
supervisor would be needed to remove that limitation.

## Navigation remains integrator-owned

The repository guide and `ARCHITECTURE.md` forbid feature tasks from changing
`web/src/router.ts`. The screen therefore exports the normal built `Screen`
contract and adds its ID to contracts, but registration is owed by an integrator
task. Browser validation mounts that same renderer in a fixture shell. This
does not evidence reachability from the production navigation. Library settings
are outside this Connections and API calls task.
