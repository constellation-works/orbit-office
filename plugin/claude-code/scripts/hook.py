"""Isolated Python bootstrap for fail-open provider hooks."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from scribe import main  # noqa: E402

raise SystemExit(main())
