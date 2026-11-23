# Setting up the read-only identities

Tell the user how to create the plugin's read-only identity. Every command below stores a secret outside the working tree: the user runs it in their own terminal; you never run it and never see a key.

The plugin's identities are the security boundary: the command policy in `SKILL.md`
is only a second layer. Tell the user to create identities that can read and
never write, and to keep them separate from their own logins.

## AWS

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

## Google Cloud

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
