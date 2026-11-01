//! Plan file creation, renaming and append batches without I/O.

use std::collections::BTreeSet;
use std::path::Path;

use super::entry_line::Ctx;
use super::naming::{derived_title, named_title};
use super::{
    Entry, Front, Message, Parsed, Plan, Refusal, Request, ScrFile, ScribeRules, find_session,
    free_id, invalid, line_time, local, message_id, message_index, planned, refusal, session_start,
    slug, validate_scr_path,
};
use crate::Result;

const COUNTERPARTS: &[&str] = &["orchestrator", "agent", "coworker", "meeting", "self"];

fn single_line(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

/// The entry line, without the time token, for duplicate detection.
fn identity(line: &str) -> String {
    line.split(' ')
        .enumerate()
        .filter(|(index, _)| *index != 2)
        .map(|(_, part)| part)
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_body(markdown: &str) -> String {
    super::super::super::source_body(markdown)
}

/// Validate entries and plan the SCR file after this append.
pub fn append(
    request: &Request<'_>,
    entries: &[Entry],
    messages: &[Message],
    parsed: &Parsed,
    rules: &ScribeRules,
    files: &[ScrFile],
) -> Result<Plan> {
    if let Some(title) = request.title
        && !single_line(title, 4096)
    {
        return Err(invalid("title must be a nonempty single line"));
    }
    if let Some(title_by) = request.title_by
        && !matches!(title_by, "user" | "custom" | "scribe")
    {
        return Err(invalid("title_by must be user, custom or scribe"));
    }
    if request.title_by.is_some() && request.title.is_none() {
        return Err(invalid("title_by requires title"));
    }
    // A placeholder or blank explicit title counts as no title.
    let explicit: Option<(String, &'static str)> = match (request.title, request.title_by) {
        (Some(title), by) if named_title(Some(title)).is_some() => {
            let by = match by {
                Some("custom") => "custom",
                Some("scribe") => "scribe",
                _ => "user",
            };
            if by == "scribe" && title.split_whitespace().count() > 8 {
                return Err(invalid("a scribe title has at most eight words"));
            }
            Some((title.to_owned(), by))
        }
        _ => None,
    };
    let derived = derived_title(parsed, messages);
    if let Some(counterpart) = request.counterpart
        && !COUNTERPARTS.contains(&counterpart)
    {
        return Err(invalid(
            "counterpart is orchestrator, agent, coworker, meeting or self",
        ));
    }
    if let Some(name) = request.counterpart_name
        && !single_line(name, 256)
    {
        return Err(invalid(
            "counterpart_name must be a nonempty single line of at most 256 bytes",
        ));
    }
    let through = request
        .through
        .map(|through| {
            message_index(through)
                .filter(|i| *i < messages.len())
                .ok_or_else(|| {
                    refusal(
                        "scribe_through_unknown",
                        "through must name a parsed message; run office.scribe-pending first",
                    )
                })
        })
        .transpose()?;
    let existing = find_session(files, request.session_id)?;
    let previous_through = existing.and_then(|f| f.front.scribed_through.clone());
    let after = match &previous_through {
        None => None,
        Some(id) => Some(message_index(id).ok_or_else(|| {
            refusal(
                "scribe_file_invalid",
                "the SCR file's scribed_through is not a message id",
            )
        })?),
    };
    if through.is_some_and(|through| after.is_some_and(|after| through < after)) {
        return Err(refusal(
            "scribe_through_behind",
            "through is before the SCR file's scribed_through",
        ));
    }
    let now = local(request.now, request.zone)?;
    let others: Vec<ScrFile> = files
        .iter()
        .filter(|f| f.front.session_id != request.session_id)
        .cloned()
        .collect();

    // The file as it stands, or a new one.
    let (mut front, mut path, body, previous) = if let Some(file) = existing {
        (
            file.front.clone(),
            file.path.clone(),
            split_body(&file.markdown),
            Some((file.path.clone(), file.markdown.clone())),
        )
    } else {
        // Precedence on creation: user, then transcript-derived, then scribe.
        let chosen: Option<(String, &'static str)> = match (&explicit, &derived) {
            (Some((title, "user")), _) => Some((title.clone(), "user")),
            (_, Some((title, by))) => Some((title.clone(), *by)),
            (Some((title, by)), None) => Some((title.clone(), *by)),
            (None, None) => None,
        };
        let (id, path) = planned(
            &others,
            parsed,
            messages,
            chosen.as_ref().map(|(title, _)| title.as_str()),
            request.zone,
        )?;
        let start = session_start(parsed, messages, request.zone)?;
        let (title, title_by) = match chosen {
            Some((title, by)) => (Some(title), Some(by.to_owned())),
            None => (None, None),
        };
        let front = Front {
            id,
            title,
            title_by,
            session_id: request.session_id.to_owned(),
            provider: parsed.provider.clone(),
            model: parsed.model.clone(),
            host: parsed.host.clone(),
            project: parsed.project.clone(),
            cwd: parsed.cwd.clone(),
            branch: parsed.branch.clone(),
            transcript: Some(parsed.transcript.clone()),
            parsed: Some(parsed.parsed.clone()),
            started_at: Some(start.to_rfc3339()),
            ..Front::default()
        };
        (front, path, String::new(), None)
    };
    let created = previous.is_none();

    // Title precedence on an existing file. A legacy title with no
    // `title_by` was set by `/scribe title`, so it counts as user-owned.
    let mut renamed_from = None;
    if !created {
        let current_by: Option<String> = if front.title.is_some() {
            Some(front.title_by.clone().unwrap_or_else(|| "user".to_owned()))
        } else {
            None
        };
        let user_owned = current_by.as_deref() == Some("user");
        // A user title always wins and renames the file once.
        if let Some((title, "user")) = &explicit
            && front.title.as_deref() != Some(title.as_str())
        {
            front.title = Some(title.clone());
            front.title_by = Some("user".to_owned());
            let prefix = front.id.get(..12).unwrap_or_default().to_owned();
            let candidate = free_id(
                &others,
                &format!("{prefix}{}", {
                    let s = slug(title);
                    if s.is_empty() { "session".into() } else { s }
                }),
                request.session_id,
            )?;
            if candidate != front.id {
                let dir = path
                    .rsplit_once('/')
                    .map(|(d, _)| d.to_owned())
                    .unwrap_or_default();
                renamed_from = Some(front.id.clone());
                front.aliases.push(front.id.clone());
                front.id = candidate;
                path = format!("{dir}/{}.md", front.id);
            }
        } else if !user_owned {
            // Transcript-derived beats scribe; scribe refines only its own.
            if let Some((title, by)) = &derived {
                if front.title.as_deref() != Some(title.as_str()) {
                    front.title = Some(title.clone());
                    front.title_by = Some((*by).to_owned());
                }
            } else if let Some((title, by)) = &explicit {
                let may_set = match *by {
                    "custom" => true,
                    "scribe" => front.title.is_none() || current_by.as_deref() == Some("scribe"),
                    _ => false,
                };
                if may_set && front.title.as_deref() != Some(title.as_str()) {
                    front.title = Some(title.clone());
                    front.title_by = Some((*by).to_owned());
                }
            }
        }
    }
    validate_scr_path(Path::new(&path))?;
    if let Some(counterpart) = request.counterpart {
        front.counterpart = Some(counterpart.to_owned());
    }
    if let Some(name) = request.counterpart_name {
        front.counterpart_name = Some(name.to_owned());
    }

    let ctx = Ctx {
        messages,
        rules,
        files: &others,
        after,
        through,
        zone: request.zone,
        now,
    };
    let header_lines = front.render().lines().count();
    let mut body = body;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    let mut known: BTreeSet<String> = body.lines().map(identity).collect();
    let mut added = Vec::new();
    let mut refused = Vec::new();
    let mut duplicates = 0;
    let mut own_ids = vec![front.id.clone()];
    own_ids.extend(front.aliases.iter().cloned());
    let mut topics = front.topics.clone();
    for (index, entry) in entries.iter().enumerate() {
        let own = format!("{}{body}", front.render());
        match ctx.line(entry, &own, &own_ids) {
            Ok(line) => {
                if !known.insert(identity(&line)) {
                    duplicates += 1;
                    continue;
                }
                for topic in &entry.topics {
                    if !topics.contains(topic) {
                        topics.push(topic.clone());
                    }
                }
                body.push_str(&line);
                body.push('\n');
                added.push((header_lines + body.lines().count(), line));
            }
            Err((reason, message)) => refused.push(Refusal {
                index,
                msg: entry.msg.clone().unwrap_or_else(|| "note".into()),
                reason,
                message,
            }),
        }
    }
    // A batch that refuses something and captures nothing is a failed
    // capture: it writes no file, keeps the file's name and cursor, so the
    // messages stay pending and a corrected retry can capture them. An empty
    // batch (a catch-up) and a batch with a new line still advance the cursor.
    if !entries.is_empty() && added.is_empty() && !refused.is_empty() {
        let (front, path) = match existing {
            Some(file) => (file.front.clone(), file.path.clone()),
            None => (front, path),
        };
        return Ok(Plan {
            id: front.id.clone(),
            path,
            previous,
            markdown: None,
            added,
            duplicates,
            refused,
            renamed_from: None,
            previous_through,
            front,
        });
    }
    front.topics = topics;
    if let Some(through) = through {
        front.scribed_through = Some(message_id(through));
    }
    let unchanged = |front: &Front| {
        previous
            .as_ref()
            .is_some_and(|(_, old)| format!("{}{body}", front.render()) == *old)
    };
    let markdown = if unchanged(&front) {
        None
    } else {
        front.last_scribed_at = Some(line_time(now));
        Some(format!("{}{body}", front.render()))
    };
    Ok(Plan {
        id: front.id.clone(),
        path,
        previous,
        markdown,
        added,
        duplicates,
        refused,
        renamed_from,
        previous_through,
        front,
    })
}
