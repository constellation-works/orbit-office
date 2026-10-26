#!/usr/bin/env bash
# scripts/check-dependency-direction.sh — STD-02 R1–R6, R16, R17 for this workspace.
#
# Two layers of checks, both from source files alone (no build, no network,
# no jq), so the gate runs in `make ci-fast`:
#
#   1. Crate edges. Every workspace crate needs a policy below. A crate may
#      depend only on the internal crates in its allowlist, and never on the
#      external crates in its banlist. Dev-dependencies are exempt. A crate
#      without a policy fails the check, so a new member forces a decision.
#   2. Module edges inside a crate, as grep bans over non-test sources.
#
# The layer order is written down in ARCHITECTURE.md; change it and this
# script in the same commit (STD-02 R6).
set -euo pipefail
cd "$(dirname "$0")/.."

PREFIX="office"
fail=0
err() { echo "dependency-direction: $*" >&2; fail=1; }

# --- 1. crate policies -------------------------------------------------------
# policy <crate>  -> sets ALLOWED (internal deps) and BANNED (external deps)
policy() {
  case "$1" in
    office-common)
      ALLOWED=""
      # The foundation never parses arguments, draws on a terminal or
      # installs a log subscriber: those belong to surfaces.
      BANNED="clap tracing-subscriber anstream anstyle termcolor terminal_size crossterm owo-colors colored"
      ;;
    office-core)
      ALLOWED="office-common"
      # The domain never parses arguments, draws on a terminal or installs a
      # log subscriber: those belong to surfaces.
      BANNED="clap tracing-subscriber anstream anstyle termcolor terminal_size crossterm owo-colors colored"
      ;;
    office-store)
      ALLOWED="office-common"
      # Persistence never parses arguments, draws on a terminal or installs
      # a log subscriber: those belong to surfaces.
      BANNED="clap tracing-subscriber anstream anstyle termcolor terminal_size crossterm owo-colors colored"
      ;;
    office)
      ALLOWED="office-common office-core office-store"
      BANNED=""
      ;;
    *) return 1 ;;
  esac
}

# Print the dependency names declared in the non-dev dependency tables of a
# Cargo.toml: [dependencies], [build-dependencies] and target-specific ones.
declared_deps() {
  awk '
    /^\[/ {
      section = $0
      in_deps = (section ~ /^\[(target\..*\.)?(build-)?dependencies\]$/)
      next
    }
    in_deps && /^[A-Za-z0-9_-]+[[:space:]]*(\.workspace)?[[:space:]]*=/ {
      name = $1; sub(/\.workspace$/, "", name); sub(/=.*/, "", name)
      gsub(/[[:space:]]/, "", name); print name
    }
  ' "$1"
}

contains() { # contains <word> <space-separated list>
  local word="$1" item
  for item in $2; do [[ "$item" == "$word" ]] && return 0; done
  return 1
}

for manifest in crates/*/Cargo.toml; do
  crate=$(awk -F'"' '/^name[[:space:]]*=/ { print $2; exit }' "$manifest")
  if ! policy "$crate"; then
    err "crate '$crate' ($manifest) has no policy; add one here and a row in ARCHITECTURE.md"
    continue
  fi
  while IFS= read -r dep; do
    if [[ "$dep" == "$PREFIX" || "$dep" == "$PREFIX"-* ]]; then
      contains "$dep" "$ALLOWED" || err "$crate must not depend on internal crate $dep"
    elif contains "$dep" "$BANNED"; then
      err "$crate must not depend on $dep (surface crate in the domain)"
    fi
  done < <(declared_deps "$manifest")
done

# --- 2. module bans ----------------------------------------------------------
# ban <path> <extended-regex> <message>: fail if the pattern appears in
# non-test, non-comment lines of the Rust sources under <path> (a file or a
# directory).
ban() {
  local hits
  hits=$(grep -rnHE --include='*.rs' --exclude-dir=tests "$2" "$1" 2>/dev/null \
    | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)
  if [[ -n "$hits" ]]; then
    echo "$hits" >&2
    err "$3"
  fi
}

CORE=crates/office-core/src
STORE=crates/office-store/src
CLI=crates/office/src

# office-store contracts are I/O-free record shapes (R5); the drivers own
# all filesystem and SQLite access.
ban "$STORE/contracts.rs" 'std::(fs|net|process)' "office-store contracts are record shapes: I/O belongs in driver/"

# Core is the protocol domain (R5): every core module is I/O-free today.
# When a Library persistence module lands, it becomes the one exempt module
# here, with its own bans, in the same commit.
for path in "$CORE"/*; do
  name=$(basename "$path")
  case "$name" in tests) continue ;; esac
  ban "$path" 'std::(fs|net|process)|tempfile::|rustix::' "core/$name is protocol logic: file and process access belong in a future persistence module"
done
# The domain receives resolved inputs in the call context (R3).
ban "$CORE"          'std::env::|home_dir|current_dir'           "the core crate must not read the environment or discover paths"
# Composition does not know about parsing or rendering (R2).
ban "$CLI/app.rs"    'crate::(cli|output)'                       "app.rs (composition) must not import surface modules"
# Rendering is derived from payloads only; it never reaches back up (R1).
ban "$CLI/output"    'crate::(app|cli)'                          "output must not import app or cli"

# Persisted shapes never fabricate a timestamp on read (R16).
ban crates 'serde\(default *= *"[A-Za-z_:]*(now|now_utc)"'    "a persisted timestamp must not default to the current time"

# Retired paths stay retired (R17). List a path here when you delete a module.
for retired in \
  crates/office/src/commands \
  crates/office/src/audit_middleware.rs \
  crates/office/src/output/render.rs \
  crates/office/src/output/table.rs \
  crates/office-core/src/note.rs \
  crates/office-core/src/query.rs \
  crates/office-core/src/store; do
  [[ -e "$retired" ]] && err "retired path exists again: $retired"
done

if [[ "$fail" -eq 0 ]]; then
  echo "dependency-direction: ok"
fi
exit "$fail"
