---
name: cloudcli-cloud-cli
description: Runs read-only aws and gcloud commands through the cloudcli plugin instead of the aws or gcloud CLI in the shell, and tells the user how to set up the plugin's read-only identities. Use whenever a task needs to look up AWS or Google Cloud resources, when the user asks how to set up those identities, or when cloudcli reports a missing identity. Not for web APIs (use gateway-api-requests) or for any cloud write.
---

# Read-only cloud commands through cloudcli

The `cloudcli` plugin runs `aws` and `gcloud` for you with the plugin's own
identities, which the user created with read-only roles. You never see the
credentials, every call is audited, and every result is redacted before it
reaches you.

## Use cloudcli, not the CLI in your shell

Do not run `aws` or `gcloud` in your shell, and never ask the user for their
own keys or login. Use `cloudcli.run`. `args` is the command's argv after the
program name, one array item per argument, never a shell line:

```bash
orbit tool run cloudcli.run --input '{"program":"aws","args":["ec2","describe-instances","--max-items","20"],"region":"eu-west-1"}'
orbit tool run cloudcli.run --input '{"program":"gcloud","args":["compute","instances","list","--limit","20"],"project":"<project-id>"}'
```

- **aws**: `<service> <operation>` first, then the operation's flags. The
  operation must start with `describe-`, `list-` or `get-` (and
  `sts get-caller-identity` shows who the plugin is). `region` sets the
  region; `--region` in `args` works too.
- **gcloud**: the command words, ending in `list` or `describe` (a `describe`
  takes the resource name after it), then flags. `project` sets the project
  and `region` the default compute region. A leading `alpha` or `beta` is
  fine.
- Global flags you may set: aws `--region`, `--query`, `--max-items`,
  `--page-size`, `--starting-token`; gcloud `--filter`, `--limit`,
  `--page-size`, `--sort-by`. Output is always JSON, so `--output` and
  `--format` are refused; so are flags that change the endpoint, profile,
  account, configuration or logging, short flags (`-q`), and flags placed
  before the command words.

## What comes back

`output` is the command's JSON, parsed. `text` holds the output instead when
it is not JSON. Output is cut at 1 MiB, with `truncated: true`: narrow the
query (`--query`, `--filter`, `--max-items`, `--limit`) rather than paging
through everything. Anything that looks like a credential reads `[secret]`;
that is the plugin working, not a fault.

## When cloudcli refuses

- `command_not_allowed` — the command is outside the read-only policy; the
  message and `detail.rule` name the rule. Some commands are refused although
  their verb looks read-only, because they return secrets, data or tokens:
  - `aws secretsmanager get-secret-value`, `aws kms decrypt`,
    `aws s3api get-object`, `aws lambda get-function`,
    `aws ecr get-login-password`, `aws ec2 get-password-data`,
    `aws rds generate-db-auth-token`, `aws eks get-token`.
  - `aws ssm get-parameter` and `get-parameters` (and `-by-path`, `-history`)
    with `--with-decryption`.
  - `gcloud secrets versions access`, `gcloud auth print-access-token` and
    `print-identity-token`, `gcloud sql generate-login-token`,
    `gcloud container clusters get-credentials`.
  - Others of the same kind: temporary credentials, registry and database
    tokens, `dynamodb get-item`, `kinesis get-records`, the CLIs' own
    `configure`/`config`.

  Do not look for another way to read them; tell the user what you needed.
- `secret_unset` or `invalid_secret` — the plugin's identity for that cloud
  is not set up. Ask the user to set it up
  ([identity-setup.md](references/identity-setup.md)).
- `command_failed` — the CLI exited with an error; `detail.stderr` says why.
  `AccessDenied` or `PERMISSION_DENIED` means the read-only identity may not
  read that; tell the user rather than retrying.
- `timeout` — the command ran past 60 seconds and was stopped. Narrow it.
- `program_unavailable` — `aws` or `gcloud` is not installed on this machine.

## References

| Open when | Read |
|---|---|
| The user must create or rotate the plugin's read-only AWS or Google Cloud identity | [identity-setup.md](references/identity-setup.md) |
