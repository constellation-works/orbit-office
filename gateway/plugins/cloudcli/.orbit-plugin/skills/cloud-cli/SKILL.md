---
name: cloud-cli
description: Run read-only aws and gcloud commands through the cloudcli plugin instead of the aws or gcloud CLI in the shell. Use whenever a task needs to look up AWS or Google Cloud resources, and to tell the user how to set up the plugin's read-only identities.
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
  `aws secretsmanager get-secret-value`, `aws ssm get-parameter` and
  `get-parameters` (and `-by-path`, `-history`) with `--with-decryption`,
  `aws kms decrypt`, `aws s3api get-object`, `aws lambda get-function`,
  `aws ecr get-login-password`, `aws ec2 get-password-data`,
  `aws rds generate-db-auth-token`, `aws eks get-token`,
  `gcloud secrets versions access`, `gcloud auth print-access-token` and
  `print-identity-token`, `gcloud sql generate-login-token`,
  `gcloud container clusters get-credentials`, and others of the same kind
  (temporary credentials, registry and database tokens, `dynamodb get-item`,
  `kinesis get-records`, the CLIs' own `configure`/`config`). Do not look for
  another way to read them; tell the user what you needed.
- `secret_unset` or `invalid_secret` — the plugin's identity for that cloud
  is not set up. Ask the user to set it up as below.
- `command_failed` — the CLI exited with an error; `detail.stderr` says why.
  `AccessDenied` or `PERMISSION_DENIED` means the read-only identity may not
  read that; tell the user rather than retrying.
- `timeout` — the command ran past 60 seconds and was stopped. Narrow it.
- `program_unavailable` — `aws` or `gcloud` is not installed on this machine.

## Setting up the read-only identities (for the user)

The plugin's identities are the security boundary: the command policy above
is only a second layer. Tell the user to create identities that can read and
never write, and to keep them separate from their own logins.

**AWS**

1. In the IAM console, create a user for the plugin (for example
   `orbit-cloudcli-readonly`) with no console access.
2. Attach the AWS managed policy `ReadOnlyAccess`, or the narrower
   `ViewOnlyAccess` if listing resources is enough. Attach nothing that
   writes.
3. Create an access key for it (use case: "Application running outside
   AWS"), then store both halves, each value on stdin, never as an argument:

   ```bash
   orbit plugin secret set cloudcli aws_access_key_id
   orbit plugin secret set cloudcli aws_secret_access_key
   ```

**Google Cloud**

1. In the project, create a service account for the plugin (for example
   `orbit-cloudcli-readonly`) and grant it `roles/viewer` (Viewer) only, on
   each project it should read.
2. Create a JSON key for it, store the file's contents, and delete the
   downloaded file:

   ```bash
   orbit plugin secret set cloudcli gcp_service_account_json < key.json && rm key.json
   ```

To rotate, create a new key, set it the same way, and delete the old one in
the console. The plugin never reads the user's own `~/.aws` or gcloud
configuration, so their personal logins are never used.
