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
# Surface crates: argument parsing, terminals, log subscribers.
SURFACE="clap tracing-subscriber anstream anstyle termcolor terminal_size crossterm owo-colors colored"
# HTTP clients: only office-models (and the composing binary) may speak HTTP.
HTTP="ureq reqwest hyper hyper-util isahc attohttpc curl"

# policy <crate>  -> sets ALLOWED (internal deps) and BANNED (external deps).
# No crate lists office-testkit: it is a dev-dependency only, and
# dev-dependencies are exempt from this check.
policy() {
  case "$1" in
    office-common)
      ALLOWED=""
      BANNED="$SURFACE $HTTP"
      ;;
    office-core)
      ALLOWED="office-common"
      BANNED="$SURFACE $HTTP"
      ;;
    office-store)
      # LibraryStore implements the core Library port; records/settings use this edge.
      ALLOWED="office-common office-core"
      BANNED="$SURFACE $HTTP"
      ;;
    office-orbit)
      ALLOWED="office-common office-core"
      BANNED="$SURFACE $HTTP"
      ;;
    office-models)
      ALLOWED="office-common office-core"
      BANNED="$SURFACE"
      ;;
    office-tools)
      # The application reaches adapters only through office-core ports.
      ALLOWED="office-common office-core"
      BANNED="$SURFACE $HTTP"
      ;;
    office-testkit)
      # Fake port implementations depend on office-core; consumers use them only in tests.
      ALLOWED="office-common office-core office-store"
      BANNED="$SURFACE $HTTP"
      ;;
    office)
      ALLOWED="office-common office-core office-store office-models office-orbit office-tools"
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
      err "$crate must not depend on $dep (banned for this crate; see ARCHITECTURE.md)"
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
TOOLS=crates/office-tools/src
MODELS=crates/office-models/src
CLI=crates/office/src

# office-store contracts are I/O-free record shapes (R5); the drivers own
# all filesystem and SQLite access.
ban "$STORE/contracts.rs" 'std::(fs|net|process)' "office-store contracts are record shapes: I/O belongs in driver/"

# Core is the pure domain (R5): every core module is I/O-free.
for path in "$CORE"/*; do
  name=$(basename "$path")
  case "$name" in tests) continue ;; esac
  ban "$path" 'std::(fs|net|process)|tempfile::|rustix::' "core/$name is pure domain: file, network and process access belong in an adapter crate"
done
# The domain receives resolved inputs in the call context (R3).
ban "$CORE"          'std::env::|home_dir|current_dir'           "the core crate must not read the environment or discover paths"
# The application reaches files, models and Orbit only through ports.
ban "$TOOLS"         'std::(fs|net|process)|tempfile::|rustix::' "office-tools is the application: use an office-core port, not direct I/O"
ban "$TOOLS"         'std::env::|home_dir|current_dir'           "office-tools must not read the environment or discover paths"
# Model calls are HTTP only; the one subprocess exception is media.rs
# (ffmpeg audio splitting, STD-03 supervised), so std::process anywhere
# else in office-models fails here.
MODELS_PROCESS_HITS=$(grep -rnHE --include='*.rs' --exclude-dir=tests 'std::process' "$MODELS" 2>/dev/null \
  | grep -v "^$MODELS/media.rs:" \
  | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)
if [[ -n "$MODELS_PROCESS_HITS" ]]; then
  echo "$MODELS_PROCESS_HITS" >&2
  err "office-models speaks HTTP only: std::process is confined to media.rs (ffmpeg)"
fi
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
  crates/office-core/src/tools.rs \
  app/src \
  crates/office-core/src/query.rs \
  crates/office-core/src/store; do
  [[ -e "$retired" ]] && err "retired path exists again: $retired"
done

if [[ "$fail" -eq 0 ]]; then
  echo "dependency-direction: ok"
fi
exit "$fail"
