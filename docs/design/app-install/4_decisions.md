# App installation decisions

## Publish generated directories after building, preserving rollback material

The login service can point at a clone's app directory or an installed copy.
Replacing the entire app root would also replace user source and configuration.
The Make targets therefore build a pinned Git archive away from the running
service, then replace only the three package `dist` directories, the static
shell and `node_modules`. The installed service definition supplies the root
and port; unsupported custom definitions fail rather than guessing.

For `STD-03@2 §R5`, publication is atomic per generated directory, rather than
one rename across all runtime directories. This is a deliberate exception for
regenerable build output: the existing service uses several paths under a
source checkout, and a portable single rename would require changing that
checkout or the service layout. The app may briefly read mixed generations
during publication. The target restarts it and verifies the commit and shell
digest before reporting success. This is not a migration of application data.

An advisory lock bounds competing installs. Before publication a flushed,
atomically written pending record identifies the root, backup, candidate and
generated paths. A crash leaves the record and both generations available; a
later install refuses pending operator reconciliation. A handled failure
restores previous paths, tries to restart the old service, and retains the
candidate for inspection. Successful publication retains the previous runtime
and prints its location. TypeScript sources, Library data and service
configuration are preserved. The static shell is replaced from the archive,
with its prior files retained in the backup.

Build identity is a static JSON asset, so the existing router and package
manifests need no edit. A shell digest is compiled into the emitted worker after
TypeScript compilation. Production builds use the pinned commit; development
builds carry an explicit development identity and hash the actual shell bytes.
