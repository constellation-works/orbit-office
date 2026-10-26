.PHONY: check plugin-check app-check

check: plugin-check

plugin-check:
	$(MAKE) -C plugin check

# Requires `npm install` in app/ first; kept separate so the plugin gate
# stays runnable on a Python-only machine.
app-check:
	cd app && npm run typecheck
