"""cloudcli.run through the real exported backend, with stub `aws` and `gcloud`
that record what they were given."""

from __future__ import annotations

import ast
import json
import os
import stat
import subprocess
import sys
import time
from pathlib import Path

from tests.support import (
    CALL_TIMEOUT,
    AWS_KEY_ID,
    AWS_SECRET,
    GCP_KEY,
    GCP_PRIVATE_KEY,
    GCP_PRIVATE_KEY_ID,
    CloudCase,
    Tripwire,
)

from orbit_gateway import cloud
from tests.test_backend import OLD_PYTHON

# Injected by macOS into every process environment; never passed by the plugin.
OS_INJECTED_ENV = {"__CF_USER_TEXT_ENCODING"}

# Every command the spec's denylist names, and the rule each refusal names.
SPEC_DENYLIST = [
    ("aws", ["secretsmanager", "get-secret-value", "--secret-id", "prod/db"], "aws secretsmanager get-secret-value"),
    ("aws", ["ssm", "get-parameter", "--name", "/prod/db", "--with-decryption"], "aws ssm get-parameter --with-decryption"),
    ("aws", ["ssm", "get-parameters", "--names", "a", "b", "--with-decryption"], "aws ssm get-parameters --with-decryption"),
    ("aws", ["kms", "decrypt", "--ciphertext-blob", "AQIDAHh"], "aws kms decrypt"),
    ("aws", ["s3api", "get-object", "--bucket", "b", "--key", "k", "out.bin"], "aws s3api get-object"),
    ("aws", ["lambda", "get-function", "--function-name", "f"], "aws lambda get-function"),
    ("aws", ["ecr", "get-login-password"], "aws ecr get-login-password"),
    ("aws", ["ec2", "get-password-data", "--instance-id", "i-0123456789abcdef0"], "aws ec2 get-password-data"),
    ("aws", ["rds", "generate-db-auth-token", "--hostname", "db", "--port", "5432", "--username", "u"], "aws rds generate-db-auth-token"),
    ("aws", ["eks", "get-token", "--cluster-name", "c"], "aws eks get-token"),
    ("gcloud", ["secrets", "versions", "access", "latest", "--secret", "db"], "gcloud secrets versions access"),
    ("gcloud", ["auth", "print-access-token"], "gcloud auth print-access-token"),
    ("gcloud", ["auth", "print-identity-token"], "gcloud auth print-identity-token"),
    ("gcloud", ["sql", "generate-login-token"], "gcloud sql generate-login-token"),
    ("gcloud", ["container", "clusters", "get-credentials", "c", "--region", "europe-west1"], "gcloud container clusters get-credentials"),
]

AWS_SHAPE = "aws <service> <describe-*|list-*|get-*>"
GCLOUD_SHAPE = "gcloud <group>... <list|describe>"


