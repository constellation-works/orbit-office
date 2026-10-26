.PHONY: check lint test sync export

sync:
	uv sync

lint:
	uv lock --check
	uv run ruff check src tests
	uv run ruff format --check src tests
	uv run basedpyright

test:
	uv run pytest -q

check: lint test

# Export the committed plugin tree (Orbit reads plugin.yaml at its root).
# Always from HEAD, never the working tree; the path names the commit.
export:
	@sha=$$(git rev-parse --short HEAD); out=/tmp/office-plugin-$$sha; \
	rm -rf "$$out" && mkdir "$$out" && git archive HEAD plugin | tar -x -C "$$out" --strip-components=1 && echo "$$out"
