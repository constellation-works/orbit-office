# office — agent guide

`office` is the Orbit exec backend for the orbit-office plugin, with a Rust workspace and a TypeScript web app. `CLAUDE.md` links to this guide.

## Rules

- Work only on authorized scope. Neither implementation nor a PR request authorizes merging.
- Update affected docs in the same PR as the code; stale docs block review.
- Edit an instruction file only when a rule, command or pointer in it changes; feature descriptions belong in feature docs.
- Don't touch `CHANGELOG.md` during tasks; it is compiled at release time.
- Handoff lists each validation command with its outcome, per [STD-04](docs/standards/STD-04-testing-and-verification/STD-04.md) §R16.

## Branching

- `main` is the integration branch; commit directly.

## Gates

- `make ci-fast`, `make ci-lint` and `make goldens` must pass before a task moves to review.
- After an intended change to help or output, run `make goldens UPDATE=1` and review the diff as a contract change.
- `make ci` runs in CI on Linux and macOS, including tests and doctests.

## Code

- Follow [ARCHITECTURE.md](ARCHITECTURE.md) for layers, feature layout, integrator-owned files, and how to build a planned tool or screen.
- Feature edits stay in their [task footprint](ARCHITECTURE.md#task-footprints); [integrator-owned files](ARCHITECTURE.md#integrator-owned-files) change only in their own task.
- For design and decisions see [docs/design/office/](docs/design/office/); new features copy [docs/design/_templates/](docs/design/_templates/).
- Identity and secrets arrive in the call context, never in tool input.
- Exec envelope shapes (`schema_version`, tool names, accepted input keys, error codes) are wire contracts ([STD-02](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) §R16): add tools and fields, never rename or retype them. Refuse unknown input keys.
- Secrets never appear in output, logs or errors: handlers report presence (`set`/`unset`) only.
- Errors follow [STD-02](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) §R10–R12; the core's `Error` crosses into the surface `Error` through its one `#[from]`.
- Exit codes follow [STD-01](docs/standards/STD-01-cli-surface/STD-01.md) §R20, except that tool failures in error envelopes exit 0.
- Enum matches and narrowing casts follow the [workspace lint policy](Cargo.toml#L68), including its scoped exception for foreign enums.
- Visibility and dependency declarations follow [STD-02](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) §R8–R9.
- Follow the [Rust standard](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) for lints, errors, stream handling and test layout, and the [concurrency standard](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md) for locks across `.await`.
- Internal task, friction and record IDs stay out of user-facing text ([STD-01](docs/standards/STD-01-cli-surface/STD-01.md) §R23).

## Tests

- Test layout and behaviour follow [STD-02](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) §R19–R21; never use text-matching tests over source or copy.
- Regression tests follow [STD-04](docs/standards/STD-04-testing-and-verification/STD-04.md) §R1–R2.
- Isolate tests from host state per [STD-03](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md) §R20.
- Set `HOME` and `OFFICE_*` on child commands using [test support](crates/office/tests/support/mod.rs).

<!-- constellation-standards:begin -->
<!-- Managed by the constellation's operations/scripts/sync-standards.sh; edits inside this block are overwritten. -->
## Constellation standards

This repository adopts these constellation standards, vendored read-only in `docs/standards/` (each a directory: `STD-nn.md` holds the rules, `why.md` and `checks.md` the reasons and gates):

- `STD-01@2` — [docs/standards/STD-01-cli-surface/STD-01.md](docs/standards/STD-01-cli-surface/STD-01.md): CLI surface — command grammar, inputs and effects, output modes, errors, help, and golden-tested public surfaces
- `STD-02@4` — [docs/standards/STD-02-rust-architecture-and-errors/STD-02.md](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md): Rust architecture and errors — layering, visibility, typed errors, failure semantics, idiom and naming, logging, and…
- `STD-03@2` — [docs/standards/STD-03-concurrency-and-process-safety/STD-03.md](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md): Concurrency and process safety — locks, channels, durable writes, state evolution, subprocess lifecycles, destructive…
- `STD-04@2` — [docs/standards/STD-04-testing-and-verification/STD-04.md](docs/standards/STD-04-testing-and-verification/STD-04.md): Testing and verification — test design, hermetic tests, honest gates, docs as contracts, and evidence-based…
- `STD-05@1` — [docs/standards/STD-05-security-boundaries/STD-05.md](docs/standards/STD-05-security-boundaries/STD-05.md): Security boundaries — authority, filesystem containment, child environments, redaction, local network surfaces, supply…
- `STD-11@2` — [docs/standards/STD-11-agent-skills/STD-11.md](docs/standards/STD-11-agent-skills/STD-11.md): Agent skills — a short router and core procedure in SKILL.md, with everything else in task-sized references it links by…

Follow them; they are normative. To deviate from a rule, record a decision in `docs/design/<feature>/4_decisions.md` citing `STD-nn@<version> §Rn`; never edit `docs/standards/` (`sh docs/standards/check.sh` enforces this).
Reviewers check every change against the adopted standards and report violations as `STD-nn §Rn` with file:line evidence.
<!-- constellation-standards:end -->
