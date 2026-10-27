"""`cloudcli.run`: one read-only `aws` or `gcloud` command with the plugin's own identities.

Read-only is enforced by the credentials: the user creates the plugin's
identities with read-only roles and stores them as declared secrets. The
command policy is a second layer, checked before any process starts:

1. **Denylist, first.** Commands that read secrets, data or tokens although
   their verb looks read-only (`DENY`), and the CLIs' own configuration.
2. **Allowlist.** `aws <service> <operation>` with a `describe-`, `list-` or
   `get-` operation (and `sts get-caller-identity`); `gcloud <group>… list`
   or `… describe`. Global flags only from `ALLOWED_GLOBALS`; output is
   forced to JSON. Anything else is `command_not_allowed`, naming the rule.

Each call writes the credentials to owner-only files in a fresh private
directory under `{{plugin_state}}/calls/`, points the CLI at them (and `HOME`
at the directory, so the user's `~/.aws` and gcloud configuration are never
read), runs the CLI with argv only and an allowlisted environment (STD-05@1
§R10, §R12), stops it after 60 seconds, keeps 1 MiB of its output, and
deletes the directory. The output is redacted with the shared redactor and
the call is audited.

With `callbacks` (Orbit's own callbacks), the only module that spawns a process.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import time
from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import audit, state
from .callbacks import supervise
from .envelope import Call, GatewayError, closed, required, typed
from .redact import Redactor

INPUT_KEYS = {"program", "args", "region", "project"}
PROGRAMS = ("aws", "gcloud")
TIMEOUT_SECONDS = 60.0
OUTPUT_LIMIT = 1024 * 1024
STDERR_RETURNED = 64 * 1024
MAX_ARGS = 64
MAX_ARG_LENGTH = 4096
# A call directory older than this was left by a backend that was killed
# before it could delete it; the next call removes it.
STALE_SECONDS = 600

AWS_KEY_ID = "aws_access_key_id"
AWS_SECRET_KEY = "aws_secret_access_key"
GCP_KEY = "gcp_service_account_json"
SECRETS = {"aws": (AWS_KEY_ID, AWS_SECRET_KEY), "gcloud": (GCP_KEY,)}

# The child's environment (STD-05@1 §R10): these non-credential names are
# copied from the backend's environment when set; everything else the child
# sees is set below (`_environment`). Nothing else of the backend's
# environment reaches it.
ENV_BASELINE = ("PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ")
# Names the plugin sets itself, per program.
ENV_SET = {
    "aws": (
        "HOME",
        "TMPDIR",
        "AWS_SHARED_CREDENTIALS_FILE",
        "AWS_CONFIG_FILE",
        "AWS_PAGER",
        "AWS_EC2_METADATA_DISABLED",
    ),
    "gcloud": ("HOME", "TMPDIR", "CLOUDSDK_CONFIG", "CLOUDSDK_CORE_DISABLE_PROMPTS"),
}
# gcloud runs on a Python of its choosing; this names it and is not a credential.
ENV_PASS = {"aws": (), "gcloud": ("CLOUDSDK_PYTHON",)}

REGION = re.compile(r"^[a-z0-9][a-z0-9-]{0,31}$")
PROJECT = re.compile(r"^[a-z0-9][a-z0-9.:-]{0,62}$")
WORD = re.compile(r"^[a-z0-9][a-z0-9-]*$")
CALL_DIR = re.compile(r"^call-[A-Za-z0-9_]+$")


@dataclass(frozen=True)
class Rule:
    """A denied command: these leading command words, optionally only with `flag`."""

    words: tuple[str, ...]
    reason: str
    flag: str | None = None

    def name(self, program: str) -> str:
        return " ".join((program, *self.words, *((self.flag,) if self.flag else ())))


DECRYPTED = "returns decrypted SecureString values"
DENY: dict[str, tuple[Rule, ...]] = {
    "aws": (
        Rule(("secretsmanager", "get-secret-value"), "returns a secret value"),
        Rule(("ssm", "get-parameter"), DECRYPTED, "--with-decryption"),
        Rule(("ssm", "get-parameters"), DECRYPTED, "--with-decryption"),
        Rule(("ssm", "get-parameters-by-path"), DECRYPTED, "--with-decryption"),
        Rule(("ssm", "get-parameter-history"), DECRYPTED, "--with-decryption"),
        Rule(("kms", "decrypt"), "decrypts data"),
        Rule(("s3api", "get-object"), "returns an object's data"),
        Rule(("s3api", "get-object-torrent"), "returns an object's data"),
        Rule(("lambda", "get-function"), "returns a download link to the function's code"),
        Rule(("ecr", "get-login-password"), "returns a registry password"),
        Rule(("ecr", "get-authorization-token"), "returns a registry token"),
        Rule(("ec2", "get-password-data"), "returns an instance's administrator password"),
        Rule(("rds", "generate-db-auth-token"), "mints a database token"),
        Rule(("eks", "get-token"), "mints a cluster token"),
        Rule(("sts", "get-session-token"), "mints temporary credentials"),
        Rule(("sts", "get-federation-token"), "mints temporary credentials"),
        Rule(("sso", "get-role-credentials"), "returns role credentials"),
        Rule(("codeartifact", "get-authorization-token"), "returns a repository token"),
        Rule(("redshift", "get-cluster-credentials"), "mints database credentials"),
        Rule(("redshift", "get-cluster-credentials-with-iam"), "mints database credentials"),
        Rule(("redshift-serverless", "get-credentials"), "mints database credentials"),
        Rule(("cognito-identity", "get-credentials-for-identity"), "returns identity credentials"),
        Rule(("cognito-identity", "get-open-id-token"), "returns an identity token"),
        Rule(("cognito-identity", "get-open-id-token-for-developer-identity"), "returns an identity token"),
        Rule(("dynamodb", "get-item"), "returns table data"),
        Rule(("kinesis", "get-records"), "returns stream data"),
        Rule(("glacier", "get-job-output"), "returns archive data"),
        Rule(("configure",), "reads or changes the CLI's own configuration"),
    ),
    "gcloud": (
        Rule(("secrets", "versions", "access"), "returns a secret value"),
        Rule(("auth", "print-access-token"), "returns an access token"),
        Rule(("auth", "print-identity-token"), "returns an identity token"),
        Rule(("auth", "application-default", "print-access-token"), "returns an access token"),
        Rule(("sql", "generate-login-token"), "mints a database token"),
        Rule(("container", "clusters", "get-credentials"), "writes cluster credentials"),
        Rule(("config",), "reads or changes the CLI's own configuration"),
    ),
}

# Every global flag each CLI knows, and the ones a call may set. A flag that
# abbreviates a global one counts as it (argparse accepts unique prefixes).
GLOBALS = {
    "aws": frozenset(
        {
            "--debug", "--endpoint-url", "--no-verify-ssl", "--no-paginate", "--output", "--query",
            "--profile", "--region", "--version", "--color", "--no-sign-request", "--ca-bundle",
            "--cli-read-timeout", "--cli-connect-timeout", "--cli-binary-format", "--no-cli-pager",
            "--cli-auto-prompt", "--no-cli-auto-prompt", "--cli-input-json", "--cli-input-yaml",
            "--generate-cli-skeleton", "--max-items", "--page-size", "--starting-token", "--help",
        }
    ),
    "gcloud": frozenset(
        {
            "--account", "--billing-project", "--configuration", "--flags-file", "--flatten",
            "--format", "--help", "--project", "--quiet", "--verbosity", "--version",
            "--impersonate-service-account", "--log-http", "--trace-token", "--user-output-enabled",
            "--no-user-output-enabled", "--access-token-file", "--filter", "--limit", "--page-size",
            "--sort-by", "--uri", "--async", "--universe-domain",
        }
    ),
}
ALLOWED_GLOBALS = {
    "aws": frozenset({"--region", "--query", "--max-items", "--page-size", "--starting-token"}),
    "gcloud": frozenset({"--filter", "--limit", "--page-size", "--sort-by"}),
}
FORCED = {"aws": ["--output", "json"], "gcloud": ["--format=json"]}

AWS_VERBS = ("describe-", "list-", "get-")
AWS_ALSO = {("sts", "get-caller-identity")}
# aws expands these parameter values into a file's (or, in v1, a URL's) contents.
AWS_PARAM_FILES = ("file://", "fileb://", "http://", "https://")
GCLOUD_TRACKS = {"alpha", "beta", "preview"}
GCLOUD_VERBS = ("list", "describe")
# Command names (not groups): one of these before `list` or `describe` would
# be the command, and `list`/`describe` merely its argument.
GCLOUD_COMMANDS = frozenset(
    {
        "access", "acknowledge", "ack", "activate-service-account", "add", "apply", "attach", "call",
        "cancel", "cat", "connect", "copy", "cp", "create", "delete", "deploy", "detach", "disable",
        "enable", "execute", "export", "failover", "import", "invoke", "login", "logout", "move", "mv",
        "patch", "promote", "publish", "pull", "purge", "reset", "resize", "restart", "restore",
        "resume", "revoke", "rm", "rollback", "rsync", "run", "scp", "seek", "set", "sign-blob",
        "sign-url", "ssh", "start", "stop", "submit", "suspend", "trigger", "undelete", "update",
    }
)
GCLOUD_COMMAND_PREFIXES = ("add-", "create-", "delete-", "generate-", "get-", "print-", "remove-", "set-", "update-")


# ---------------------------------------------------------------------------
# Policy
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Command:
    """A checked command: its argv after the program, and the words the policy read."""

    program: str
    argv: list[str]
    words: str


def refuse(rule: str, message: str) -> GatewayError:
    return GatewayError("command_not_allowed", f"{message} (rule: {rule})", detail={"rule": rule})


def _flag_name(token: str) -> str:
    return token.split("=", 1)[0]


def _globals(name: str, known: Iterable[str]) -> list[str]:
    """The global flags `name` may be: itself, or every flag it abbreviates."""
    known = sorted(known)
    if name in known:
        return [name]
    return [flag for flag in known if _same_flag(name, flag)]


def _same_flag(name: str, flag: str) -> bool:
    return name == flag or (len(name) > 2 and flag.startswith(name))


def _leading(args: list[str]) -> list[str]:
    words: list[str] = []
    for token in args:
        if token.startswith("-"):
            break
        words.append(token)
    return words


def _split(program: str, args: list[str]) -> tuple[list[str], list[str]]:
    """The leading command words, and the flag names that follow them."""
    words = _leading(args)
    if not words:
        raise refuse(
            f"{program} <command> first",
            f"put the {program} command words first, before any flag",
        )
    flags: list[str] = []
    for token in args[len(words) :]:
        if token == "--":
            raise refuse(f"{program} no --", "`--` (end of options) is not accepted")
        if re.match(r"^-[A-Za-z]", token):
            raise refuse(f"{program} no short flags", f"short flag {token!r} is not accepted; use its long form")
        if token.startswith("--"):
            flags.append(_flag_name(token))
    return words, flags


def _check_globals(program: str, flags: list[str]) -> None:
    for name in flags:
        # An abbreviation that could be a refused flag is refused.
        refused = [flag for flag in _globals(name, GLOBALS[program]) if flag not in ALLOWED_GLOBALS[program]]
        if refused:
            flag = refused[0]
            allowed = ", ".join(sorted(ALLOWED_GLOBALS[program]))
            if flag in ("--output", "--format"):
                why = "output is always JSON"
            elif flag == "--project":
                why = "use the `project` input"
            else:
                why = f"only {allowed} may be set"
            raise refuse(f"{program} global flags", f"{program} global flag {flag} is not accepted; {why}")


def check(program: str, args: list[str]) -> Command:
    """Check one command against the denylist, then the allowlist; refuse before anything runs."""
    if program == "aws":
        return _check_aws(args)
    return _check_gcloud(args)


def _check_aws(args: list[str]) -> Command:
    words, flags = _split("aws", args)
    for rule in DENY["aws"]:
        if tuple(words[: len(rule.words)]) == rule.words and (
            rule.flag is None or any(_same_flag(name, rule.flag) for name in flags)
        ):
            raise refuse(rule.name("aws"), f"`aws {' '.join(words[:2])}` {rule.reason}")
    shape = "aws <service> <describe-*|list-*|get-*>"
    if len(words) != 2 or not all(WORD.match(word) for word in words):
        raise refuse(shape, "aws takes exactly `<service> <operation>` before its flags")
    service, operation = words
    _check_globals("aws", flags)
    for token in args[2:]:
        value = token.split("=", 1)[1] if token.startswith("--") and "=" in token else token
        if value.lower().startswith(AWS_PARAM_FILES):
            raise refuse(
                "aws no file:// or URL values",
                "a parameter value starting with file://, fileb:// or a URL would be read by aws; pass the value itself",
            )
    if not operation.startswith(AWS_VERBS) and (service, operation) not in AWS_ALSO:
        raise refuse(shape, f"`aws {service} {operation}` is not a describe-, list- or get- operation")
    return Command("aws", [*args, *FORCED["aws"]], f"{service} {operation}")


def _check_gcloud(args: list[str]) -> Command:
    words, flags = _split("gcloud", args)
    core = list(words)
    while core and core[0] in GCLOUD_TRACKS:
        core.pop(0)
    for rule in DENY["gcloud"]:
        if tuple(core[: len(rule.words)]) == rule.words:
            raise refuse(rule.name("gcloud"), f"`gcloud {' '.join(rule.words)}` {rule.reason}")
    shape = "gcloud <group>... <list|describe>"
    verb = next((index for index, word in enumerate(core) if word in GCLOUD_VERBS), None)
    if verb is None or verb == 0:
        raise refuse(shape, "gcloud takes a command ending in `list` or `describe`")
    for index, group in enumerate(core[:verb]):
        if not WORD.match(group):
            raise refuse(shape, f"{group!r} is not a gcloud command group")
        if index > 0 and (group in GCLOUD_COMMANDS or group.startswith(GCLOUD_COMMAND_PREFIXES)):
            raise refuse(shape, f"`{group}` is a gcloud command, not a group; only list and describe run")
    _check_globals("gcloud", flags)
    for name in flags:
        if name.endswith("-file") or "-from-file" in name:
            raise refuse("gcloud no file flags", f"{name} reads a local file and is not accepted")
    command = " ".join(words[: len(words) - len(core) + verb + 1])
    return Command("gcloud", [*args, *FORCED["gcloud"]], command)


# ---------------------------------------------------------------------------
# Credentials
# ---------------------------------------------------------------------------


def _service_account(value: str) -> dict[str, Any] | None:
    try:
        key = json.loads(value)
    except (ValueError, RecursionError):
        return None
    if not isinstance(key, dict) or key.get("type") != "service_account":
        return None
    if not isinstance(key.get("private_key"), str) or not isinstance(key.get("client_email"), str):
        return None
    return key


def redactor_for(call: Call) -> Redactor:
    """The redactor for one call: every delivered secret, and the parts of the
    service-account key a CLI could echo on its own."""
    secrets = call.secrets()
    values = list(secrets.values())
    key = _service_account(secrets.get(GCP_KEY, ""))
    if key is not None:
        for field in ("private_key", "private_key_id"):
            part = key.get(field)
            if isinstance(part, str) and part:
                values.extend((part, json.dumps(part)[1:-1]))
    return Redactor(values)


def _credentials(program: str, call: Call) -> dict[str, str]:
    secrets = call.secrets()
    missing = [name for name in SECRETS[program] if name not in secrets]
    if missing:
        names = ", ".join(missing)
        raise GatewayError(
            "secret_unset",
            f"{program} needs the plugin's own read-only identity: ask the user to set {names} "
            f"(`orbit plugin secret set cloudcli <name>`, value on stdin)",
        )
    wanted = {name: secrets[name] for name in SECRETS[program]}
    if program == "aws":
        if not re.fullmatch(r"[A-Z0-9]{16,128}", wanted[AWS_KEY_ID]) or not re.fullmatch(
            r"[A-Za-z0-9/+=]{16,128}", wanted[AWS_SECRET_KEY]
        ):
            raise GatewayError("invalid_secret", "aws_access_key_id or aws_secret_access_key is not an AWS access key")
    elif _service_account(wanted[GCP_KEY]) is None:
        raise GatewayError("invalid_secret", "gcp_service_account_json is not a Google Cloud service-account JSON key")
    return wanted


def _write_private(path: Path, text: str) -> None:
    """Create one owner-only credential file; it never exists with other bits set."""
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        os.fchmod(fd, 0o600)
        view = memoryview(text.encode("utf-8"))
        while view:
            view = view[os.write(fd, view) :]
    finally:
        os.close(fd)


def _private_dir(directory: Path) -> Path:
    state.ensure_dir(directory)
    _sweep(directory)
    # mkdtemp creates the directory 0700 under a name nobody else can predict.
    path = Path(tempfile.mkdtemp(prefix="call-", dir=directory))
    os.chmod(path, 0o700)
    return path


def _sweep(directory: Path) -> None:
    """Remove call directories a killed backend left behind."""
    cutoff = time.time() - STALE_SECONDS
    for entry in directory.iterdir():
        try:
            info = entry.lstat()
        except FileNotFoundError:
            continue
        if CALL_DIR.match(entry.name) and entry.is_dir() and not entry.is_symlink() and info.st_mtime < cutoff:
            shutil.rmtree(entry, ignore_errors=True)


def _prepare(program: str, private: Path, secrets: dict[str, str], region: str | None, project: str | None) -> dict[str, str]:
    """Write the program's credential files; return the variables that point at them."""
    tmp = private / "tmp"
    os.mkdir(tmp, 0o700)
    env = {"HOME": str(private), "TMPDIR": str(tmp)}
    if program == "aws":
        aws = private / "aws"
        os.mkdir(aws, 0o700)
        _write_private(
            aws / "credentials",
            f"[default]\naws_access_key_id = {secrets[AWS_KEY_ID]}\naws_secret_access_key = {secrets[AWS_SECRET_KEY]}\n",
        )
        config = "[default]\noutput = json\ncli_pager =\ncli_auto_prompt = off\n"
        if region:
            config += f"region = {region}\n"
        _write_private(aws / "config", config)
        env.update(
            AWS_SHARED_CREDENTIALS_FILE=str(aws / "credentials"),
            AWS_CONFIG_FILE=str(aws / "config"),
            AWS_PAGER="",
            AWS_EC2_METADATA_DISABLED="true",
        )
    else:
        gcloud = private / "gcloud"
        os.mkdir(gcloud, 0o700)
        os.mkdir(gcloud / "configurations", 0o700)
        key = gcloud / "service-account.json"
        _write_private(key, secrets[GCP_KEY])
        config = (
            "[core]\ndisable_usage_reporting = true\n"
            + (f"project = {project}\n" if project else "")
            + f"[auth]\ncredential_file_override = {key}\n"
            + "[component_manager]\ndisable_update_check = true\n"
            + (f"[compute]\nregion = {region}\n" if region else "")
        )
        _write_private(gcloud / "configurations" / "config_default", config)
        _write_private(gcloud / "active_config", "default")
        env.update(CLOUDSDK_CONFIG=str(gcloud), CLOUDSDK_CORE_DISABLE_PROMPTS="1")
    return env