class Policy(CloudCase):
    def refuse(self, program: str, args: list[str], rule: str) -> None:
        error = self.refused("command_not_allowed", "run", {"program": program, "args": args})
        self.assertEqual(error["detail"], {"rule": rule}, error)
        self.assertIn(rule, error["message"])

    def test_every_spec_denylisted_command_is_refused_naming_its_rule_before_anything_runs(self) -> None:
        for program, args, rule in SPEC_DENYLIST:
            with self.subTest(rule=rule):
                self.refuse(program, args, rule)
        self.assertEqual(self.cli.runs(), [], "no CLI ran")
        self.assertFalse((self.state / "calls").exists(), "no credentials were written")

    def test_the_denylist_sees_through_spelling(self) -> None:
        # An abbreviated flag, `--flag=value` and a release track still match.
        self.refuse("aws", ["ssm", "get-parameter", "--name", "x", "--with-decrypt"], "aws ssm get-parameter --with-decryption")
        self.refuse("aws", ["ssm", "get-parameters-by-path", "--path", "/", "--with-decryption"], "aws ssm get-parameters-by-path --with-decryption")
        self.refuse("gcloud", ["beta", "secrets", "versions", "access", "1", "--secret=db"], "gcloud secrets versions access")
        self.refuse("aws", ["sts", "get-session-token"], "aws sts get-session-token")
        self.refuse("aws", ["configure", "list-profiles"], "aws configure")
        self.refuse("gcloud", ["config", "list"], "gcloud config")
        self.assertEqual(self.cli.runs(), [])

    def test_commands_that_are_not_describe_list_or_get_are_refused(self) -> None:
        for args in (
            ["ec2", "run-instances", "--image-id", "ami-1"],
            ["ec2", "terminate-instances", "--instance-ids", "i-1"],
            ["s3", "ls"],
            ["iam", "create-access-key"],
            ["ec2", "describe-instances", "extra"],
            ["ec2"],
        ):
            with self.subTest(args=args):
                self.refuse("aws", args, AWS_SHAPE)
        for args in (
            ["compute", "instances", "create", "vm"],
            ["compute", "instances", "delete", "list"],
            ["projects", "get-iam-policy", "list"],
            ["run", "deploy", "describe"],
            ["projects", "add-iam-policy-binding", "p"],
            ["list"],
        ):
            with self.subTest(args=args):
                self.refuse("gcloud", args, GCLOUD_SHAPE)
        self.assertEqual(self.cli.runs(), [])

    def test_refused_flags(self) -> None:
        cases = [
            ("aws", ["ec2", "describe-instances", "--endpoint-url", "https://attacker.example"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--endpoint=https://attacker.example"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--profile", "admin"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--output", "text"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--debug"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--no-verify-ssl"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--cli-input-json", "{}"], "aws global flags"),
            ("aws", ["ec2", "describe-instances", "--filters", "file:///etc/passwd"], "aws no file:// or URL values"),
            ("aws", ["ec2", "describe-instances", "--filters=fileb://key.pem"], "aws no file:// or URL values"),
            ("aws", ["--region", "eu-west-1", "ec2", "describe-instances"], "aws <command> first"),
            ("aws", ["ec2", "describe-instances", "-h"], "aws no short flags"),
            ("aws", ["ec2", "describe-instances", "--", "--debug"], "aws no --"),
            ("gcloud", ["compute", "instances", "list", "--format", "yaml"], "gcloud global flags"),
            ("gcloud", ["compute", "instances", "list", "--impersonate-service-account=admin@p.iam.gserviceaccount.com"], "gcloud global flags"),
            ("gcloud", ["compute", "instances", "list", "--account", "me@example.com"], "gcloud global flags"),
            ("gcloud", ["compute", "instances", "list", "--log-http"], "gcloud global flags"),
            ("gcloud", ["compute", "instances", "list", "--flags-file", "f.yaml"], "gcloud global flags"),
            ("gcloud", ["compute", "instances", "list", "--project", "p"], "gcloud global flags"),
            ("gcloud", ["compute", "instances", "list", "-q"], "gcloud no short flags"),
            ("gcloud", ["compute", "instances", "describe", "vm", "--metadata-from-file", "k=f"], "gcloud no file flags"),
        ]
        for program, args, rule in cases:
            with self.subTest(args=args):
                self.refuse(program, args, rule)
        self.assertEqual(self.cli.runs(), [])

    def test_refused_input(self) -> None:
        self.refused("invalid_argument", "run", {"program": "kubectl", "args": ["get", "pods"]})
        self.refused("invalid_argument", "run", {"program": "aws", "args": "ec2 describe-instances"})
        self.refused("invalid_argument", "run", {"program": "aws", "args": ["ec2", "describe-instances"], "shell": True})
        self.refused("invalid_argument", "run", {"program": "aws", "args": ["ec2", "describe-instances"], "project": "p"})
        self.refused("invalid_argument", "run", {"program": "aws", "args": ["ec2", "describe-instances"], "region": "eu\nwest"})
        self.refused("invalid_argument", "run", {"program": "aws", "args": ["ec2", "describe-instances", "a\x00b"]})
        self.assertEqual(self.cli.runs(), [])

    def test_allowed_commands_run_with_json_output_forced(self) -> None:
        self.cli.reply("aws", stdout='{"Reservations": []}')
        out = self.ok("run", {"program": "aws", "args": ["ec2", "describe-instances", "--max-items", "5", "--query", "Reservations[]"]})
        self.assertEqual(out["output"], {"Reservations": []})
        self.assertEqual((out["command"], out["truncated"], out["text"]), ("ec2 describe-instances", False, None))
        self.ok("run", {"program": "aws", "args": ["sts", "get-caller-identity"]})
        self.ok("run", {"program": "aws", "args": ["ssm", "get-parameter", "--name", "/app/flag"]})
        self.cli.reply("gcloud", stdout='[{"name": "vm-1"}]')
        out = self.ok("run", {"program": "gcloud", "args": ["beta", "compute", "instances", "describe", "vm-1", "--zone", "europe-west1-b"]})
        self.assertEqual((out["command"], out["output"]), ("beta compute instances describe", [{"name": "vm-1"}]))
        self.ok("run", {"program": "gcloud", "args": ["compute", "instances", "list", "--filter=status:RUNNING", "--limit", "3"]})
        argv = [run["argv"] for run in self.cli.runs()]
        self.assertEqual(
            argv,
            [
                ["ec2", "describe-instances", "--max-items", "5", "--query", "Reservations[]", "--output", "json"],
                ["sts", "get-caller-identity", "--output", "json"],
                ["ssm", "get-parameter", "--name", "/app/flag", "--output", "json"],
                ["beta", "compute", "instances", "describe", "vm-1", "--zone", "europe-west1-b", "--format=json"],
                ["compute", "instances", "list", "--filter=status:RUNNING", "--limit", "3", "--format=json"],
            ],
        )

    def test_a_failing_command_is_an_error_with_its_stderr(self) -> None:
        self.cli.reply("aws", stderr="An error occurred (AccessDenied)", exit=254)
        error = self.refused("command_failed", "run", {"program": "aws", "args": ["iam", "list-users"]})
        self.assertEqual(error["detail"], {"exit_code": 254, "stderr": "An error occurred (AccessDenied)"})

    def test_an_unset_identity_is_refused_before_anything_runs(self) -> None:
        error = self.refused("secret_unset", "run", {"program": "aws", "args": ["sts", "get-caller-identity"]}, secrets={"gcp_service_account_json": GCP_KEY})
        self.assertIn("aws_access_key_id", error["message"])
        self.refused("invalid_secret", "run", {"program": "gcloud", "args": ["projects", "list"]}, secrets={"gcp_service_account_json": "{}"})
        self.assertEqual(self.cli.runs(), [])

    def test_a_missing_program_is_reported(self) -> None:
        # An empty PATH directory, not /usr/bin:/bin: CI runners preinstall
        # gcloud there, so the "missing" program must be missing by
        # construction, not by host inventory.
        empty = self.state / "empty-path"
        empty.mkdir(parents=True, exist_ok=True)
        self.path = str(empty)
        self.refused("program_unavailable", "run", {"program": "gcloud", "args": ["projects", "list"]})


class Isolation(CloudCase):
    def setUp(self) -> None:
        super().setUp()
        # The user's own configuration, which must never be opened.
        self.aws_tripwires = [Tripwire(self, self.home / ".aws" / name) for name in ("credentials", "config")]
        self.gcloud_tripwire = Tripwire(self, self.home / ".config" / "gcloud" / "active_config")
        self.ambient = {
            "AWS_ACCESS_KEY_ID": "AKIAUSERSOWNKEY00000",
            "AWS_SECRET_ACCESS_KEY": "users-own-secret",
            "AWS_PROFILE": "admin",
            "AWS_SHARED_CREDENTIALS_FILE": str(self.home / ".aws" / "credentials"),
            "CLOUDSDK_CONFIG": str(self.home / ".config" / "gcloud"),
            "GOOGLE_APPLICATION_CREDENTIALS": str(self.home / "adc.json"),
            "CLOUDSDK_PYTHON": "/usr/bin/python3",
        }

    def run_once(self, program: str, args: list[str], **extra: str) -> dict:
        self.ok("run", {"program": program, "args": args, **extra}, env=self.ambient)
        runs = self.cli.runs()
        self.assertEqual(len(runs), 1)
        return runs[0]

    def assert_private(self, run: dict, secrets: list[str]) -> Path:
        private = Path(run["env"]["HOME"])
        self.assertEqual(private.parent, self.state / "calls", "HOME is the call's private directory")
        self.assertEqual(run["cwd"], str(private))
        self.assertEqual(run["home_mode"], 0o700)
        for path, info in run["files"].items():
            self.assertTrue(Path(path).is_relative_to(private), path)
            self.assertEqual((info["mode"], info["dir_mode"]), (0o600, 0o700), path)
        for secret in secrets:
            self.assertFalse(any(secret in arg for arg in run["argv"]), "a secret in argv")
            self.assertFalse(any(secret in value for value in run["env"].values()), "a secret in the environment")
        self.assertFalse(private.exists(), "the private directory is gone after the call")
        self.assertEqual(list((self.state / "calls").iterdir()), [])
        return private

    def test_aws_gets_its_credentials_only_through_private_files(self) -> None:
        run = self.run_once("aws", ["ec2", "describe-regions"], region="eu-west-1")
        private = self.assert_private(run, [AWS_KEY_ID, AWS_SECRET])
        credentials = run["files"][run["env"]["AWS_SHARED_CREDENTIALS_FILE"]]["text"]
        self.assertEqual(
            credentials,
            f"[default]\naws_access_key_id = {AWS_KEY_ID}\naws_secret_access_key = {AWS_SECRET}\n",
        )
        config = run["files"][run["env"]["AWS_CONFIG_FILE"]]["text"]
        self.assertIn("region = eu-west-1\n", config)
        self.assertEqual(
            set(run["env"]) - OS_INJECTED_ENV,
            {"PATH", "LANG", "HOME", "TMPDIR", "AWS_SHARED_CREDENTIALS_FILE", "AWS_CONFIG_FILE", "AWS_PAGER", "AWS_EC2_METADATA_DISABLED"},
        )
        self.assertEqual(run["env"]["AWS_SHARED_CREDENTIALS_FILE"], str(private / "aws" / "credentials"))
        self.assertEqual((run["env"]["AWS_PAGER"], run["env"]["AWS_EC2_METADATA_DISABLED"]), ("", "true"))
        self.assertFalse(any(tripwire.tripped() for tripwire in self.aws_tripwires), "HOME's .aws was opened")

    def test_gcloud_gets_its_key_only_through_private_files(self) -> None:
        run = self.run_once("gcloud", ["projects", "list"], project="fixture-project", region="europe-west1")
        private = self.assert_private(run, [GCP_KEY, GCP_PRIVATE_KEY, GCP_PRIVATE_KEY_ID])
        gcloud = private / "gcloud"
        self.assertEqual(run["env"]["CLOUDSDK_CONFIG"], str(gcloud))
        self.assertEqual(run["files"][str(gcloud / "service-account.json")]["text"], GCP_KEY)
        config = run["files"][str(gcloud / "configurations" / "config_default")]["text"]
        self.assertIn(f"credential_file_override = {gcloud / 'service-account.json'}\n", config)
        self.assertIn("project = fixture-project\n", config)
        self.assertIn("region = europe-west1\n", config)
        self.assertEqual(
            set(run["env"]) - OS_INJECTED_ENV,
            {"PATH", "LANG", "HOME", "TMPDIR", "CLOUDSDK_CONFIG", "CLOUDSDK_CORE_DISABLE_PROMPTS", "CLOUDSDK_PYTHON"},
        )
        self.assertFalse(self.gcloud_tripwire.tripped(), "the user's gcloud configuration was opened")
        self.assertFalse(any(tripwire.tripped() for tripwire in self.aws_tripwires), "HOME's .aws was opened")

    def test_the_tripwire_trips_when_opened(self) -> None:
        # The check above is only worth something if a read is seen.
        tripwire = Tripwire(self, self.root / "probe")
        fd = os.open(tripwire.path, os.O_RDONLY)
        os.close(fd)
        self.assertTrue(tripwire.tripped())

    def test_a_failed_call_removes_its_directory_too(self) -> None:
        self.cli.reply("aws", exit=1, stderr="boom")
        self.refused("command_failed", "run", {"program": "aws", "args": ["ec2", "describe-regions"]})
        self.assertFalse(Path(self.cli.runs()[0]["env"]["HOME"]).exists())

    def test_a_directory_left_by_a_killed_backend_is_swept(self) -> None:
        calls = self.state / "calls"
        calls.mkdir(parents=True, mode=0o700)
        stale = calls / "call-leftover"
        stale.mkdir(mode=0o700)
        (stale / "credentials").write_text("left behind")
        old = time.time() - cloud.STALE_SECONDS - 60
        os.utime(stale, (old, old))
        fresh = calls / "call-running"
        fresh.mkdir(mode=0o700)
        self.ok("run", {"program": "aws", "args": ["ec2", "describe-regions"]})
        self.assertEqual(sorted(path.name for path in calls.iterdir()), ["call-running"])


# Runs the real backend with only the CLI deadline shortened, so a stub that
# sleeps far past 60 s is stopped without the test waiting a minute.
SHORT_DEADLINE = """
import os, runpy, sys
backend = sys.argv[1]
sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(backend)), "lib"))
from orbit_gateway import cloud
cloud.TIMEOUT_SECONDS = 1.0
sys.argv = [backend]
runpy.run_path(backend, run_name="__main__")
"""


class Bounds(CloudCase):
    def test_the_deadline_is_sixty_seconds(self) -> None:
        self.assertEqual(cloud.TIMEOUT_SECONDS, 60.0)

    def test_a_cli_that_runs_past_the_deadline_is_killed(self) -> None:
        orphan = self.root / "orphan.pid"
        self.cli.reply("aws", sleep=120, orphan=str(orphan))
        started = time.monotonic()
        answer = self.call("run", {"program": "aws", "args": ["ec2", "describe-instances"]}, prelude=SHORT_DEADLINE)
        self.assertLess(time.monotonic() - started, 30)
        self.assertEqual(answer["error"]["code"], "timeout", answer)
        self.assertIn("did not finish within 1 s and was stopped", answer["error"]["message"])
        run = self.cli.runs()[0]
        for pid in (run["pid"], int(orphan.read_text())):
            with self.assertRaises(ProcessLookupError, msg=f"process {pid} survived"):
                os.kill(pid, 0)
        self.assertFalse(Path(run["env"]["HOME"]).exists())
        record = self.audit_lines()[-1]
        self.assertEqual((record["outcome"], record["status"]), ("timeout", None))

    def test_output_past_one_mebibyte_is_truncated(self) -> None:
        self.cli.reply("aws", flood=cloud.OUTPUT_LIMIT + 4096, stdout="")
        out = self.ok("run", {"program": "aws", "args": ["ec2", "describe-instances"]})
        self.assertEqual((out["truncated"], out["bytes"], out["output"]), (True, 1024 * 1024, None))
        self.assertEqual(out["text"], "x" * (1024 * 1024))
        self.assertEqual(self.audit_lines()[-1]["bytes"], 1024 * 1024)

    def test_output_at_the_cap_is_whole(self) -> None:
        body = json.dumps({"pad": "y" * (cloud.OUTPUT_LIMIT - 20)})
        self.assertLessEqual(len(body), cloud.OUTPUT_LIMIT)
        self.cli.reply("aws", stdout=body)
        out = self.ok("run", {"program": "aws", "args": ["ec2", "describe-instances"]})
        self.assertFalse(out["truncated"])
        self.assertEqual(out["output"], json.loads(body))


class Redaction(CloudCase):
    def test_secret_values_and_credential_shapes_come_back_redacted(self) -> None:
        unknown_token = "ghp_" + "A1b2C3d4E5f6G7h8I9j0K1l2M3n4"
        self.cli.reply(
            "aws",
            stdout=json.dumps(
                {
                    "Echo": AWS_SECRET,
                    "AccessKeyId": AWS_KEY_ID,
                    "Other": "AKIAOTHERKEY00000000",
                    "Token": unknown_token,
                    "Header": "Bearer abc123def456ghi789jkl",
                    "Key": GCP_PRIVATE_KEY,
                    AWS_SECRET: "as a key",
                }
            ),
        )
        out = self.ok("run", {"program": "aws", "args": ["sts", "get-caller-identity"]})
        self.assertEqual(
            out["output"],
            {
                "Echo": "[secret]",
                "AccessKeyId": "[secret]",
                "Other": "[secret]",
                "Token": "[secret]",
                "Header": "Bearer [secret]",
                "Key": "[secret]",
                "[secret]": "as a key",
            },
        )
        for value in (AWS_SECRET, AWS_KEY_ID, unknown_token, GCP_PRIVATE_KEY_ID):
            self.assertNotIn(value, self.last_stdout)

    def test_text_output_and_stderr_are_redacted(self) -> None:
        self.cli.reply("gcloud", stdout=f"not json: {GCP_PRIVATE_KEY_ID}", stderr="")
        out = self.ok("run", {"program": "gcloud", "args": ["projects", "list"]})
        self.assertEqual(out["text"], "not json: [secret]")
        self.cli.reply("aws", stderr=f"denied for {AWS_KEY_ID} / {AWS_SECRET}", exit=255)
        error = self.refused("command_failed", "run", {"program": "aws", "args": ["iam", "list-users"]})
        self.assertEqual(error["detail"]["stderr"], "denied for [secret] / [secret]")
        self.assertNotIn(AWS_SECRET, self.last_stdout)

    def test_each_call_is_audited_without_secrets(self) -> None:
        self.cli.reply("aws", stdout='{"Regions": []}')
        self.ok("run", {"program": "aws", "args": ["ec2", "describe-regions", "--query", AWS_SECRET], "region": "eu-west-1"})
        self.refused("command_not_allowed", "run", {"program": "aws", "args": ["kms", "decrypt"]})
        records = self.audit_lines()
        self.assertEqual(len(records), 2)
        first, second = records
        self.assertEqual(first["tool"], "cloudcli.run")
        self.assertEqual(first["caller"]["task_id"], "T-1")
        self.assertEqual(
            (first["connection"], first["path"], first["status"], first["outcome"], first["bytes"]),
            ("aws", "ec2 describe-regions", 0, "ok", len('{"Regions": []}')),
        )
        self.assertEqual(first["query"], {"args": ["ec2", "describe-regions", "--query", "[secret]"], "region": "eu-west-1"})
        self.assertIsNotNone(first["body_sha256"])
        self.assertEqual((second["path"], second["outcome"], second["status"]), ("kms decrypt", "command_not_allowed", None))
        audit = b"".join(path.read_bytes() for path in (self.state / "audit").glob("*.jsonl"))
        self.assertNotIn(AWS_SECRET.encode(), audit)
        for path in (self.state / "audit").iterdir():
            self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o600)


class VersionCheck(CloudCase):
    def test_an_older_interpreter_answers_python_too_old(self) -> None:
        request = json.dumps({"schema_version": 1, "tool": "cloudcli.run", "input": {"program": "aws", "args": ["sts", "get-caller-identity"]}})
        done = subprocess.run(
            [sys.executable, "-I", "-c", OLD_PYTHON, str(self.backend)],
            input=request,
            capture_output=True,
            text=True,
            env={"HOME": str(self.home), "PATH": self.path, "ORBIT_PLUGIN_STATE": str(self.state)},
            cwd=self.root,
            timeout=CALL_TIMEOUT,
            check=False,
        )
        self.assertEqual((done.returncode, done.stderr), (0, ""))
        error = json.loads(done.stdout)["error"]
        self.assertEqual(error["code"], "python_too_old")
        self.assertIn("orbit plugin enable cloudcli", error["message"])
        self.assertEqual(self.cli.runs(), [])

    def test_backend_parses_with_old_grammar(self) -> None:
        ast.parse(self.backend.read_text(), feature_version=(3, 7))
