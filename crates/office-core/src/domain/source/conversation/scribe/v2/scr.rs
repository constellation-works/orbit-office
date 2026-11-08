//! The v2 SCR file (specs/decisions.md, "Legacy SCR files"): one file per scribed
//! session at `sources/conversation/<yyyy>/<mm>/SCR-<yyyymmdd>-<hhmm>-<session6>.md`,
//! with one-line front matter keys, four rewritten upper sections and an
//! append-only Tags section whose entries carry permanent `T<n>` ids.

use chrono::{DateTime, FixedOffset};
use serde_json::{Map, Value, json};

use super::super::{SCR_DIR, is_scr_path, refusal};
use crate::Result;
use crate::domain::source::LibraryText;

/// Rewritten section headings, in file order.
pub const SECTION_HEADINGS: [&str; 4] = ["Summary", "Goals", "Outcome and Blockers", "Next"];
/// The append-only section heading.
pub const TAGS_HEADING: &str = "Tags";
/// The Tags subsection for `#scribe <text>` instructions.
pub const REQUESTS_GROUP: &str = "Requests";

/// v2 SCR front matter; every key always present and on one line.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Front {
    /// File stem, fixed at creation.
    pub id: String,
    /// Title, from the Summary while scribe-set.
    pub title: Option<String>,
    /// `scribe` or `user`; a user title is never replaced.
    pub title_by: Option<String>,
    /// `project` or `global`.
    pub scope: String,
    /// The project slug for a project scope.
    pub project: Option<String>,
    /// Provider session id.
    pub session: String,
    /// Session start, host local time.
    pub started: String,
    /// Last pass, host local time.
    pub updated: String,
    /// The scope's latest SCR when this one was created.
    pub previous: Option<String>,
    /// Last message given to the scribe.
    pub scribed_through: Option<String>,
    /// Transcript format.
    pub provider: String,
    /// Last model the transcript reports.
    pub model: Option<String>,
    /// Host the session was parsed on.
    pub host: Option<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Git branch at session start.
    pub branch: Option<String>,
    /// Transcript path.
    pub transcript: Option<String>,
    /// Message file path.
    pub parsed: Option<String>,
    /// `normal` or `private`.
    pub sensitivity: String,
}

fn text(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields.get(key).and_then(Value::as_str).map(str::to_owned)
}

impl Front {
    /// The scope token: `global` or `project:<name>`.
    #[must_use]
    pub fn scope_token(&self) -> String {
        match (&*self.scope, &self.project) {
            ("project", Some(project)) => format!("project:{project}"),
            _ => "global".to_owned(),
        }
    }

    /// Whether the session was private.
    #[must_use]
    pub fn private(&self) -> bool {
        self.sensitivity == "private"
    }

    fn parse(fields: &Map<String, Value>) -> Self {
        Self {
            id: text(fields, "id").unwrap_or_default(),
            title: text(fields, "title"),
            title_by: text(fields, "title_by"),
            scope: text(fields, "scope").unwrap_or_else(|| "global".to_owned()),
            project: text(fields, "project"),
            session: text(fields, "session").unwrap_or_default(),
            started: text(fields, "started").unwrap_or_default(),
            updated: text(fields, "updated").unwrap_or_default(),
            previous: text(fields, "previous"),
            scribed_through: text(fields, "scribed_through"),
            provider: text(fields, "provider").unwrap_or_default(),
            model: text(fields, "model"),
            host: text(fields, "host"),
            cwd: text(fields, "cwd"),
            branch: text(fields, "branch"),
            transcript: text(fields, "transcript"),
            parsed: text(fields, "parsed"),
            sensitivity: text(fields, "sensitivity").unwrap_or_else(|| "normal".to_owned()),
        }
    }

