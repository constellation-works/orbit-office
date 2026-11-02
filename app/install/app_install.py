"""Build a pinned app, then publish and restart the already registered login service."""
import contextlib
import fcntl
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request


# Keep the group leader alive until its parent sweeps the group. Unlike
# waitid(WNOWAIT), this also works with the installer's Python 3.12 on macOS.
COMMAND_SUPERVISOR = """
import os, select, subprocess, sys
status = subprocess.call(sys.argv[3:], stdin=subprocess.DEVNULL)
os.write(int(sys.argv[1]), (str(status) + '\\n').encode())
if select.select([int(sys.argv[2])], [], [], 5)[0]:
    os.read(int(sys.argv[2]), 1)
"""


def run(arguments, *, cwd=None, timeout=1200, destination=None, env=None):
    """Own the process group until it is swept and the leader is reaped."""
    with contextlib.ExitStack() as resources:
        output = resources.enter_context(tempfile.TemporaryFile())
        status_read, status_write = os.pipe()
        control_read, control_write = os.pipe()
        reader = resources.enter_context(os.fdopen(status_read, "rb", buffering=0))
        writer = resources.enter_context(os.fdopen(status_write, "wb", buffering=0))
        control_reader = resources.enter_context(os.fdopen(control_read, "rb", buffering=0))
        resources.enter_context(os.fdopen(control_write, "wb", buffering=0))
        child = subprocess.Popen([sys.executable, "-c", COMMAND_SUPERVISOR,
                                  str(status_write), str(control_read), *arguments],
                                 cwd=cwd, stdin=subprocess.DEVNULL,
                                 stdout=output, stderr=subprocess.STDOUT,
                                 start_new_session=True, env=env,
                                 pass_fds=(status_write, control_read))
        writer.close()
        control_reader.close()
        deadline = time.monotonic() + timeout
        completed = False
        try:
            payload = b""
            while b"\n" not in payload:
                if time.monotonic() >= deadline or output.tell() > 128 * 1024 * 1024:
                    raise RuntimeError(f"command exceeded its time or output limit: {arguments[0]}")
                if select.select([reader], [], [], 0.1)[0]:
                    chunk = reader.read(32)
                    if not chunk or len(payload) + len(chunk) > 32:
                        raise RuntimeError(f"command supervisor did not report a valid outcome: {arguments[0]}")
                    payload += chunk
            status = int(payload)
            completed = True
        finally:
            # Do this before wait() reaps the leader, including on a clean exit.
            if not completed:
                with contextlib.suppress(ProcessLookupError):
                    os.killpg(child.pid, signal.SIGTERM)
                grace = time.monotonic() + 2
                while time.monotonic() < grace:
                    try:
                        os.killpg(child.pid, 0)
                    except ProcessLookupError:
                        break
                    except PermissionError:
                        pass  # An unknown group state counts as survival.
                    time.sleep(0.05)
            with contextlib.suppress(ProcessLookupError):
                os.killpg(child.pid, signal.SIGKILL)
            child.wait(timeout=5)
        if destination is not None and status == 0:
            output.seek(0)
            with open(destination, "wb") as file:
                shutil.copyfileobj(output, file)
            return ""
        size = output.tell()
        output.seek(max(0, size - 65536))
        text = output.read().decode("utf-8", errors="replace")
        if size > 65536:
            text = "[output truncated to last 65536 bytes]\n" + text
        if status != 0:
            outcome = f"signal {-status}" if status < 0 else f"exit {status}"
            raise RuntimeError(f"{' '.join(arguments)} failed ({outcome}):\n{text}")
        return text.strip()


def stamp(app, commit):
    """Stamp compiled output; source and package manifests remain ordinary tsc inputs."""
    worker = app / "packages/web/dist/pwa/sw.js"
    source = worker.read_text()
    pattern = r'const SHELL_CACHE = "office-shell-[^"]+";'
    canonical, count = re.subn(pattern, 'const SHELL_CACHE = "office-shell-UNSTAMPED";', source)
    if count != 1:
        raise RuntimeError("app build error: compiled worker has no unique shell-cache stamp")
    digest = hashlib.sha256(commit.encode())
    for root in [app / "packages/web/dist", app / "packages/web/static"]:
        if not root.exists():
            continue
        for file in sorted(root.rglob("*")):
            if not file.is_file() or file.name == "build-info.json":
                continue
            digest.update(str(file.relative_to(app)).encode() + b"\0")
            digest.update(canonical.encode() if file == worker else file.read_bytes())
    identity = digest.hexdigest()
    atomic_write(worker, re.sub(pattern, f'const SHELL_CACHE = "office-shell-{identity}";', source))
    atomic_write(app / "packages/web/dist/build-info.json", json.dumps({"commit": commit, "build": identity}) + "\n")
    return identity


