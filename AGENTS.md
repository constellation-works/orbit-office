# office — agent guide

`office` is the Orbit exec backend for the orbit-office plugin, with a Rust workspace and a TypeScript web app. `CLAUDE.md` links to this guide.

## Rules

- Work only on authorized scope. Neither implementation nor a PR request authorizes merging.
- Update affected docs in the same PR as the code; stale docs block review.
- Edit an instruction file only when a rule, command or pointer in it changes; feature descriptions belong in feature docs.
- Don't touch `CHANGELOG.md` during tasks; it is compiled at release time.
- Report commands and outcomes at handoff — passed, failed, not run — never "tested".

## Branching

- `main` is the integration branch; commit directly.

## Gates

- Before review, run `make ci-fast`, `make ci-lint` and `make goldens`.
- After an intended change to help or output, run `make goldens UPDATE=1` and review the diff as a contract change.
- `make ci` runs in CI on Linux and macOS, including tests and doctests.

## Code

- Follow [ARCHITECTURE.md](ARCHITECTURE.md) for layers, feature layout, integrator-owned files, and how to build a planned tool or screen.
- Edit only your feature's folders; integrator-owned files change only in their own task.
- For design and decisions see [docs/design/office/](docs/design/office/); new features copy [docs/design/_templates/](docs/design/_templates/).
- Identity and secrets arrive in the call context, never in tool input.
- The exec envelope shapes are contracts: add tools and fields, never rename or retype them. Refuse unknown input keys.
- Secrets never appear in output, logs or errors; handlers report presence (`set`/`unset`) only.
- Use typed `thiserror` enums; the core's `Error` crosses into the surface `Error` through its one `#[from]`.
- Exit codes are 0 for success (including tool failures in error envelopes), 1 for failure and 2 for usage.
- Default to `pub(crate)`; declare workspace dependencies with `.workspace = true`.
- Follow the [Rust standard](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md) for lints, errors, stream handling and test layout, and the [concurrency standard](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md) for locks across `.await`.
- Never put internal task, friction or record IDs in help, examples or messages; use placeholders such as `<id>`.

## Tests

- No text-matching tests over source or copy.
- Follow the [testing standard](docs/standards/STD-04-testing-and-verification/STD-04.md) for regression tests and the [concurrency standard](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md) for host-state isolation; set `HOME` and `OFFICE_*` on child commands using [test support](crates/office/tests/support/mod.rs).

<!-- constellation-standards:begin -->
<!-- Managed by the constellation's operations/scripts/sync-standards.sh; edits inside this block are overwritten. -->
## Constellation standards

This repository adopts these constellation standards, vendored read-only in `docs/standards/` (each a directory: `STD-nn.md` holds the rules, `why.md` and `checks.md` the reasons and gates):

- `STD-01@2` — [docs/standards/STD-01-cli-surface/STD-01.md](docs/standards/STD-01-cli-surface/STD-01.md)
- `STD-02@4` — [docs/standards/STD-02-rust-architecture-and-errors/STD-02.md](docs/standards/STD-02-rust-architecture-and-errors/STD-02.md)
- `STD-03@2` — [docs/standards/STD-03-concurrency-and-process-safety/STD-03.md](docs/standards/STD-03-concurrency-and-process-safety/STD-03.md)
- `STD-04@2` — [docs/standards/STD-04-testing-and-verification/STD-04.md](docs/standards/STD-04-testing-and-verification/STD-04.md)
- `STD-05@1` — [docs/standards/STD-05-security-boundaries/STD-05.md](docs/standards/STD-05-security-boundaries/STD-05.md)

Follow them; they are normative. To deviate from a rule, record a decision in `docs/design/<feature>/4_decisions.md` citing `STD-nn@<version> §Rn`; never edit `docs/standards/` (`sh docs/standards/check.sh` enforces this).
Reviewers check every change against the adopted standards and report violations as `STD-nn §Rn` with file:line evidence.
<!-- constellation-standards:end -->