    /// Render the front matter block, `---` lines included.
    #[must_use]
    pub fn render(&self) -> String {
        use crate::domain::knowledge::frontmatter::flow_value;
        let opt = |value: &Option<String>| value.as_ref().map_or(Value::Null, |v| json!(v));
        let fields: [(&str, Value); 18] = [
            ("id", json!(self.id)),
            ("title", opt(&self.title)),
            ("title_by", opt(&self.title_by)),
            ("scope", json!(self.scope)),
            ("project", opt(&self.project)),
            ("session", json!(self.session)),
            ("started", json!(self.started)),
            ("updated", json!(self.updated)),
            ("previous", opt(&self.previous)),
            ("scribed_through", opt(&self.scribed_through)),
            ("provider", json!(self.provider)),
            ("model", opt(&self.model)),
            ("host", opt(&self.host)),
            ("cwd", opt(&self.cwd)),
            ("branch", opt(&self.branch)),
            ("transcript", opt(&self.transcript)),
            ("parsed", opt(&self.parsed)),
            ("sensitivity", json!(self.sensitivity)),
        ];
        let mut out = String::from("---\n");
        for (key, value) in fields {
            out.push_str(&format!("{key}: {}\n", flow_value(&value)));
        }
        out.push_str("---\n");
        out
    }
}

/// The four rewritten sections.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sections {
    /// One line, starting `(scribe) `.
    pub summary: String,
    /// Goal items.
    pub goals: Vec<String>,
    /// Outcome and blocker items.
    pub outcome: Vec<String>,
    /// Next items, each citing a complete Orbit task id.
    pub next: Vec<String>,
}

/// One Tags subsection: its heading and its entry lines, verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagGroup {
    /// `Preferences`, `Decisions`, a custom tag name, or `Requests`.
    pub heading: String,
    /// Entry lines, byte for byte.
    pub lines: Vec<String>,
}

/// A v2 SCR read from the Library.
#[derive(Clone, Debug)]
pub struct Scr {
    /// Library-relative path.
    pub path: String,
    /// Full Markdown.
    pub markdown: String,
    /// Front matter.
    pub front: Front,
    /// The rewritten sections as written.
    pub sections: Sections,
    /// Tags subsections in file order.
    pub tags: Vec<TagGroup>,
}

/// Whether `markdown` is a v2 SCR (its front matter has `session`, not v1's
/// `session_id`).
#[must_use]
pub fn is_v2(markdown: &str) -> bool {
    crate::domain::metrics::metadata(markdown)
        .is_ok_and(|fields| fields.contains_key("session") && !fields.contains_key("session_id"))
}

fn items(lines: &[&str]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| line.strip_prefix("- "))
        .map(str::to_owned)
        .collect()
}

impl Scr {
    /// Parse a v2 SCR.
    pub fn parse(path: &str, markdown: &str) -> Result<Self> {
        let fields = crate::domain::metrics::metadata(markdown)?;
        let front = Front::parse(&fields);
        let body = crate::domain::source::source_body(markdown);
        let mut section = String::new();
        let mut blocks: Vec<(String, Vec<&str>)> = Vec::new();
        let mut tags: Vec<TagGroup> = Vec::new();
        for line in body.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                section = heading.trim().to_owned();
                blocks.push((section.clone(), Vec::new()));
                continue;
            }
            if section == TAGS_HEADING {
                if let Some(heading) = line.strip_prefix("### ") {
                    tags.push(TagGroup {
                        heading: heading.trim().to_owned(),
                        lines: Vec::new(),
                    });
                } else if line.starts_with("- ")
                    && let Some(group) = tags.last_mut()
                {
                    group.lines.push(line.to_owned());
                }
                continue;
            }
            if let Some((_, lines)) = blocks.last_mut()
                && !line.trim().is_empty()
            {
                lines.push(line);
            }
        }
        let block = |name: &str| -> Vec<&str> {
            blocks
                .iter()
                .find(|(heading, _)| heading == name)
                .map(|(_, lines)| lines.clone())
                .unwrap_or_default()
        };
        let sections = Sections {
            summary: block("Summary").join(" "),
            goals: items(&block("Goals")),
            outcome: items(&block("Outcome and Blockers")),
            next: items(&block("Next")),
        };
        Ok(Self {
            path: path.to_owned(),
            markdown: markdown.to_owned(),
            front,
            sections,
            tags,
        })
    }

    /// Every Tags entry, parsed, in file order.
    #[must_use]
    pub fn entries(&self) -> Vec<TagEntry> {
        self.tags
            .iter()
            .flat_map(|group| {
                group
                    .lines
                    .iter()
                    .filter_map(|line| TagEntry::parse(&group.heading, line))
            })
            .collect()
    }

    /// The next free `T<n>` number.
    #[must_use]
    pub fn next_tag_number(&self) -> usize {
        self.entries()
            .iter()
            .map(|entry| entry.number)
            .max()
            .unwrap_or(0)
            + 1
    }
}

