# office — agent guide

Loaded as both `AGENTS.md` and `CLAUDE.md` (a symlink). `office` is the Orbit exec backend for the orbit-office plugin; it is a Rust workspace built from the constellation Rust CLI template, beside the plugin tree (`plugin/`) and the TypeScript web app (`app/`, npm workspaces).

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

## Rules

- Work only on authorized scope. Neither implementation nor a PR request authorizes merging.
- Don't add a crate, or a dependency between crates, without updating [`ARCHITECTURE.md`](ARCHITECTURE.md) and `scripts/check-dependency-direction.sh` in the same change.
- Update affected docs in the same PR as the code. Stale docs are a review blocker.
- Don't touch `CHANGELOG.md` during tasks; it is compiled at release time.
- Every test must exercise behavior. No text-matching tests (`include_str!` plus `contains()` over source or copy).
- A fix comes with a regression test through the real entry point (the built binary or the public API), seen failing before the fix.
- Tests never touch real host state: temp roots, and `HOME`/`OFFICE_*` set on the child command ([`tests/support`](crates/office/tests/support/mod.rs)).

## Branching

- `main` — the integration branch for this repo during the hackathon window; commit directly.

## Gates

`make ci-fast` (fmt, structure, `standards-check`), `make ci-lint` and `make goldens` must pass before a task moves to review. After an intended change to help or output, run `make goldens UPDATE=1` and review the diff as a contract change. `make ci` (everything, including tests and doctests) runs in CI on Linux and macOS.

## Code

- Layers: [`ARCHITECTURE.md`](ARCHITECTURE.md). Design and decisions: [`docs/design/office/`](docs/design/office/); new features copy [`docs/design/_templates/`](docs/design/_templates/).
- Layout for parallel work: one folder per feature in each layer, one file per tool (`crates/office-tools/src/tools/<family>/<tool>.rs`), one folder per app feature and screen. Edit only your feature's folders; the integrator-owned files listed in ARCHITECTURE.md (registry, routers, manifests, `Cargo.toml`, `scripts/`) change only in their own task. Building a planned tool or screen: ARCHITECTURE.md ("Tools", "Web app").
- `office-core` is the pure domain: no clap, no terminal, no environment reads, no I/O. `office-tools` holds dispatch and the handlers and reaches files, models and Orbit only through `office-core` ports; identity and secrets arrive in the call context, never in tool input. The CLI crate parses (`cli.rs`), composes (`app.rs`) and renders (`output/`).
- Lints come from `[workspace.lints]`: no `unwrap`/`expect` in production code, no `print!` (use `tracing`), no lock guard across `.await`, no wildcard arm on an enum, no narrowing `as`. CI runs clippy with `-D warnings`.
- Only `crates/office/src/output/` touches stdout, stderr, TTY state or color/format env vars. stdout is the one response envelope only.
- Errors: typed `thiserror` enums; the core's `Error` crosses into the surface `Error` through its one `#[from]`. Exit codes: 0 ok (tool failures are error envelopes), 1 failure, 2 usage.
- The exec envelope shapes (`schema_version`, tool names, accepted input keys, error codes) are contracts: add tools and fields, never rename or retype them. Unknown input keys are refused, never ignored.
- Secrets never appear in output, logs or errors: handlers report presence (`set`/`unset`) only.
- Default to `pub(crate)`; workspace dependencies via `.workspace = true`.
- Unit tests live in a sibling `tests/` directory mirroring source file names, declared in its `mod.rs`; crate-root `tests/` is integration only.
- Never put internal task, friction or record ids in help, examples or messages; use placeholders such as `<id>`.
- Report commands and outcomes at handoff — passed, failed, not run — never "tested".