def atomic_write(path, text):
    descriptor, temporary = tempfile.mkstemp(prefix=".office-write-", dir=path.parent)
    try:
        with os.fdopen(descriptor, "w") as file:
            file.write(text)
            file.flush()
            os.fsync(file.fileno())
        os.replace(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temporary)


def service():
    system = run(["uname", "-s"], timeout=5)
    home = Path.home()
    suffix = "/packages/server/dist/main.js"
    if system == "Darwin":
        config = home / "Library/LaunchAgents/com.orbit-office.app.plist"
        if not config.is_file():
            raise RuntimeError(f"app login service is missing at {config}; provision it with install.sh before app-install")
        with config.open("rb") as file:
            data = plistlib.load(file)
        args = data["ProgramArguments"]
        if len(args) != 2:
            raise RuntimeError(f"unsupported app service arguments in {config}")
        entry = args[1]
        port = data.get("EnvironmentVariables", {}).get("PORT", "4317")
        command = ["launchctl", "kickstart", "-k", f"gui/{os.getuid()}/com.orbit-office.app"]
    elif system == "Linux":
        config = home / ".config/systemd/user/orbit-office.service"
        if not config.is_file():
            raise RuntimeError(f"app login service is missing at {config}; provision it with install.sh before app-install")
        if (config.parent / (config.name + ".d")).exists():
            raise RuntimeError(f"custom app service drop-ins exist beside {config}; reconcile them before app-install")
        text = config.read_text()
        # This is the installer's own unit format. Refuse customized command
        # lines rather than guessing which tree or endpoint to overwrite.
        entries = re.findall(r"^ExecStart=\S+ (/.+/packages/server/dist/main\.js)$", text, re.M)
        ports = re.findall(r'^Environment="?PORT=(\d+)"?$', text, re.M)
        if len(entries) != 1 or len(ports) > 1:
            raise RuntimeError(f"unsupported app service definition in {config}")
        entry = entries[0]
        port = ports[0] if ports else "4317"
        command = ["systemctl", "--user", "restart", "orbit-office.service"]
    else:
        raise RuntimeError(f"app-install supports macOS and Linux, got {system}")
    if not entry.endswith(suffix) or not str(port).isdigit() or not 0 < int(port) < 65536:
        raise RuntimeError(f"invalid app entrypoint or port in {config}")
    app = Path(entry[:-len(suffix)]).resolve(strict=True)
    return app, f"http://127.0.0.1:{port}/build-info.json", command, config


