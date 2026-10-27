"""Small, bounded helpers for the synthetic demo, never host-state discovery."""
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
FAKE_TOKEN = "SIMULATED-acme-token-not-a-credential-0123456789"


def run(argv, *, env=None, cwd=None, input=None, timeout=120):
    result = subprocess.run([str(v) for v in argv], input=input, text=True,
                            capture_output=True, env=env, cwd=cwd, timeout=timeout,
                            check=False)
    if result.returncode:
        # Command input and remote responses can contain tokens; don't echo them.
        raise RuntimeError(f"{Path(argv[0]).name} exited {result.returncode}")
    return result.stdout


def write_json(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    with path.open("w", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write("\n")


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
