"""Small, bounded helpers for the synthetic demo, never host-state discovery."""
import json
import os
from pathlib import Path
import subprocess
import signal
import sys
import tempfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
FAKE_TOKEN = "SIMULATED-acme-token-not-a-credential-0123456789"


def run(argv, *, env=None, cwd=None, input=None, timeout=120):
    # Use the repository's bounded supervisor, also used by real callbacks.
    # Stdin is a private anonymous file, so provider tokens never reach argv.
    sys.path.insert(0, str(REPO / 'gateway/src'))
    from orbit_gateway.callbacks import supervise
    with tempfile.TemporaryFile(dir=env['TMPDIR'] if env else HERE) as payload:
        payload.write((input or '').encode())
        payload.seek(0)
        child = subprocess.Popen([str(v) for v in argv], stdin=payload,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 env=env, cwd=cwd, start_new_session=True)
        try:
            code, stdout, _, overflow = supervise(child, timeout, limit=8 * 1024 * 1024)
        finally:
            if child.returncode is None:
                try:
                    os.killpg(child.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                supervise(child, 2)
    if code != 0 or overflow:
        # Command input and remote responses can contain tokens; don't echo them.
        raise RuntimeError(f"{Path(argv[0]).name} exited {code}; output_limit={bool(overflow)}")
    return stdout.decode()


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    fd, staging = tempfile.mkstemp(prefix='.' + path.name, dir=path.parent)
    try:
        with os.fdopen(fd, 'w', encoding='utf-8') as stream:
            json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(staging, path)
    finally:
        if os.path.exists(staging):
            os.unlink(staging)


def child_env(root):
    """Explicit environment; no real Orbit credentials or provider variables."""
    root = Path(root).resolve()
    for name in ["home", "tmp"]:
        (root / name).mkdir(mode=0o700, exist_ok=True)
    return {"HOME": str(root / "home"), "TMPDIR": str(root / "tmp"),
            "PATH": os.defpath + ":/opt/homebrew/bin:/usr/local/bin",
            "LANG": "en_US.UTF-8", "LC_ALL": "C", "TZ": "UTC",
            "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
            "ORBIT_WORKSPACE": str(root), "ORBIT_ROOT": str(root),
            "ORBIT_REGISTRY_ROOT": str(root / "registry"),
            "ORBIT_BIN": str(HERE / "orbit_fixture.py")}
