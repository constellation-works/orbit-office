"""Exercise activity registration through the installed Orbit plugin loader."""

import json
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


class ConversationExtractSeeding(unittest.TestCase):
    """A normal enable seeds office-conversation-extract disabled (ORB-20899)."""

    def setUp(self):
        self.orbit = shutil.which("orbit")
        self.assertIsNotNone(self.orbit, "Orbit >=0.34 is required for seeding checks")
        self.scratch = tempfile.TemporaryDirectory(prefix="office-seed-test-")
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.orbit_root = self.root / "orbit-root"
        self.orbit_root.mkdir()
        self.tmpdir = self.root / "tmp"
        self.tmpdir.mkdir()
        self.workspace = self.root / "ws"
        self.workspace.mkdir()
        self.export = self.root / "export"
        shutil.copytree(PLUGIN, self.export / ".orbit-plugin")
        stub = self.export / ".orbit-plugin" / "bin" / "office-bin"
        stub.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
        stub.chmod(0o755)

    def run_orbit(self, args, timeout=60):
        # Isolate the child from the managed run: a fresh HOME, a fresh
        # ORBIT_ROOT and a disposable workspace. Seeding never touches the
        # host's Orbit state and never enables automation or dispatches work.
        env = {
            "PATH": os.environ.get("PATH", ""),
            "HOME": str(self.home),
            "ORBIT_ROOT": str(self.orbit_root),
            "TMPDIR": str(self.tmpdir),
        }
        command = [self.orbit, *args]
        child = subprocess.Popen(
            command,
            cwd=self.workspace,
            env=env,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            process_group=0,
        )
        try:
            stdout, stderr = child.communicate(timeout=timeout)
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

    def test_conversation_extract_seeded_disabled(self):
        result = self.run_orbit(["workspace", "init", "--name", "seed-probe"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

        result = self.run_orbit(["plugin", "add", str(self.export)])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("office.capture", result.stdout)

        result = self.run_orbit(
            [
                "plugin",
                "enable",
                "office",
                "--grant",
                "fs={{workspace}},{{plugin_state}}",
                "--grant",
                "network",
                "--grant",
                "orbit_tools",
                "--grant",
                "agent_tools",
            ],
            timeout=120,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

        result = self.run_orbit(["auto-task", "list", "--json"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        definitions = json.loads(result.stdout)
        matching = [
            item for item in definitions if item.get("name") == "office-office-conversation-extract"
        ]
        self.assertEqual(
            len(matching), 1, "a fresh enable must seed office-conversation-extract once"
        )
        seeded = matching[0]
        self.assertIs(seeded.get("enabled"), False, "the sweep must stay disabled")
        self.assertEqual(seeded.get("dedupe"), "skip_if_open")

        result = self.run_orbit(["auto-task", "show", "office-office-conversation-extract", "--json"])
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        shown = json.loads(result.stdout)
        required = shown.get("template", {}).get("required_tools", [])
        for tool in ("office.capture", "orbit.task.show", "orbit.task.artifact.put"):
            self.assertIn(tool, required)


if __name__ == "__main__":
    unittest.main()