/// Render a whole v2 SCR from its parts.
#[must_use]
pub fn render(front: &Front, sections: &Sections, tags: &[TagGroup]) -> String {
    let mut out = front.render();
    out.push_str("## Summary\n");
    out.push_str(&sections.summary);
    out.push('\n');
    for (heading, list) in [
        ("Goals", &sections.goals),
        ("Outcome and Blockers", &sections.outcome),
        ("Next", &sections.next),
    ] {
        out.push_str(&format!("\n## {heading}\n"));
        for item in list {
            out.push_str(&format!("- {item}\n"));
        }
    }
    out.push_str(&format!("\n## {TAGS_HEADING}\n"));
    for group in tags {
        out.push_str(&format!("### {}\n", group.heading));
        for line in &group.lines {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The Tags subsection heading of a tag: `Preferences`, `Decisions`, the
/// custom tag's name, or `Requests` for an instruction.
#[must_use]
pub fn group_heading(tag: Option<&str>) -> String {
    match tag {
        Some("preferences") => "Preferences".to_owned(),
        Some("decisions") => "Decisions".to_owned(),
        Some(name) => name.to_owned(),
        None => REQUESTS_GROUP.to_owned(),
    }
}

/// Subsection order: Preferences, Decisions, custom tags by name, Requests.
pub fn sort_groups(groups: &mut [TagGroup]) {
    let rank = |heading: &str| match heading {
        "Preferences" => 0,
        "Decisions" => 1,
        REQUESTS_GROUP => 3,
        _ => 2,
    };
    groups.sort_by(|a, b| {
        rank(&a.heading)
            .cmp(&rank(&b.heading))
            .then_with(|| a.heading.cmp(&b.heading))
    });
}

/// One Tags entry: `- T<n> <msg> <time> by: <who> scope: <scope> "<quote>" [<key>: <value>]…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagEntry {
    /// The subsection heading.
    pub group: String,
    /// The `T<n>` number.
    pub number: usize,
    /// The cited message id.
    pub msg: String,
    /// Minute-precision local time.
    pub time: String,
    /// `user` or `agent`.
    pub by: String,
    /// `global` or `project:<name>`.
    pub scope: String,
    /// The marker line, verbatim.
    pub quote: String,
    /// Trailing `key: value` pairs (`ledger`, `proposal`, `page`, `task`,
    /// `matter`, `propose`, `withheld`, …).
    pub fields: Vec<(String, String)>,
    /// The line as written.
    pub line: String,
}

impl TagEntry {
    /// `T<n>`.
    #[must_use]
    pub fn id(&self) -> String {
        format!("T{}", self.number)
    }

    /// A trailing field's value.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    /// Parse an entry line; `None` for anything else.
    #[must_use]
    pub fn parse(group: &str, line: &str) -> Option<Self> {
        let rest = line.strip_prefix("- T")?;
        let (number, rest) = rest.split_once(' ')?;
        if number.is_empty()
            || number.starts_with('0')
            || !number.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let number: usize = number.parse().ok()?;
        let (msg, rest) = rest.split_once(' ')?;
        let (time, rest) = rest.split_once(' ')?;
        let rest = rest.strip_prefix("by: ")?;
        let (by, rest) = rest.split_once(' ')?;
        let rest = rest.strip_prefix("scope: ")?;
        let (scope, rest) = rest.split_once(' ')?;
        let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<String>();
        let quote = stream.next()?.ok()?;
        let tail = rest.get(stream.byte_offset()..)?.trim();
        let mut fields = Vec::new();
        let mut words = tail.split(' ').filter(|w| !w.is_empty());
        while let Some(key) = words.next() {
            let key = key.strip_suffix(':')?;
            let value = words.next()?;
            fields.push((key.to_owned(), value.to_owned()));
        }
        Some(Self {
            group: group.to_owned(),
            number,
            msg: msg.to_owned(),
            time: time.to_owned(),
            by: by.to_owned(),
            scope: scope.to_owned(),
            quote,
            fields,
            line: line.to_owned(),
        })
    }

    /// Render an entry line.
    #[must_use]
    pub fn render(
        number: usize,
        msg: &str,
        time: &str,
        by: &str,
        scope: &str,
        quote: &str,
        fields: &[(String, String)],
    ) -> String {
        let quoted = serde_json::to_string(quote).unwrap_or_else(|_| "\"\"".to_owned());
        let mut line = format!("- T{number} {msg} {time} by: {by} scope: {scope} {quoted}");
        for (key, value) in fields {
            line.push_str(&format!(" {key}: {value}"));
        }
        line
    }
}

/// The v2 id of a session starting at `start` (local): `SCR-<yyyymmdd>-<hhmm>-<session6>`,
/// where `session6` is the session id's first six characters, lowercased,
/// with any other character as `0`.
#[must_use]
pub fn scr_id(start: DateTime<FixedOffset>, session_id: &str) -> String {
    let mut six: String = session_id
        .chars()
        .take(6)
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '0'
            }
        })
        .collect();
    while six.len() < 6 {
        six.push('0');
    }
    format!("SCR-{}-{six}", start.format("%Y%m%d-%H%M"))
}

