.PHONY: check export

# The Rust backend lives in the workspace at the repo root (crates/office);
# the full gate is the root `make ci`.
check:
	$(MAKE) -C .. ci

# Export the committed plugin tree for `orbit plugin validate/test/add`.
# Always from HEAD, never the working tree; the path names the commit.
# The export root holds .orbit-plugin/, as Orbit expects, with the Rust
# backend prebuilt into .orbit-plugin/bin/office-bin (a local directory
# install never runs spec.build, so the binary must already exist).
export:
	cd .. && cargo build --release --locked -p office
	@top=$$(git rev-parse --show-toplevel); sha=$$(git rev-parse --short HEAD); \
	out=/tmp/office-plugin-$$sha; rm -rf "$$out" && mkdir "$$out" && \
	git -C "$$top" archive HEAD plugin | tar -x -C "$$out" --strip-components=1 && \
	cp "$$top/target/release/office" "$$out/.orbit-plugin/bin/office-bin" && echo "$$out"
