#!/usr/bin/env bash
# scripts/check-terminal-guard.sh — STD-01 R12/R17 and STD-02 R15.
#
# Only the CLI's output module may touch stdout/stderr, query whether a
# stream is a terminal, or read the color and format environment. Anything
# else doing so would make a second, disagreeing decision.
set -euo pipefail
cd "$(dirname "$0")/.."

OUTPUT_DIR=crates/office/src/output
PATTERN='is_terminal|IsTerminal|io::stdout|io::stderr|"NO_COLOR"|"CLICOLOR_FORCE"|"TERM"|"COLUMNS"|OFFICE_FORMAT|println!|eprintln!|print!|eprint!'

hits=$(grep -rnE --include='*.rs' --exclude-dir=tests "$PATTERN" crates \
  | grep -v "^$OUTPUT_DIR/" \
  | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)

if [[ -n "$hits" ]]; then
  echo "$hits" >&2
  echo "terminal-guard: terminal state and std streams belong to $OUTPUT_DIR only" >&2
  exit 1
fi
echo "terminal-guard: ok"