@contextlib.contextmanager
def install_lock(app):
    path = app.parent / ".office-app-install.lock"
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, 0o600)
    with os.fdopen(descriptor, "r+") as file:
        deadline = time.monotonic() + 30
        while True:
            try:
                fcntl.flock(file, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    file.seek(0)
                    raise RuntimeError(f"app-install lock timeout: {path}; holder: {file.read(1024)}")
                time.sleep(0.1)
        file.seek(0)
        file.truncate()
        file.write(f"pid={os.getpid()} started={time.time()} app-install {app}\n")
        file.flush()
        yield


def verify(url, commit, identity):
    # A bounded local probe, bypassing proxy variables. Build-info is never
    # precached by the worker and this request does not go through a browser.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    deadline = time.monotonic() + 15
    last = "no response"
    while time.monotonic() < deadline:
        try:
            with opener.open(urllib.request.Request(url, headers={"Cache-Control": "no-cache"}), timeout=1) as response:
                data = json.loads(response.read(4096))
            if not isinstance(data, dict):
                raise ValueError("build-info response must be a JSON object")
            if data.get("commit") == commit and data.get("build") == identity:
                return
            last = f"reported commit {data.get('commit')!r}, build {data.get('build')!r}"
        except (OSError, ValueError, urllib.error.URLError) as error:
            last = str(error)
        time.sleep(0.2)
    raise RuntimeError(f"app commit check failed at {url}: expected {commit} / {identity}; {last}")


def install(repo, revision):
    commit = run(["git", "rev-parse", "--verify", revision + "^{commit}"], cwd=repo, timeout=10)
    if re.fullmatch(r"[0-9a-f]{40,64}", commit) is None:
        raise RuntimeError("git did not return a full commit identity")
    app, url, restart, config = service()
    with install_lock(app):
        pending = app.parent / ".office-app-install.pending.json"
        if pending.exists():
            raise RuntimeError(f"an interrupted app publication needs recovery; inspect {pending} before installing again")
        config_bytes = config.read_bytes()
        with tempfile.TemporaryDirectory(prefix="office-app-build-") as scratch:
            source = Path(scratch)
            archive = source / "source.tar"
            run(["git", "archive", commit, "Makefile", "app"], cwd=repo, destination=archive)
            run(["tar", "-xf", str(archive), "-C", str(source)], timeout=30)
            try:
                run(["make", "-s", "-C", str(source), "app-deps"])
                run(["npm", "--prefix", str(source / "app"), "run", "build"])
                identity = stamp(source / "app", commit)
            except RuntimeError as error:
                raise RuntimeError(f"app build error; running service left untouched: {error}") from error
            if config.read_bytes() != config_bytes:
                raise RuntimeError("app service definition changed during build; rerun app-install")
            # Publish the compiled packages, static shell and dependencies.
            # TypeScript sources and app/service configuration stay intact.
            paths = [Path("packages") / package / "dist" for package in ["contracts", "server", "web"]]
            paths += [Path("packages/web/static"), Path("node_modules")]
            for path in paths:
                if not (app / path).parent.resolve().is_relative_to(app):
                    raise RuntimeError(f"app runtime path escapes the registered app root: {app / path}")
            backup = Path(tempfile.mkdtemp(prefix=".office-app-backup-", dir=app.parent))
            staging = Path(tempfile.mkdtemp(prefix=".office-app-stage-", dir=app.parent))
            changed = []
            try:
                for path in paths:
                    built = source / "app" / path
                    if built.exists():
                        shutil.copytree(built, staging / path, symlinks=True)
                atomic_write(pending, json.dumps({"app": str(app), "backup": str(backup),
                                                 "candidate": str(staging), "commit": commit,
                                                 "paths": [str(path) for path in paths]}) + "\n")
                for path in paths:
                    incoming = staging / path
                    if not incoming.exists():
                        continue
                    target = app / path
                    target.parent.mkdir(parents=True, exist_ok=True)
                    previous = backup / path
                    previous.parent.mkdir(parents=True, exist_ok=True)
                    if target.exists() or target.is_symlink():
                        target.rename(previous)
                    changed.append(path)
                    incoming.rename(target)
                run(restart, timeout=30)
                verify(url, commit, identity)
            except BaseException:
                # Keep both generations available for diagnosis; restore the
                # old directories even when the service command/check fails.
                for path in reversed(changed):
                    target = app / path
                    if target.exists() or target.is_symlink():
                        target.rename(staging / path)
                    previous = backup / path
                    if previous.exists() or previous.is_symlink():
                        previous.rename(target)
                print(f"app-install failed; previous files restored; candidate retained at {staging}", file=sys.stderr)
                if changed:
                    try:
                        run(restart, timeout=30)
                    except RuntimeError as error:
                        print(f"previous service restart failed; run {' '.join(restart)} after repair: {error}", file=sys.stderr)
                pending.unlink(missing_ok=True)
                raise
            else:
                pending.unlink()
                shutil.rmtree(staging)
                print(f"app-install: {app} serves commit {commit} ({url}); previous runtime retained at {backup}")


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "stamp":
        stamp(Path(sys.argv[2]), sys.argv[3])
    elif len(sys.argv) == 3 and sys.argv[1] == "install":
        install(Path.cwd(), sys.argv[2])
    else:
        raise RuntimeError("usage: app_install.py install <revision> | stamp <app-root> <commit>")


if __name__ == "__main__":
    def interrupted(signum, _frame):
        raise InterruptedError(f"app-install interrupted by signal {signum}")
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        main()
    except (RuntimeError, OSError, ValueError, KeyError) as error:
        print(f"error: {error}", file=sys.stderr)
        sys.exit(1)
