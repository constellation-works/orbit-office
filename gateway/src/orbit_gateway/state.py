"""Owner-only state under `{{plugin_state}}` (STD-05@1 §R8, §R9).

Directories are created `0700` and files `0600`, set explicitly at creation
rather than left to the umask. State that is loaded is checked first: not a
symlink and owned by the current user; group or world bits on it are removed.
Every text this module writes goes through the call's redactor.
"""

from __future__ import annotations

import contextlib
import fcntl
import os
import stat
import tempfile
from collections.abc import Iterator
from pathlib import Path

from .envelope import GatewayError
from .redact import Redactor

DIR_MODE = 0o700
FILE_MODE = 0o600


def _check(path: Path, want_dir: bool) -> bool:
    """True when `path` exists and is trusted; refuse it otherwise."""
    try:
        info = os.lstat(path)
    except FileNotFoundError:
        return False
    if stat.S_ISLNK(info.st_mode):
        raise GatewayError("state_untrusted", f"{path} is a symlink; refusing to use it")
    if want_dir != stat.S_ISDIR(info.st_mode):
        kind = "directory" if want_dir else "regular file"
        raise GatewayError("state_untrusted", f"{path} is not a {kind}; refusing to use it")
    if info.st_uid != os.geteuid():
        raise GatewayError("state_untrusted", f"{path} is not owned by the current user; refusing to use it")
    if info.st_mode & 0o077:
        os.chmod(path, DIR_MODE if want_dir else FILE_MODE)  # lstat above: not a symlink
    return True


def ensure_dir(path: Path) -> Path:
    """Create `path` (and missing parents below an existing ancestor) owner-only."""
    if _check(path, want_dir=True):
        return path
    if not path.parent.exists():
        ensure_dir(path.parent)
    try:
        os.mkdir(path, DIR_MODE)
    except FileExistsError:
        # Created by a concurrent call: check it like any existing directory.
        _check(path, want_dir=True)
        return path
    os.chmod(path, DIR_MODE)  # mkdir's mode is filtered by the umask
    return path


def dir_exists(path: Path) -> bool:
    """True when the state directory exists (and is trusted)."""
    return _check(path, want_dir=True)


def read_text(path: Path) -> str | None:
    """A state file's text, or None when it (or its directory) does not exist."""
    if not _check(path.parent, want_dir=True) or not _check(path, want_dir=False):
        return None
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    with os.fdopen(fd, "r", encoding="utf-8") as handle:
        return handle.read()


def store_text(path: Path, text: str, redactor: Redactor) -> None:
    """Replace a state file atomically with redacted text, mode 0600."""
    ensure_dir(path.parent)
    _check(path, want_dir=False)
    data = redactor.text(text).encode("utf-8")
    fd, temp = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        os.fchmod(fd, FILE_MODE)
        with os.fdopen(fd, "wb") as handle:
            handle.write(data)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp, path)
    except BaseException:
        with contextlib.suppress(FileNotFoundError):
            os.unlink(temp)
        raise


def append_line(path: Path, line: str, redactor: Redactor) -> None:
    """Append one redacted line to an append-only file, mode 0600, under its lock."""
    ensure_dir(path.parent)
    _check(path, want_dir=False)
    data = (redactor.text(line).replace("\n", " ") + "\n").encode("utf-8")
    fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_CREAT | os.O_NOFOLLOW, FILE_MODE)
    try:
        os.fchmod(fd, FILE_MODE)
        fcntl.flock(fd, fcntl.LOCK_EX)
        view = memoryview(data)
        while view:
            view = view[os.write(fd, view) :]
    finally:
        os.close(fd)


@contextlib.contextmanager
def locked(path: Path) -> Iterator[None]:
    """Hold an exclusive advisory lock on `path` (a 0600 lock file) for a read-modify-write."""
    ensure_dir(path.parent)
    _check(path, want_dir=False)
    fd = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW, FILE_MODE)
    try:
        os.fchmod(fd, FILE_MODE)
        fcntl.flock(fd, fcntl.LOCK_EX)
        yield
    finally:
        os.close(fd)
