//! SCR front matter and files read from the Library.

use serde_json::{Map, Value, json};

use super::{is_scr_path, refusal};
use crate::Result;
use crate::ports::LibraryText;

/// SCR front matter, every key always present and on one line, so file
/// line numbers of entries never move.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Front {
    /// File stem.
    pub id: String,
    /// Title set by `/scribe title`; `None` until then.
    pub title: Option<String>,
    /// Who set `title`: `user`, `custom` or `scribe`; `None` with no title.
    pub title_by: Option<String>,
    /// Provider session id.
    pub session_id: String,
    /// Transcript format.
    pub provider: String,
    /// Last model the transcript reports.
    pub model: Option<String>,
    /// orchestrator, agent, coworker, meeting or self.
    pub counterpart: Option<String>,
    /// Free-form name of the counterpart.
    pub counterpart_name: Option<String>,
    /// Host the session was parsed on.
    pub host: Option<String>,
    /// Project name.
    pub project: Option<String>,
    /// Working directory.
    pub cwd: Option<String>,
    /// Git branch at session start.
    pub branch: Option<String>,
    /// Transcript path.
    pub transcript: Option<String>,
    /// Message file path.
    pub parsed: Option<String>,
    /// Session start, host local time.
    pub started_at: Option<String>,
    /// Last append, host local time.
    pub last_scribed_at: Option<String>,
    /// Last message given to the scribe.
    pub scribed_through: Option<String>,
    /// Merged entry topics.
    pub topics: Vec<String>,
    /// Earlier ids of this file.
    pub aliases: Vec<String>,
}

fn text(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn list(fields: &Map<String, Value>, key: &str) -> Vec<String> {
    fields
        .get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

impl Front {
    /// Read the front matter of an SCR file.
    pub fn parse(markdown: &str) -> Result<Self> {
        let fields = crate::domain::metrics::metadata(markdown)?;
        Ok(Self {
            id: text(&fields, "id").unwrap_or_default(),
            title: text(&fields, "title"),
            title_by: text(&fields, "title_by"),
            session_id: text(&fields, "session_id").unwrap_or_default(),
            provider: text(&fields, "provider").unwrap_or_default(),
            model: text(&fields, "model"),
            counterpart: text(&fields, "counterpart"),
            counterpart_name: text(&fields, "counterpart_name"),
            host: text(&fields, "host"),
            project: text(&fields, "project"),
            cwd: text(&fields, "cwd"),
            branch: text(&fields, "branch"),
            transcript: text(&fields, "transcript"),
            parsed: text(&fields, "parsed"),
            started_at: text(&fields, "started_at"),
            last_scribed_at: text(&fields, "last_scribed_at"),
            scribed_through: text(&fields, "scribed_through"),
            topics: list(&fields, "topics"),
            aliases: list(&fields, "aliases"),
        })
    }

    /// Render the front matter block, `---` lines included.
    #[must_use]
    pub fn render(&self) -> String {
        use crate::domain::knowledge::frontmatter::flow_value;
        let opt = |value: &Option<String>| value.as_ref().map_or(Value::Null, |v| json!(v));
        let fields: [(&str, Value); 19] = [
            ("id", json!(self.id)),
            ("title", opt(&self.title)),
            ("title_by", opt(&self.title_by)),
            ("session_id", json!(self.session_id)),
            ("provider", json!(self.provider)),
            ("model", opt(&self.model)),
            ("counterpart", opt(&self.counterpart)),
            ("counterpart_name", opt(&self.counterpart_name)),
            ("host", opt(&self.host)),
            ("project", opt(&self.project)),
            ("cwd", opt(&self.cwd)),
            ("branch", opt(&self.branch)),
            ("transcript", opt(&self.transcript)),
            ("parsed", opt(&self.parsed)),
            ("started_at", opt(&self.started_at)),
            ("last_scribed_at", opt(&self.last_scribed_at)),
            ("scribed_through", opt(&self.scribed_through)),
            ("topics", json!(self.topics)),
            ("aliases", json!(self.aliases)),
        ];
        let mut out = String::from("---\n");
        for (key, value) in fields {
            out.push_str(&format!("{key}: {}\n", flow_value(&value)));
        }
        out.push_str("---\n");
        out
    }

    /// Metadata a caller reports about this file.
    #[must_use]
    pub fn view(&self, path: &str, exists: bool) -> Value {
        json!({"exists":exists,"id":self.id,"path":path,"title":self.title,"title_by":self.title_by,
            "scribed_through":self.scribed_through,"last_scribed_at":self.last_scribed_at,
            "topics":self.topics,"aliases":self.aliases,"counterpart":self.counterpart,
            "counterpart_name":self.counterpart_name})
    }
}

/// An SCR file read from the Library.
#[derive(Clone, Debug)]
pub struct ScrFile {
    /// Library-relative path.
    pub path: String,
    /// Full Markdown.
    pub markdown: String,
    /// Parsed front matter.
    pub front: Front,
}

/// Every readable SCR file, parsed.
pub fn scr_files(files: &[LibraryText]) -> Result<Vec<ScrFile>> {
    files
        .iter()
        .filter(|file| is_scr_path(&file.path))
        .map(|file| {
            let markdown = file.markdown.clone().ok_or_else(|| {
                refusal(
                    "library_document_unreadable",
                    format!("cannot read {}", file.path),
                )
            })?;
            Ok(ScrFile {
                path: file.path.clone(),
                front: Front::parse(&markdown)?,
                markdown,
            })
        })
        .collect()
}

/// The SCR file recording `session_id`, if any.
pub fn find_session<'a>(files: &'a [ScrFile], session_id: &str) -> Result<Option<&'a ScrFile>> {
    let mut found = files.iter().filter(|f| f.front.session_id == session_id);
    let first = found.next();
    if found.next().is_some() {
        return Err(refusal(
            "scribe_session_conflict",
            "several SCR files record this session id; reconcile them first",
        ));
    }
    Ok(first)
}
