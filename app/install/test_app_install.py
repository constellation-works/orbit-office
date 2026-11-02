"""Make-entry-point regressions; all service, git and npm effects are fixtures."""
import http.server
import json
import os
from pathlib import Path
import shutil
import tarfile
import tempfile
import threading
import unittest
from types import SimpleNamespace

from app_install import run


SOURCE = Path(__file__).resolve().parents[2]
PLUGIN_MAKEFILE = SOURCE / "plugin/Makefile"
COMMIT = "a" * 40


class InstallTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="office-app-install-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        shutil.copy(SOURCE / "Makefile", self.repo)
        shutil.copytree(SOURCE / "app/install", self.repo / "app/install")
        self.app = self.root / "served app"
        (self.app / "packages/web/dist").mkdir(parents=True)
        (self.app / "packages/web/dist/build-info.json").write_text('{"commit":"old"}')
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.log = self.root / "calls"
        self.log.touch()
        fixture = self.root / "fixture"
        (fixture / "app/packages/web/src/pwa").mkdir(parents=True)
        (fixture / "app/packages/web/src/pwa/sw.ts").write_text('const SHELL_CACHE = "office-shell-UNSTAMPED";')
        (fixture / "Makefile").write_text("app-deps:\n\t@npm --prefix app ci\n")
        self.archive = self.root / "source.tar"
        with tarfile.open(self.archive, "w") as archive:
            archive.add(fixture / "app", arcname="app")
            archive.add(fixture / "Makefile", arcname="Makefile")
        self.stub("git", '''echo "git $*" >> "$FIXTURE_LOG"
if [ "$1" = -C ]; then shift 2; fi
if [ "$1" = archive ]; then
  test "$2" = "$FIXTURE_COMMIT"
  cat "$FIXTURE_ARCHIVE"
elif [ "${2:-}" = --show-toplevel ]; then
  echo "$FIXTURE_REPO"
elif [ "${3:-}" = 'HEAD^{commit}' ] && [ -n "${FIXTURE_HEAD:-}" ]; then
  echo "$FIXTURE_HEAD"
else echo "$FIXTURE_COMMIT"; fi''')
        self.stub("npm", '''echo "npm $*" >> "$FIXTURE_LOG"
if [ "${4:-}" = build ]; then
  if [ "${FAIL_BUILD:-}" = 1 ]; then echo "fixture compiler error" >&2; exit 1; fi
  for pkg in web server contracts; do mkdir -p "$2/packages/$pkg/dist"; done
  mkdir -p "$2/packages/web/dist/pwa"
  cp "$2/packages/web/src/pwa/sw.ts" "$2/packages/web/dist/pwa/sw.js"
  echo "new server" > "$2/packages/server/dist/main.js"
fi''')
        self.stub("uname", 'echo "${FIXTURE_OS:-Linux}"')
        for command in ["launchctl", "systemctl"]:
            self.stub(command, f'echo "{command} $*" >> "$FIXTURE_LOG"')
        self.stub("orbit", 'echo "orbit $*" >> "$FIXTURE_LOG"; echo "office plugin"')
        self.stub("make-step", '''if [ "$1" = app-install ]; then exec /usr/bin/make "$@"; fi
echo "plugin export $*" >> "$FIXTURE_LOG"''')
        self.home = self.root / "home"
        self.home.mkdir()
        self.env = {"PATH": str(self.bin) + os.pathsep + os.defpath,
                    "HOME": str(self.home), "TMPDIR": str(self.root),
                    "FIXTURE_ARCHIVE": str(self.archive), "FIXTURE_LOG": str(self.log),
                    "FIXTURE_REPO": str(self.repo),
                    "FIXTURE_COMMIT": COMMIT, "PYTHONDONTWRITEBYTECODE": "1"}
        app = self.app
        fixture_state = self
        self.requests = 0
        self.report_wrong_commit = False
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self):
                fixture_state.requests += 1
                data = (app / "packages/web/dist/build-info.json").read_bytes()
                if fixture_state.report_wrong_commit:
                    data = b'{"commit":"wrong","build":"wrong"}'
                self.send_response(200)
                self.end_headers()
                self.wfile.write(data)
            def log_message(self, *args):
                pass
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=self.server.serve_forever)
        thread.start()
        def stop():
            self.server.shutdown()
            self.server.server_close()
            thread.join(5)
        self.addCleanup(stop)
        self.port = self.server.server_port
        unit = self.home / ".config/systemd/user/orbit-office.service"
        unit.parent.mkdir(parents=True)
        unit.write_text(f"[Service]\nExecStart=/usr/bin/node {self.app}/packages/server/dist/main.js\nEnvironment=PORT={self.port}\n")

    def stub(self, name, body):
        file = self.bin / name
        file.write_text("#!/bin/sh\nset -eu\n" + body + "\n")
        file.chmod(0o755)

    def run_make(self, target, **env):
        try:
            text = run(["make", "-s", target, f"MAKE={self.bin / 'make-step'}"], cwd=self.repo,
                       env={**self.env, **env}, timeout=30)
            return SimpleNamespace(returncode=0, stdout=text, stderr="")
        except RuntimeError as error:
            return SimpleNamespace(returncode=1, stdout="", stderr=str(error))

    def test_app_only_builds_publishes_and_checks_configured_port(self):
        result = self.run_make("app-install")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(json.loads((self.app / "packages/web/dist/build-info.json").read_text())["commit"], COMMIT)
        calls = self.log.read_text()
        self.assertIn("systemctl --user restart orbit-office.service", calls)
        self.assertNotIn("orbit plugin", calls)
        self.assertIn(COMMIT, result.stdout)
        self.assertGreater(self.requests, 0, "installation must check the running HTTP service")

    def test_failed_build_never_restarts_or_changes_served_files(self):
        result = self.run_make("app-install", FAIL_BUILD="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fixture compiler error", result.stderr)
        self.assertNotIn("systemctl", self.log.read_text())
        self.assertEqual(json.loads((self.app / "packages/web/dist/build-info.json").read_text())["commit"], "old")

    def test_macos_uses_kickstart(self):
        import plistlib
        plist = self.home / "Library/LaunchAgents/com.orbit-office.app.plist"
        plist.parent.mkdir(parents=True)
        plist.write_bytes(plistlib.dumps({"ProgramArguments": ["/usr/bin/node", str(self.app / "packages/server/dist/main.js")],
                                         "EnvironmentVariables": {"PORT": str(self.port)}}))
        result = self.run_make("app-install", FIXTURE_OS="Darwin")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn(f"launchctl kickstart -k gui/{os.getuid()}/com.orbit-office.app", self.log.read_text())
        self.assertNotIn("systemctl", self.log.read_text())

    def test_plugin_install_builds_and_checks_app_before_plugin_upgrade(self):
        result = self.run_make("plugin-install")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = self.log.read_text()
        self.assertLess(calls.index("npm"), calls.index("systemctl"))
        self.assertLess(calls.index("systemctl"), calls.index("orbit plugin upgrade"))
        self.assertIn("PLUGIN_REVISION=" + COMMIT, calls)
        self.assertIn(COMMIT, result.stdout)

    def test_failed_app_build_stops_plugin_upgrade(self):
        result = self.run_make("plugin-install", FAIL_BUILD="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("fixture compiler error", result.stderr)
        self.assertNotIn("orbit plugin", self.log.read_text())
        self.assertNotIn("systemctl", self.log.read_text())

    def test_reported_commit_mismatch_fails_and_preserves_previous_files(self):
        self.report_wrong_commit = True
        result = self.run_make("plugin-install")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("app commit check failed", result.stderr)
        self.assertIn("wrong", result.stderr)
        self.assertNotIn("orbit plugin", self.log.read_text())
        self.assertEqual(json.loads((self.app / "packages/web/dist/build-info.json").read_text())["commit"], "old")

    def test_plugin_export_uses_the_pinned_commit_even_if_head_changed(self):
        plugin = self.repo / "plugin"
        plugin.mkdir()
        shutil.copy(PLUGIN_MAKEFILE, plugin / "Makefile")
        fixture = self.root / "export-fixture/plugin/.orbit-plugin/bin"
        fixture.mkdir(parents=True)
        (fixture.parent / "metadata").write_text(COMMIT)
        with tarfile.open(self.archive, "w") as archive:
            archive.add(fixture.parents[1], arcname="plugin")
        self.stub("cargo", '''mkdir -p "$PWD/target"
while [ "$1" != --target-dir ]; do shift; done
mkdir -p "$2/release"
printf '#!/bin/sh\\necho pinned\\n' > "$2/release/office"
chmod +x "$2/release/office"''')
        output = self.root / "exported"
        text = run(["make", "-s", "-C", str(plugin), "export", f"PLUGIN_REVISION={COMMIT}", f"OUT={output}"],
                   env={**self.env, "FIXTURE_HEAD": "b" * 40}, timeout=30)
        self.assertEqual((output / ".orbit-plugin/metadata").read_text(), COMMIT)
        self.assertTrue((output / ".orbit-plugin/bin/office-bin").is_file())
        self.assertIn(str(output), text)


if __name__ == "__main__":
    unittest.main()
