.PHONY: check export

# The Rust backend lives in the workspace at the repo root (crates/office);
# the full gate is the root `make ci`.
check:
	$(MAKE) -C .. ci

# Export one resolved commit for `orbit plugin validate/test/add`. OUT can
# select a new destination; an existing destination is never replaced.
export:
	@set -eu; \
	top=$$(git rev-parse --show-toplevel); rev=$$(git rev-parse --verify 'HEAD^{commit}'); \
	sha=$$(git rev-parse --short "$$rev"); out="$(OUT)"; \
	if [ -z "$$out" ]; then out="$${TMPDIR:-/tmp}/office-plugin-$$sha"; fi; \
	work=$$(mktemp -d "$${TMPDIR:-/tmp}/office-plugin-build.XXXXXX"); \
	trap 'rm -rf "$$work"' EXIT HUP INT TERM; \
	git -C "$$top" archive "$$rev" > "$$work/source.tar"; \
	mkdir "$$work/source"; tar -xf "$$work/source.tar" -C "$$work/source"; \
	(cd "$$work/source" && cargo build --release --locked -p office --target-dir "$$work/target"); \
	mkdir "$$out" || { echo "export destination already exists or cannot be created: $$out" >&2; exit 1; }; \
	cp -R "$$work/source/plugin/." "$$out/"; \
	cp "$$work/target/release/office" "$$out/.orbit-plugin/bin/office-bin"; \
	printf '%s\n' "$$out"
