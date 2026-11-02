use super::*;

impl FakeLibrary {
    pub(super) fn stage_attachment(&self, name: &str, text: &str) -> Result<String> {
        self.ready()?;
        let mut state = self.staging.state.borrow_mut();
        let path = format!("staging/{}/{name}", state.written.len() + 1);
        state.written.insert(path.clone(), text.to_string());
        Ok(path)
    }

    pub(super) fn remove_staged(&self, staged: &str) -> Result<()> {
        let mut state = self.staging.state.borrow_mut();
        if !state.written.contains_key(staged) {
            return Err(Error::Library {
                code: "unsafe_path",
                message: format!("{staged} is not a staged attachment"),
            });
        }
        state.removed.push(staged.to_string());
        Ok(())
    }

    pub(super) fn write_scribe_parse(
        &self,
        session_id: &str,
        messages: &str,
        meta: &str,
    ) -> Result<String> {
        self.ready()?;
        self.scribe_parses.borrow_mut().insert(
            session_id.to_string(),
            ScribeParse {
                messages: messages.to_string(),
                meta: meta.to_string(),
            },
        );
        self.scribe_parse_path(session_id)
    }

    pub(super) fn scribe_parse_path(&self, session_id: &str) -> Result<String> {
        Ok(office_core::domain::source::conversation::scribe::parse_path("library", session_id))
    }

    pub(super) fn read_scribe_parse(&self, session_id: &str) -> Result<Option<ScribeParse>> {
        self.ready()?;
        Ok(self.scribe_parses.borrow().get(session_id).cloned())
    }

    pub(super) fn scribe_files(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .scribe_texts
            .borrow()
            .iter()
            .map(|(path, text)| LibraryText {
                path: path.clone(),
                markdown: Some(text.clone()),
            })
            .collect())
    }

    pub(super) fn write_scribe_file(&self, write: &ScribeWrite) -> Result<()> {
        use office_core::domain::source::conversation::scribe::validate_scr_path;
        self.ready()?;
        validate_scr_path(Path::new(&write.path))?;
        let mut texts = self.scribe_texts.borrow_mut();
        let changed = || Error::Library {
            code: "scribe_changed",
            message: "the SCR file changed since it was read; retry".into(),
        };
        match &write.previous {
            Some((path, text)) => {
                validate_scr_path(Path::new(path))?;
                if texts.get(path) != Some(text) {
                    return Err(changed());
                }
                if path != &write.path {
                    if texts.contains_key(&write.path) {
                        return Err(Error::Library {
                            code: "scribe_exists",
                            message: "an SCR file already has the new name".into(),
                        });
                    }
                    texts.remove(path);
                }
            }
            None if texts.contains_key(&write.path) => return Err(changed()),
            None => {}
        }
        texts.insert(write.path.clone(), write.text.clone());
        drop(texts);
        self.record_commit();
        Ok(())
    }

    pub(super) fn decision_ledgers(&self) -> Result<Vec<LibraryText>> {
        self.ready()?;
        Ok(self
            .texts
            .borrow()
            .iter()
            .filter(|(path, _)| {
                office_core::domain::source::decisions::is_ledger_path(Path::new(path.as_str()))
            })
            .map(|(path, text)| LibraryText {
                path: path.clone(),
                markdown: Some(text.clone()),
            })
            .collect())
    }

    /// The fake checks every planned text, then writes the SCR and each file;
    /// the pass counts as one commit.
    pub(super) fn write_scribe_pass(&self, pass: &office_core::ports::ScribePass) -> Result<()> {
        use office_core::domain::source::conversation::scribe::{v2, validate_scr_path};
        self.ready()?;
        validate_scr_path(Path::new(&pass.scr.path))?;
        let changed = || Error::Library {
            code: "scribe_changed",
            message:
                "the SCR, a ledger or a preference page changed since the pass was planned; retry"
                    .into(),
        };
        let planned = pass.scr.previous.as_ref().map(|(_, text)| text);
        if self.scribe_texts.borrow().get(&pass.scr.path) != planned {
            return Err(changed());
        }
        if let Some(chain) = &pass.chain {
            let scrs = v2::v2_files(&self.scribe_files()?)?;
            let latest = v2::latest_in_scope(&scrs, &chain.scope, &chain.session)
                .map(|scr| scr.front.id.clone());
            if latest != chain.previous {
                return Err(changed());
            }
        }
        for file in &pass.files {
            if self.texts.borrow().get(&file.path) != file.expected.as_ref() {
                return Err(changed());
            }
        }
        for file in &pass.files {
            self.write_document(Path::new(&file.path), &file.text)?;
        }
        self.scribe_texts
            .borrow_mut()
            .insert(pass.scr.path.clone(), pass.scr.text.clone());
        self.record_commit();
        Ok(())
    }
}
