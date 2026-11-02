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
}
