.PHONY: check lint test sync export

sync:
	uv sync

lint:
	uv lock --check
	uv run ruff check .orbit-plugin/src tests
	uv run ruff format --check .orbit-plugin/src tests
	uv run basedpyright

test:
	uv run pytest -q

check: lint test

# Export the committed plugin tree for `orbit plugin validate/test/add`.
# Always from HEAD, never the working tree; the path names the commit.
# The export root holds .orbit-plugin/, as Orbit expects.
export:
	@top=$$(git rev-parse --show-toplevel); sha=$$(git rev-parse --short HEAD); \
	out=/tmp/office-plugin-$$sha; rm -rf "$$out" && mkdir "$$out" && \
	git -C "$$top" archive HEAD plugin | tar -x -C "$$out" --strip-components=1 && echo "$$out"
