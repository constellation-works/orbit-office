//! Session provenance, source scrubbing and append-only merge.

use serde_json::{Value, json};

use super::scrub::scrub;
use super::types::{Session, invalid};
use crate::{Error, Result};

/// Validate nullable session provenance on old and new sources.
pub fn validate_provenance(markdown: &str) -> Result<()> {
    let fields = crate::domain::metrics::metadata(markdown)?;
    for key in ["orchestrator", "model", "session_id"] {
        if let Some(value) = fields.get(key)
            && !value.is_null()
            && !value.as_str().is_some_and(|s| {
                !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
            })
        {
            return Err(Error::InvalidFrontmatter {
                field: key.into(),
                reason: "expected a nonempty single-line string or null",
            });
        }
    }
    if fields.get("kind").and_then(Value::as_str) == Some("conversation") {
        super::recall::validate_context(&fields)?;
    }
    Ok(())
}
/// Add transcript provenance without widening the general capture request.
fn provenance(session: &Session) -> Value {
    json!({"orchestrator":session.orchestrator,"model":session.model,"session_id":session.session_id,"transcript_sha256":session.transcript_sha256})
}

/// Scrub copied source text too: a handoff appended since the last extraction
/// must not introduce credentials into a newly persisted historical snapshot.
/// Clean source bytes stay identical, including their existing formatting.
pub fn scrub_source(markdown: &str) -> Result<String> {
    use crate::domain::knowledge::frontmatter::flow_value;
    fn clean(value: &Value) -> Value {
        match value {
            Value::String(text) => json!(scrub(text)),
            Value::Array(items) => Value::Array(items.iter().map(clean).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(key, value)| (key.clone(), clean(value)))
                    .collect(),
            ),
            Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        }
    }
    let fields = crate::domain::metrics::metadata(markdown)?;
    let old_body = super::super::source_body(markdown);
    let body = scrub(&old_body);
    let cleaned = fields
        .iter()
        .map(|(key, value)| (key.clone(), clean(value)))
        .collect::<serde_json::Map<_, _>>();
    if cleaned == fields && body == old_body {
        return Ok(markdown.into());
    }
    let mut result = String::from("---\n");
    for (key, value) in cleaned {
        result.push_str(&format!("{key}: {}\n", flow_value(&value)));
    }
    result.push_str("---\n");
    result.push_str(&body);
    Ok(result)
}

/// Recorded extraction progress of a conversation source, if it has run a
/// pass under the incremental scheme. Legacy sources return `(None, None)`.
pub fn extracted_state(markdown: &str) -> Result<(Option<usize>, Option<String>)> {
    let fields = crate::domain::metrics::metadata(markdown)?;
    let through = fields.get("extracted_through").and_then(|value| {
        value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    });
    let prefix = fields
        .get("extracted_prefix_sha256")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((through, prefix))
}

/// True when this session's bytes up to the source's recorded
/// `extracted_through` hash to its `extracted_prefix_sha256`: the
/// transcript only grew since that pass, so new entries may append.
pub fn prefix_grew(session: &Session, markdown: &str) -> Result<bool> {
    let (through, prefix) = extracted_state(markdown)?;
    Ok(match (through, prefix) {
        (Some(through), Some(prefix)) => session
            .line_prefix_sha256
            .get(through)
            .is_some_and(|hash| hash == &prefix),
        _ => false,
    })
}

/// Merge new evidence append-only, retaining handoffs and old citation anchors.
/// The caller journals the returned Markdown with an immutable prior snapshot.
pub fn merge(
    markdown: &str,
    session: &Session,
    new_entries: &[String],
    sensitivity: &str,
    changed: bool,
) -> Result<String> {
    use crate::domain::knowledge::frontmatter::flow_value;
    let mut fields = crate::domain::metrics::metadata(markdown)?;
    let version = super::super::parse_source_meta(markdown).version;
    let next = if changed {
        version
            .checked_add(1)
            .ok_or_else(|| invalid("conversation version overflow"))?
    } else {
        version
    };
    for (key, value) in provenance(session)
        .as_object()
        .ok_or_else(|| invalid("invalid provenance"))?
    {
        fields.insert(key.clone(), value.clone());
    }
    fields.insert("sensitivity".into(), json!(sensitivity));
    fields.insert("version".into(), json!(next));
    fields.insert("captured_by".into(), json!("transcript-extractor"));
    // Every successful pass advances the incremental cursor, even one that
    // selected nothing; the next pass starts after the last line here.
    if let (Some(last), Some(prefix)) = (session.lines.last(), session.line_prefix_sha256.last()) {
        fields.insert("extracted_through".into(), json!(last.line));
        fields.insert("extracted_prefix_sha256".into(), json!(prefix));
    }
    // Any new evidence revokes previous sharing approval.
    fields.insert("shareable".into(), json!(false));
    if changed {
        fields.insert("supersedes".into(), json!(format!("v{version}.md")));
    }
    let mut body = super::super::source_body(markdown);
    let mut known: std::collections::BTreeSet<String> = body.lines().map(str::to_owned).collect();
    for entry in new_entries {
        if known.insert(entry.clone()) {
            if !body.is_empty() && !body.ends_with('\n') {
                body.push('\n');
            }
            body.push_str(entry);
            body.push('\n');
        }
    }
    let mut result = String::from("---\n");
    for (key, value) in fields {
        result.push_str(&format!("{key}: {}\n", flow_value(&value)));
    }
    result.push_str("---\n");
    result.push_str(&body);
    validate_provenance(&result)?;
    Ok(result)
}
