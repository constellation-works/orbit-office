//! Bundled templates and retired note-source migration.

use std::path::Path;

use office_common::fs::atomic::write_atomic;
use office_common::fs::permissions::create_private_dirs;
use office_core::domain::template::BUNDLED;

use crate::error::{Error, Result};

use super::LibraryFs;
use super::TemplateOnDisk;

impl LibraryFs {
    /// True when every bundled template path already exists. An existing
    /// symlink or other entry counts as present so init will not overwrite it.
    pub(crate) fn bundled_templates_present(&self) -> Result<bool> {
        for bundled in BUNDLED {
            if !self.template_entry_exists(bundled.filename)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Copy each bundled template that is missing. Never overwrites an
    /// existing path. Returns the Library-relative paths this call created,
    /// including `.office/templates` when that directory was created.
    pub(crate) fn install_missing_templates(&self) -> Result<Vec<String>> {
        let dir = self.root.join(".office/templates");
        let mut created = Vec::new();
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(Error::UnsafePath {
                    kind: "symlink",
                    value: ".office/templates".to_string(),
                });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_private_dirs(&dir)?;
                created.push(".office/templates".to_string());
            }
            Err(error) => return Err(Error::io("inspect template directory", &dir, error)),
        }
        for bundled in BUNDLED {
            if self.template_entry_exists(bundled.filename)? {
                continue;
            }
            let path = dir.join(bundled.filename);
            write_atomic(&path, bundled.markdown.as_bytes())?;
            let filename = bundled.filename;
            created.push(format!(".office/templates/{filename}"));
        }
        Ok(created)
    }

    /// Template Markdown directly under `.office/templates/`, in path order.
    /// A missing directory is an empty list. A symlink or unreadable file is
    /// returned with `markdown: None` so status can name it and continue.
    pub(crate) fn read_templates(&self) -> Result<Vec<TemplateOnDisk>> {
        let dir = self.root.join(".office/templates");
        match std::fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Ok(vec![TemplateOnDisk {
                    relative: ".office/templates".to_string(),
                    markdown: None,
                }]);
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("inspect template directory", &dir, error)),
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(Error::io("list templates", &dir, error)),
        };
        let mut templates = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| Error::io("list templates", &dir, error))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !name.ends_with(".md") {
                continue;
            }
            let path = entry.path();
            let relative = format!(".office/templates/{name}");
            let markdown = match std::fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => None,
                Ok(_) => std::fs::read_to_string(&path).ok(),
                Err(_) => None,
            };
            templates.push(TemplateOnDisk { relative, markdown });
        }
        templates.sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(templates)
    }

    /// Move every retired `note` source into the decision ledgers
    /// (specs/decisions.md, "Retiring `sources/note`"): one entry per note in
    /// its scope's ledger, `moved_sources` mapping the note id to the entry,
    /// then the note's directory is removed. Ledgers are written before any
    /// directory is removed, and a note already listed in `moved_sources`
    /// is not appended again, so an interrupted run completes on a re-run.
    /// A private note is left in place: its text cannot enter a normal
    /// ledger. Returns the removed note directories and the written ledger
    /// paths, both Library-relative and sorted; a re-run on a migrated
    /// Library returns two empty lists and writes nothing.
    pub(crate) fn migrate_note_sources(
        &self,
        created_at: &str,
    ) -> Result<(Vec<String>, Vec<String>)> {
        use office_core::domain::source::decisions::{Ledger, note_entry, note_source};
        let mut notes = Vec::new();
        for (path, text) in self.read_layer_markdown("sources", &["source"])? {
            let Some(note) = note_source(&path, &text, created_at) else {
                continue;
            };
            if note.private {
                continue;
            }
            notes.push((
                note.decided_at,
                note.id,
                path,
                note.title,
                note.body,
                note.scope,
            ));
        }
        if notes.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }
        notes.sort();
        let mut ledgers: Vec<(Ledger, String)> = Vec::new();
        for (path, text) in self.read_ledger_markdown()? {
            ledgers.push((Ledger::parse(&path, &text)?, text));
        }
        let mut written: Vec<String> = Vec::new();
        for (decided_at, id, _, title, body, scope) in &notes {
            if ledgers
                .iter()
                .any(|(ledger, _)| ledger.moved_sources.contains_key(id))
            {
                continue;
            }
            let index = match ledgers
                .iter()
                .position(|(ledger, _)| ledger.scope == *scope)
            {
                Some(index) => index,
                None => {
                    let ledger = Ledger::new(scope.clone(), created_at);
                    let text = ledger.render()?;
                    ledgers.push((ledger, text));
                    ledgers.len() - 1
                }
            };
            let (ledger, text) = &mut ledgers[index];
            let entry_id = ledger.next_id();
            let decision = note_entry(scope, &entry_id, id, title, body, decided_at)?;
            let line = decision.line()?;
            let mut appended = office_core::domain::source::decisions::append_line(text, &line);
            ledger.moved_sources.insert(id.clone(), entry_id);
            // Re-render only the front matter; every body byte is kept.
            let body = office_core::domain::source::source_body(&appended);
            appended = format!("{}{body}", ledger.frontmatter());
            *ledger = Ledger::parse(&ledger.path.clone(), &appended)?;
            *text = appended;
            if !written.contains(&ledger.path) {
                written.push(ledger.path.clone());
            }
        }
        for (ledger, text) in &ledgers {
            if written.contains(&ledger.path) {
                self.write_document(Path::new(&ledger.path), text)?;
            }
        }
        let mut removed = Vec::new();
        for (_, _, path, _, _, _) in &notes {
            let Some(dir) = Path::new(path).parent() else {
                continue;
            };
            self.validate_relative_path(dir)?;
            let absolute = self.root.join(dir);
            std::fs::remove_dir_all(&absolute)
                .map_err(|error| Error::io("remove migrated note", &absolute, error))?;
            removed.push(dir.to_string_lossy().replace('\\', "/"));
            // Prune emptied month, year and kind directories up to `sources`.
            let mut parent = absolute.parent();
            while let Some(dir) = parent {
                if dir == self.root.join("sources") || std::fs::remove_dir(dir).is_err() {
                    break;
                }
                parent = dir.parent();
            }
        }
        removed.sort();
        written.sort();
        Ok((removed, written))
    }

    fn template_entry_exists(&self, filename: &str) -> Result<bool> {
        let path = self.root.join(".office/templates").join(filename);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(Error::io("inspect template", &path, error)),
        }
    }
}
