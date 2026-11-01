"""Isolated Python bootstrap: import only the installed plugin's sibling modules."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from mcp import main  # noqa: E402

main()
