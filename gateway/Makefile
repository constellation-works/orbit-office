.PHONY: help test check export

# The gateway plugins: Python 3.12, standard library only (ARCHITECTURE.md,
# "Gateway plugins"). Every command runs the interpreter isolated (-I), so no
# user site-packages, PYTHON* variable or working-directory module is used.
PYTHON ?= python3
# The plugin `make export` exports, and where. The default path names the
# commit the working tree sits on; pass OUT=<dir> to choose another.
PLUGIN ?= gateway
OUT ?=

help:
	@echo "gateway make targets"
	@echo ""
	@echo "  make test             unittest suite (standard library only)"
	@echo "  make check            version check, import-direction scan, schema JSON"
	@echo "  make export [OUT=dir] export plugins/$(PLUGIN) with src/orbit_gateway in lib/"

test:
	$(PYTHON) -I -m unittest discover -s tests -t . -v

check:
	$(PYTHON) -I -c 'import sys; v = sys.version_info; sys.exit(0 if v >= (3, 12) else "error: Python 3.12+ is required, found %d.%d" % v[:2])'
	$(PYTHON) -I scripts/check_imports.py
	@for f in plugins/*/.orbit-plugin/schemas/*.json; do $(PYTHON) -I -m json.tool "$$f" >/dev/null || exit 1; done; echo "schemas: ok"

export:
	@out="$(OUT)"; \
	if [ -z "$$out" ]; then out="$${TMPDIR:-/tmp}/orbit-$(PLUGIN)-plugin-$$(git rev-parse --short HEAD)"; rm -rf "$$out"; fi; \
	$(PYTHON) -I scripts/export.py $(PLUGIN) "$$out"