/// `sources/conversation/<yyyy>/<mm>/<id>.md`.
#[must_use]
pub fn scr_path(start: DateTime<FixedOffset>, id: &str) -> String {
    format!("{SCR_DIR}/{}/{id}.md", start.format("%Y/%m"))
}

/// Every readable v2 SCR among the Library's SCR files; v1 files are
/// skipped (they stay readable through the v1 readers).
pub fn v2_files(files: &[LibraryText]) -> Result<Vec<Scr>> {
    let mut out = Vec::new();
    for file in files.iter().filter(|file| is_scr_path(&file.path)) {
        let markdown = file.markdown.as_deref().ok_or_else(|| {
            refusal(
                "library_document_unreadable",
                format!("cannot read {}", file.path),
            )
        })?;
        if is_v2(markdown) {
            out.push(Scr::parse(&file.path, markdown)?);
        }
    }
    Ok(out)
}

/// The v2 SCR recording `session_id`, if any.
pub fn find_session<'a>(files: &'a [Scr], session_id: &str) -> Result<Option<&'a Scr>> {
    let mut found = files.iter().filter(|f| f.front.session == session_id);
    let first = found.next();
    if found.next().is_some() {
        return Err(refusal(
            "scribe_session_conflict",
            "several SCR files record this session id; reconcile them first",
        ));
    }
    Ok(first)
}

fn updated_key(scr: &Scr) -> (Option<i64>, String) {
    (
        crate::domain::metrics::timestamp(&scr.front.updated),
        scr.front.id.clone(),
    )
}

/// The latest v2 SCR by `updated` in `scope`, other than `session_id`'s own.
#[must_use]
pub fn latest_in_scope<'a>(files: &'a [Scr], scope: &str, session_id: &str) -> Option<&'a Scr> {
    files
        .iter()
        .filter(|f| f.front.session != session_id && f.front.scope_token() == scope)
        .max_by_key(|f| updated_key(f))
}

/// The latest v2 SCR per scope token, in scope order (`global` first).
#[must_use]
pub fn latest_per_scope(files: &[Scr]) -> Vec<&Scr> {
    let mut latest: std::collections::BTreeMap<String, &Scr> = std::collections::BTreeMap::new();
    for file in files {
        let key = file.front.scope_token();
        match latest.get(&key) {
            Some(current) if updated_key(current) >= updated_key(file) => {}
            _ => {
                latest.insert(key, file);
            }
        }
    }
    latest.into_values().collect()
}
