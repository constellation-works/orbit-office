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

    /// Append one entry like the store adapter: the scope's next id, a new
    /// ledger on its first entry, and `reverses` checked across ledgers.
    pub(super) fn append_decision(
        &self,
        decision: &office_core::domain::source::decisions::Decision,
        captured_at: &str,
    ) -> Result<office_core::ports::DecisionOutcome> {
        use office_core::domain::source::decisions::{Ledger, append_line, views};
        self.ready()?;
        let scope = decision.parsed_scope()?;
        let mut ledgers = self
            .decision_ledgers()?
            .into_iter()
            .map(|file| {
                let text = file.markdown.unwrap_or_default();
                Ok((Ledger::parse(&file.path, &text)?, text))
            })
            .collect::<Result<Vec<_>>>()?;
        let index = match ledgers.iter().position(|(ledger, _)| ledger.scope == scope) {
            Some(index) => index,
            None => {
                let ledger = Ledger::new(scope, captured_at);
                let text = ledger.render()?;
                ledgers.push((ledger, text));
                ledgers.len() - 1
            }
        };
        let mut decision = decision.clone();
        decision.id = ledgers[index].0.next_id();
        decision.validate()?;
        if let Some(target) = &decision.reverses
            && !ledgers
                .iter()
                .any(|(ledger, _)| ledger.entry(target).is_some())
        {
            return Err(Error::Library {
                code: "decision_not_found",
                message: format!("reverses names {target}, which no decision ledger holds"),
            });
        }
        let path = ledgers[index].0.path.clone();
        let text = append_line(&ledgers[index].1, &decision.line()?);
        ledgers[index] = (Ledger::parse(&path, &text)?, text.clone());
        self.texts.borrow_mut().insert(path.clone(), text);
        self.record_commit();
        let parsed: Vec<Ledger> = ledgers.into_iter().map(|(ledger, _)| ledger).collect();
        let title = parsed[index].title();
        let entry = views(&parsed)
            .into_iter()
            .find(|view| view.decision.id == decision.id)
            .ok_or(Error::LibraryUnavailable)?;
        Ok(office_core::ports::DecisionOutcome { entry, path, title })
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

    /// The fake checks every planned ledger text, then writes each; the
    /// batch counts as one commit. No files: no commit.
    pub(super) fn write_decision_ledgers(
        &self,
        files: &[office_core::domain::source::conversation::scribe::v2::PlannedFile],
    ) -> Result<()> {
        self.ready()?;
        if files.is_empty() {
            return Ok(());
        }
        for file in files {
            if !office_core::domain::source::decisions::is_ledger_path(Path::new(&file.path)) {
                return Err(Error::InvalidArgument(
                    "a decision ledger batch writes only decision ledgers, each once".into(),
                ));
            }
            if self.texts.borrow().get(&file.path) != file.expected.as_ref() {
                return Err(Error::Library {
                    code: "decision_ledger_changed",
                    message: "a decision ledger changed since the batch was planned; retry".into(),
                });
            }
        }
        for file in files {
            self.write_document(Path::new(&file.path), &file.text)?;
        }
        self.record_commit();
        Ok(())
    }
}
