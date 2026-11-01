.PHONY: check test

check:
	python3 -I -c 'import ast,pathlib; [ast.parse(p.read_text(),filename=str(p)) for p in pathlib.Path(".").rglob("*.py")]'

test: check
	python3 -I -m unittest discover -s tests -v
