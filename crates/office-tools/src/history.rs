//! Optional, bounded document history shared by the three Library show tools.

use std::path::Path;

use office_core::ports::Library;
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

const MAX_REVISIONS: usize = 50;

pub(crate) fn requested(input: &Map<String, Value>) -> Result<bool> {
    match input.get("history") {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(Error::InvalidArgument("history must be a boolean".into())),
    }
}

pub(crate) fn append(library: &dyn Library, path: &Path, output: &mut Value) -> Result<()> {
    let revisions = library.history(path)?;
    let total = revisions.len();
    let revisions: Vec<_> = revisions
        .into_iter()
        .take(MAX_REVISIONS)
        .map(|revision| {
            json!({
                "commit": revision.commit,
                "time": revision.authored_at,
                "summary": revision.message,
            })
        })
        .collect();
    output["history"] = json!({
        "revisions": revisions,
        "total": total,
        "truncated": total > MAX_REVISIONS,
    });
    Ok(())
}
