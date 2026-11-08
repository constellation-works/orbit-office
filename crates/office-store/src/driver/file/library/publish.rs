//! Artifact bundle publication and legacy source migration.

use std::path::Path;

use office_common::fs::atomic::write_atomic_new;
use office_common::fs::permissions::create_private_dirs;

use crate::error::{Error, Result};

use super::LibraryFs;
use super::collection::refuse_symlink;

impl LibraryFs {
    /// Stage a complete immutable artifact beside its destination and publish
    /// it with one directory rename. The durable journal owns staging on every
    /// error path; readers never scan that reserved staging directory.
    pub(crate) fn publish_artifact_bundle(
        &self,
        relative: &Path,
        text: &str,
        files: &[office_core::domain::artifact::ArtifactFile],
        token: &str,
    ) -> Result<()> {
        self.validate_document_path(relative)?;
        let directory = relative.parent().ok_or_else(|| Error::UnsafePath {
            kind: "artifact",
            value: relative.display().to_string(),
        })?;
        let destination = self.root.join(directory);
        let mut expected = files
            .iter()
            .map(|file| (file.path.as_str(), file.bytes.as_slice()))
            .collect::<Vec<_>>();
        expected.push(("artifact.md", text.as_bytes()));
        match std::fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.is_dir() => {
                // Recovery after publication may only verify, never repair or
                // replace an immutable published directory.
                for (name, bytes) in &expected {
                    self.validate_relative_path(&directory.join(name))?;
                    let path = destination.join(name);
                    let actual = std::fs::read(&path)
                        .map_err(|e| Error::io("verify published artifact", &path, e))?;
                    if actual != *bytes {
                        return Err(Error::Corrupt {
                            path,
                            reason: "published artifact differs from the decided bundle".into(),
                        });
                    }
                }
                return Ok(());
            }
            Ok(_) => {
                return Err(Error::UnsafePath {
                    kind: "occupied artifact",
                    value: directory.display().to_string(),
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io("inspect artifact destination", &destination, e)),
        }
        let parent = destination.parent().ok_or_else(|| Error::UnsafePath {
            kind: "artifact",
            value: directory.display().to_string(),
        })?;
        create_private_dirs(parent)?;
        let staging = parent.join(format!(".office-pending-artifact-{token}"));
        refuse_symlink(&staging)?;
        create_private_dirs(&staging)?;
        for (name, bytes) in &expected {
            let path = staging.join(name);
            // Names were validated by the domain. Validate every existing
            // staging component too: a stale journal never grants a symlink.
            let mut physical = staging.clone();
            for component in Path::new(name).components() {
                let std::path::Component::Normal(component) = component else {
                    return Err(Error::UnsafePath {
                        kind: "artifact file",
                        value: (*name).into(),
                    });
                };
                physical.push(component);
                refuse_symlink(&physical)?;
            }
            if let Some(parent) = path.parent() {
                create_private_dirs(parent)?;
            }
            match write_atomic_new(&path, bytes) {
                Ok(()) => {}
                Err(office_common::Error::Io { source, .. })
                    if source.kind() == std::io::ErrorKind::AlreadyExists =>
                {
                    let actual = std::fs::read(&path)
                        .map_err(|e| Error::io("verify staged artifact", &path, e))?;
                    if actual != *bytes {
                        return Err(Error::Corrupt {
                            path,
                            reason: "staged artifact differs from the decided bundle".into(),
                        });
                    }
                }
                Err(error) => return Err(error.into()),
            }
            let mut ancestor = path.parent();
            while let Some(dir) = ancestor {
                std::fs::File::open(dir)
                    .and_then(|file| file.sync_all())
                    .map_err(|e| Error::io("sync staged artifact", dir, e))?;
                if dir == parent {
                    break;
                }
                ancestor = dir.parent();
            }
        }
        // Only plugin writers share this root, serialized by the writer lock.
        // Refuse a newly occupied destination rather than replacing it.
        match std::fs::symlink_metadata(&destination) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {
                return Err(Error::UnsafePath {
                    kind: "occupied artifact",
                    value: directory.display().to_string(),
                });
            }
            Err(e) => return Err(Error::io("inspect artifact destination", &destination, e)),
        }
        std::fs::rename(&staging, &destination)
            .map_err(|e| Error::io("publish artifact bundle", &destination, e))?;
        let mut ancestor = Some(parent);
        while let Some(dir) = ancestor {
            std::fs::File::open(dir)
                .and_then(|file| file.sync_all())
                .map_err(|e| Error::io("sync artifact directory", dir, e))?;
            if dir == self.root {
                break;
            }
            ancestor = dir.parent();
        }
        Ok(())
    }

