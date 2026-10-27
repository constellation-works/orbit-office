"""Exercise activity registration through the installed Orbit plugin loader."""

import os
from pathlib import Path
import shutil
import signal
import subprocess
import tempfile
import unittest


PLUGIN = Path(__file__).resolve().parents[1] / ".orbit-plugin"
ACTIVITY = Path("definitions/activities/office-agent-todo.yaml")


class AgentTodoActivity(unittest.TestCase):
    def setUp(self):
        self.orbit = shutil.which("orbit")
        self.assertIsNotNone(self.orbit, "Orbit >=0.34 is required for activity validation")
        self.scratch = tempfile.TemporaryDirectory(prefix="office-activity-test-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.plugin = self.root / "plugin" / ".orbit-plugin"
        shutil.copytree(PLUGIN, self.plugin)
        (self.root / "home").mkdir()

    def validate(self):
        # Keep managed-run authority on the child, while isolating HOME and
        # office paths. Validation reads assets; it never installs the plugin.
        env = {
            key: value
            for key, value in os.environ.items()
            if key == "PATH" or key.startswith("ORBIT_")
        }
        env.update(
            HOME=str(self.root / "home"),
            OFFICE_HOME=str(self.root / "office"),
            OFFICE_CONFIG=str(self.root / "config"),
            OFFICE_STATE=str(self.root / "state"),
        )
        command = [self.orbit, "plugin", "validate", str(self.plugin.parent)]
        child = subprocess.Popen(
            command,
            cwd=self.root,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            process_group=0,
        )
        try:
            stdout, stderr = child.communicate(timeout=30)
            return subprocess.CompletedProcess(command, child.returncode, stdout, stderr)
        finally:
            # An unreaped leader pins the group identity. Never signal the
            # group after communicate has reaped that leader (STD-03 R14).
            if child.returncode is None:
                try:
                    os.killpg(child.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    child.communicate(timeout=1)
                except subprocess.TimeoutExpired:
                    if child.returncode is None:
                        try:
                            os.killpg(child.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    child.communicate(timeout=5)

    def test_registered_activity_is_valid(self):
        result = self.validate()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_validator_loads_the_registered_activity(self):
        (self.plugin / ACTIVITY).write_text("not valid YAML: [", encoding="utf-8")
        result = self.validate()
        self.assertNotEqual(
            result.returncode, 0, "The loader must reject a malformed registered activity"
        )
        self.assertIn("office-agent-todo.yaml", result.stderr)

    def test_spawn_requires_a_program_policy(self):
        activity = self.plugin / ACTIVITY
        lines = activity.read_text(encoding="utf-8").splitlines(keepends=True)
        activity.write_text(
            "".join(line for line in lines if not line.lstrip().startswith("proc_allowed_programs:")),
            encoding="utf-8",
        )
        result = self.validate()
        self.assertNotEqual(
            result.returncode, 0, "The loader must refuse proc.spawn without its program policy"
        )
        self.assertIn("omits a program policy", result.stderr)


if __name__ == "__main__":
    unittest.main()