def _environment(program: str, environ: Mapping[str, str], pointers: dict[str, str]) -> dict[str, str]:
    env = {name: environ[name] for name in (*ENV_BASELINE, *ENV_PASS[program]) if environ.get(name)}
    env.update(pointers)
    return env


# ---------------------------------------------------------------------------
# The tool
# ---------------------------------------------------------------------------


def tool_run(call: Call) -> dict[str, Any]:
    root = call.state_root()
    redactor = redactor_for(call)
    record = audit.Record(tool=call.tool, caller=call.caller())
    started = time.monotonic()
    try:
        return _run(call, root, record, redactor)
    except GatewayError as error:
        record.outcome = error.code
        raise
    except Exception:
        record.outcome = "backend_error"
        raise
    finally:
        record.duration_ms = int((time.monotonic() - started) * 1000)
        audit.append(root, record, redactor)


def _input(call: Call, record: audit.Record) -> tuple[str, list[str], str | None, str | None]:
    raw = closed(call.input, INPUT_KEYS, "input")
    program = required(raw, "program", str, "input")
    args = required(raw, "args", list, "input")
    region = typed(raw, "region", str, "input", None)
    project = typed(raw, "project", str, "input", None)
    record.connection = program if program in PROGRAMS else None
    record.query = {key: value for key, value in (("args", args), ("region", region), ("project", project)) if value is not None}
    if program not in PROGRAMS:
        raise GatewayError("invalid_argument", f"`input.program` must be one of {', '.join(PROGRAMS)}")
    if not 1 <= len(args) <= MAX_ARGS:
        raise GatewayError("invalid_argument", f"`input.args` takes 1 to {MAX_ARGS} arguments")
    for arg in args:
        if not isinstance(arg, str) or not 1 <= len(arg) <= MAX_ARG_LENGTH:
            raise GatewayError("invalid_argument", f"each of `input.args` is a string of 1 to {MAX_ARG_LENGTH} characters")
        if any(ord(char) < 0x20 and char not in "\t\n" or ord(char) == 0x7F for char in arg):
            raise GatewayError("invalid_argument", "`input.args` may not hold control characters")
    if region is not None and not REGION.match(region):
        raise GatewayError("invalid_argument", "`input.region` is not a region name")
    if project is not None:
        if program != "gcloud":
            raise GatewayError("invalid_argument", "`input.project` applies to gcloud only")
        if not PROJECT.match(project):
            raise GatewayError("invalid_argument", "`input.project` is not a project id")
    return program, args, region, project


