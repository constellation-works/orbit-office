"""The exec launcher's environment contract with Orbit."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


PLUGIN = Path(__file__).resolve().parents[1] / ".orbit-plugin"


class LauncherTmpdir(unittest.TestCase):
    # ORB-21397: TMPDIR must point inside plugin state. Unset, media staging
    # fell back to /tmp, which Linux Landlock denies.
    def test_tmpdir_is_created_under_plugin_state(self):
        with tempfile.TemporaryDirectory(prefix="office-launcher-test-") as scratch:
            root = Path(scratch) / "plugin"
            (root / "bin").mkdir(parents=True)
            shutil.copy(PLUGIN / "bin" / "office", root / "bin" / "office")
            stub = root / "bin" / "office-bin"
            stub.write_text('#!/bin/sh\nprintf %s "$TMPDIR"\n', encoding="utf-8")
            stub.chmod(0o755)
            state = Path(scratch) / "state"
            state.mkdir()
            env = {
                "PATH": os.environ.get("PATH", ""),
                "ORBIT_PLUGIN_ROOT": str(root),
                "ORBIT_PLUGIN_STATE": str(state),
                "TMPDIR": str(Path(scratch) / "caller-tmp"),
            }
            out = subprocess.run(
                ["/bin/sh", str(root / "bin" / "office")],
                env=env,
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=10,
                check=True,
            )
            self.assertEqual(out.stdout, str(state / "tmp"))
            self.assertTrue((state / "tmp").is_dir())


if __name__ == "__main__":
    unittest.main()