    /// Move every legacy `sources/YYYY/MM/<id>/` directory into the
    /// kind-partitioned `sources/<kind>/YYYY/MM/<id>/` layout, using the
    /// `kind` in its `source.md` frontmatter. The whole directory moves, so
    /// originals and sidecars travel with it. A source whose kind is not a
    /// known source kind, or whose destination already exists, is left in
    /// place and stays readable. Returns `(old, new)` Library-relative
    /// directories in path order; a re-run on a migrated Library is empty.
    pub(crate) fn migrate_legacy_sources(&self) -> Result<Vec<(String, String)>> {
        let sources = self.root.join("sources");
        match std::fs::symlink_metadata(&sources) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Ok(Vec::new());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("inspect sources", &sources, error)),
        }
        let mut years = Vec::new();
        for entry in std::fs::read_dir(&sources)
            .map_err(|error| Error::io("list sources", &sources, error))?
        {
            let entry = entry.map_err(|error| Error::io("list sources", &sources, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| Error::io("inspect source entry", entry.path(), error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if file_type.is_dir()
                && name.len() == 4
                && name.bytes().all(|byte| byte.is_ascii_digit())
            {
                years.push((name.to_string(), entry.path()));
            }
        }
        years.sort();
        let mut moved = Vec::new();
        for (yyyy, year_dir) in years {
            let mut months = Vec::new();
            for entry in std::fs::read_dir(&year_dir)
                .map_err(|error| Error::io("list source year", &year_dir, error))?
            {
                let entry =
                    entry.map_err(|error| Error::io("list source year", &year_dir, error))?;
                let file_type = entry
                    .file_type()
                    .map_err(|error| Error::io("inspect source entry", entry.path(), error))?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if file_type.is_dir()
                    && name.len() == 2
                    && name.bytes().all(|byte| byte.is_ascii_digit())
                {
                    months.push((name.to_string(), entry.path()));
                }
            }
            months.sort();
            for (mm, month_dir) in months {
                let mut ids = Vec::new();
                for entry in std::fs::read_dir(&month_dir)
                    .map_err(|error| Error::io("list source month", &month_dir, error))?
                {
                    let entry =
                        entry.map_err(|error| Error::io("list source month", &month_dir, error))?;
                    let file_type = entry
                        .file_type()
                        .map_err(|error| Error::io("inspect source entry", entry.path(), error))?;
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if file_type.is_dir() {
                        ids.push((name.to_string(), entry.path()));
                    }
                }
                ids.sort();
                for (id, id_dir) in ids {
                    let source_file = id_dir.join("source.md");
                    match std::fs::symlink_metadata(&source_file) {
                        Ok(metadata) if metadata.is_file() => {}
                        Ok(_) => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(error) => {
                            return Err(Error::io("inspect source", &source_file, error));
                        }
                    }
                    let text = std::fs::read_to_string(&source_file)
                        .map_err(|error| Error::io("read source", &source_file, error))?;
                    let meta = office_core::domain::source::parse_source_meta(&text);
                    let Some(kind) = office_core::domain::source::SourceKind::parse(&meta.kind)
                    else {
                        continue;
                    };
                    let old = format!("sources/{yyyy}/{mm}/{id}");
                    let new = format!("sources/{}/{yyyy}/{mm}/{id}", kind.as_str());
                    let destination = self.root.join(&new);
                    match std::fs::symlink_metadata(&destination) {
                        Ok(_) => continue,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => {
                            return Err(Error::io("inspect source", &destination, error));
                        }
                    }
                    if let Some(parent) = destination.parent() {
                        create_private_dirs(parent)?;
                    }
                    std::fs::rename(&id_dir, &destination)
                        .map_err(|error| Error::io("move legacy source", &destination, error))?;
                    moved.push((old, new));
                }
                let _ = std::fs::remove_dir(&month_dir);
            }
            let _ = std::fs::remove_dir(&year_dir);
        }
        Ok(moved)
    }
}
