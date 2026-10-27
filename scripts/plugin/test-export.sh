#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/office-export-test.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
repo=$tmp/repo
mkdir -p "$repo/plugin/.orbit-plugin/bin" "$repo/src" "$tmp/home" "$tmp/bin" "$tmp/outputs"
export CARGO_HOME=${CARGO_HOME:-$HOME/.cargo} RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}
export HOME=$tmp/home GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
unset OFFICE_HOME OFFICE_CONFIG OFFICE_STATE 2>/dev/null || :
git -C "$repo" init -q
git -C "$repo" config user.email test@example.invalid
git -C "$repo" config user.name Test
cp "$root/plugin/Makefile" "$repo/plugin/Makefile"
cat > "$repo/Cargo.toml" <<'TOML'
[package]
name = "office"
version = "0.1.0"
edition = "2021"
TOML
cat > "$repo/Cargo.lock" <<'LOCK'
version = 3

[[package]]
name = "office"
version = "0.1.0"
LOCK
printf 'fn main() { println!("committed-one"); }\n' > "$repo/src/main.rs"
printf 'placeholder\n' > "$repo/plugin/.orbit-plugin/bin/.keep"
printf 'metadata-one\n' > "$repo/plugin/.orbit-plugin/metadata"
git -C "$repo" add .
git -C "$repo" commit -qm one
first=$(git -C "$repo" rev-parse HEAD)
printf 'fn main() { println!("committed-two"); }\n' > "$repo/src/main.rs"
printf 'metadata-two\n' > "$repo/plugin/.orbit-plugin/metadata"
git -C "$repo" add .
git -C "$repo" commit -qm two
second=$(git -C "$repo" rev-parse HEAD)
git -C "$repo" reset -q --hard "$first"
printf 'fn main() { println!("dirty-backend"); }\n' > "$repo/src/main.rs"
printf 'untouched\n' > "$tmp/outputs/sentinel"
real_cargo=$(command -v cargo)
cat > "$tmp/bin/cargo" <<'WRAPPER'
#!/bin/sh
if [ -n "${MOVE_HEAD_TO:-}" ]; then
    git -C "$FIXTURE_REPO" update-ref HEAD "$MOVE_HEAD_TO"
fi
exec "$REAL_CARGO" "$@"
WRAPPER
chmod +x "$tmp/bin/cargo"
export REAL_CARGO=$real_cargo FIXTURE_REPO=$repo
export PATH="$tmp/bin:$PATH"
short=$(git -C "$repo" rev-parse --short "$first")
out=$tmp/outputs/office-plugin-$short
actual=$(TMPDIR="$tmp/outputs" make -s -C "$repo/plugin" export)
[ "$actual" = "$out" ] || { echo "unexpected export path: $actual" >&2; exit 1; }
[ "$(cat "$out/.orbit-plugin/metadata")" = metadata-one ] || { echo 'metadata did not come from resolved commit' >&2; exit 1; }
[ "$("$out/.orbit-plugin/bin/office-bin")" = committed-one ] || { echo 'backend did not come from resolved commit' >&2; exit 1; }
# A second export advances HEAD exactly when the build starts.
mkdir "$tmp/moving"
out2=$tmp/moving/office-plugin-$short
actual=$(TMPDIR="$tmp/moving" MOVE_HEAD_TO=$second make -s -C "$repo/plugin" export)
[ "$actual" = "$out2" ]
[ "$(cat "$out2/.orbit-plugin/metadata")" = metadata-one ] || { echo 'moving HEAD changed metadata' >&2; exit 1; }
[ "$("$out2/.orbit-plugin/bin/office-bin")" = committed-one ] || { echo 'moving HEAD changed backend' >&2; exit 1; }
[ "$(git -C "$repo" rev-parse HEAD)" = "$second" ]
[ "$(cat "$tmp/outputs/sentinel")" = untouched ]
# Existing output is never overwritten, even on repeated export.
printf 'keep\n' > "$out/keep"
if make -s -C "$repo/plugin" export OUT="$out" > "$tmp/repeat.log" 2>&1; then
    echo 'export overwrote an existing destination' >&2; exit 1
fi
[ "$(cat "$out/keep")" = keep ]
printf 'plugin export provenance: passed\n'