def _run(call: Call, root: Path, record: audit.Record, redactor: Redactor) -> dict[str, Any]:
    program, args, region, project = _input(call, record)
    record.path = " ".join(_leading(args)) or None
    command = check(program, args)
    record.path = command.words
    secrets = _credentials(program, call)
    executable = shutil.which(program, path=call.environ.get("PATH") or os.defpath)
    if executable is None:
        raise GatewayError("program_unavailable", f"{program} is not installed or not on PATH")
    private = _private_dir(root / "calls")
    try:
        env = _environment(program, call.environ, _prepare(program, private, secrets, region, project))
        try:
            child = subprocess.Popen(
                [executable, *command.argv],
                cwd=private,
                env=env,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                start_new_session=True,
            )
        except OSError as error:
            raise GatewayError("program_unavailable", f"could not run {program}: {error.strerror or error}") from None
        status, stdout, stderr, overflow = supervise(child, TIMEOUT_SECONDS, OUTPUT_LIMIT)
    finally:
        shutil.rmtree(private, ignore_errors=True)
    record.status = status
    record.bytes = len(stdout)
    text = redactor.text(stdout.decode("utf-8", "replace"))
    record.body_sha256 = hashlib.sha256(text.encode("utf-8")).hexdigest()
    errors = stderr.decode("utf-8", "replace")[:STDERR_RETURNED]
    if status is None:
        raise GatewayError(
            "timeout",
            f"{program} {command.words} did not finish within {TIMEOUT_SECONDS:.0f} s and was stopped",
            retryable=True,
        )
    if status != 0:
        raise GatewayError(
            "command_failed",
            f"{program} {command.words} exited {status}",
            detail={"exit_code": status, "stderr": errors},
        )
    truncated = "stdout" in overflow
    output: Any = None
    shown: str | None = None
    if text.strip() and not truncated:
        try:
            output = json.loads(text)
        except ValueError:
            shown = text
    elif text.strip():
        shown = text
    return {
        "program": program,
        "command": command.words,
        "output": output,
        "text": shown,
        "bytes": len(stdout),
        "truncated": truncated,
        "stderr": errors,
    }
