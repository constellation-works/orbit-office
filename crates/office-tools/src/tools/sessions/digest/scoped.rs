//! The digest since one meeting: Decided, Shipped and Needs you, every line
//! cited. Deterministic and read-only: no model call, no Library write.

use office_core::domain::session::task_open;
use office_core::domain::source::decision_precedents::conflicting_pair;
use office_core::domain::source::video::refusal;
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::Call;

/// Tasks filed from one source, in one workspace (`None` is the office's).
struct Filed {
    workspace: Option<String>,
    tasks: Vec<Value>,
    truncated: bool,
}

fn invalid(what: &str) -> Error {
    Error::Library {
        code: "digest_data_invalid",
        message: format!("source digest received an invalid {what}"),
    }
}

fn request(value: Value, workspace: Option<&str>) -> Result<Map<String, Value>> {
    let mut input = value
        .as_object()
        .cloned()
        .ok_or_else(|| invalid("request"))?;
    if let Some(selector) = workspace {
        input.insert("workspace".into(), json!(selector));
    }
    Ok(input)
}

fn text<'a>(row: &'a Value, key: &str) -> Result<&'a str> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(&format!("task {key}")))
}

fn tagged(row: &Value, tag: &str) -> bool {
    row.get("tags")
        .and_then(Value::as_array)
        .is_some_and(|tags| tags.iter().any(|t| t.as_str() == Some(tag)))
}

pub(super) fn run(call: &Call<'_>) -> Result<Value> {
    if call.input.contains_key("day") {
        return Err(Error::InvalidArgument(
            "digest day and source cannot be combined".into(),
        ));
    }
    let id = match call.input.get("source") {
        Some(Value::String(id)) if !id.trim().is_empty() => id.clone(),
        _ => {
            return Err(Error::InvalidArgument(
                "digest source must be a nonempty source id".into(),
            ));
        }
    };
    let view = call.ports.library.read_source(&id)?;
    if view.kind != "meeting" {
        return Err(refusal(
            "invalid_source_kind",
            "a source digest needs a meeting source",
        ));
    }
    let result = call
        .ports
        .library
        .source_sidecar(&id, "extract-result.json")?
        .ok_or_else(|| refusal("meeting_not_extracted", "extract the meeting first"))?;
    let result: Value =
        serde_json::from_str(&result).map_err(|_| invalid("meeting extract result"))?;
    let decided = decided(&id, &result, view.sensitivity == "private")?;

    // The office workspace is all-or-nothing; an extra workspace that cannot
    // be read is named in `unavailable` and never refuses the digest.
    let settings = call.ports.library.settings()?;
    let mut filed = vec![filed(call, &id, None)?];
    let mut unavailable = Vec::new();
    for selector in &settings.brief.task_workspaces {
        match filed_in(call, &id, selector) {
            Ok(found) => filed.push(found),
            Err(_) => unavailable.push(selector.clone()),
        }
    }
    let mut shipped = Vec::new();
    let mut needs_you = Vec::new();
    for found in &filed {
        let workspace = found.workspace.as_deref();
        for task in &found.tasks {
            let task_id = text(task, "id")?;
            let title = text(task, "title")?;
            let status = text(task, "status")?;
            if status == "done" && !tagged(task, "owner:you") {
                shipped.push(delivered(call, task_id, title, workspace));
            } else if task_open(status) && tagged(task, "owner:you") {
                // A contradiction to-do also cites both ledger entries.
                let kind = if tagged(task, "office-scribe-contradictions") {
                    "contradiction"
                } else {
                    "to_do"
                };
                let mut cites = vec![task_id.to_owned()];
                if let Some((left, right)) = conflicting_pair(task) {
                    cites.extend([left, right]);
                }
                needs_you.push(json!({"kind":kind,"text":title,"task":task_id,"workspace":workspace,"cites":cites}));
            }
        }
        match open_decisions(call, found) {
            Ok(decisions) => needs_you.extend(decisions),
            Err(error) => match workspace {
                None => return Err(error),
                Some(selector) => unavailable.push(selector.to_owned()),
            },
        }
    }
    unavailable.dedup();
    let truncated = filed.iter().any(|f| f.truncated);
    let report = report(&view.title, &decided, &shipped, &needs_you);
    Ok(json!({
        "source": id,
        "title": view.title,
        "decided": decided,
        "shipped": shipped,
        "needs_you": needs_you,
        "report": report,
        "unavailable": unavailable,
        "truncated": truncated,
    }))
}

/// The meeting's extracted decisions, each citing its anchors. A private
/// meeting's decisions are listed by reference only.
fn decided(id: &str, result: &Value, private: bool) -> Result<Vec<Value>> {
    let decisions = result
        .get("decisions")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("meeting extract result"))?;
    // A decision without a citation is not reported.
    decisions
        .iter()
        .filter(|d| {
            d.get("cites")
                .and_then(Value::as_array)
                .is_none_or(|c| !c.is_empty())
        })
        .map(|decision| {
            let cites = decision
                .get("cites")
                .and_then(Value::as_array)
                .ok_or_else(|| invalid("decision citation"))?
                .iter()
                .map(|anchor| {
                    anchor
                        .as_str()
                        .map(|anchor| format!("{id}{anchor}"))
                        .ok_or_else(|| invalid("decision citation"))
                })
                .collect::<Result<Vec<_>>>()?;
            let text = if private {
                "Private decision (reference only).".to_owned()
            } else {
                let text = decision
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid("decision"))?;
                text.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            Ok(json!({"text":text,"cites":cites}))
        })
        .collect()
}

fn filed(call: &Call<'_>, id: &str, workspace: Option<&str>) -> Result<Filed> {
    let input = request(
        json!({"tag":format!("source:{id}"),"limit":10000,"fields":["id","title","status","tags","description"]}),
        workspace,
    )?;
    let response = call.ports.orbit.task_list(&input)?;
    let tasks = response
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("task list"))?
        .clone();
    let truncated = response
        .get("truncated")
        .and_then(Value::as_bool)
        .ok_or_else(|| invalid("task list"))?;
    Ok(Filed {
        workspace: workspace.map(str::to_owned),
        tasks,
        truncated,
    })
}

fn filed_in(call: &Call<'_>, id: &str, selector: &str) -> Result<Filed> {
    let found = filed(call, id, Some(selector))?;
    // Validate the rows here so a malformed extra workspace is unavailable,
    // not a refusal of the whole digest.
    for task in &found.tasks {
        for key in ["id", "title", "status"] {
            text(task, key)?;
        }
    }
    Ok(found)
}

/// A done task with its PR and merge evidence from the newest delivery run.
/// A task with no readable delivery record still shipped, with no links.
fn delivered(call: &Call<'_>, task: &str, title: &str, workspace: Option<&str>) -> Value {
    let delivery = request(json!({"id":task,"field":"delivery"}), workspace)
        .and_then(|input| call.ports.orbit.task_show(&input))
        .ok()
        .filter(|d| d.get("task_id").and_then(Value::as_str) == Some(task));
    let repository = delivery
        .as_ref()
        .and_then(|d| d.get("repository"))
        .and_then(Value::as_str)
        .filter(|repo| repository(repo));
    let landing = delivery.as_ref().and_then(|d| d.get("landing"));
    let pr = landing
        .and_then(|l| l.get("pr_number"))
        .and_then(Value::as_u64)
        .map(|number| json!({"number":number,"url":repository.map(|repo| format!("https://github.com/{repo}/pull/{number}"))}));
    let merge = landing
        .filter(|l| l.get("status").and_then(Value::as_str) == Some("merged"))
        .and_then(|l| l.get("landed_commit"))
        .and_then(Value::as_str)
        .filter(|sha| (7..=64).contains(&sha.len()) && sha.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(|sha| json!({"commit":sha,"url":repository.map(|repo| format!("https://github.com/{repo}/commit/{sha}"))}));
    let status = delivery
        .as_ref()
        .and_then(|d| d.get("delivery_status"))
        .and_then(Value::as_str);
    json!({"task":task,"title":title,"workspace":workspace,"delivery_status":status,"pr":pr,"merge":merge,"cites":[task]})
}

/// An `owner/name` repository, the only shape a link is built from.
fn repository(repo: &str) -> bool {
    let part = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    matches!(repo.split_once('/'), Some((owner, name)) if part(owner) && part(name))
}

/// Open human decision requests on the tasks filed from the source.
fn open_decisions(call: &Call<'_>, found: &Filed) -> Result<Vec<Value>> {
    if found.tasks.is_empty() {
        return Ok(Vec::new());
    }
    let workspace = found.workspace.as_deref();
    let response = call
        .ports
        .orbit
        .decision_list(&request(json!({}), workspace)?)?;
    let requests = response
        .get("requests")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("decision list"))?;
    let mut out = Vec::new();
    for item in requests {
        let Some(task) = item.get("task_id").and_then(Value::as_str) else {
            continue;
        };
        let ours = found
            .tasks
            .iter()
            .any(|t| t.get("id").and_then(Value::as_str) == Some(task));
        let open = item
            .get("state")
            .and_then(Value::as_str)
            .is_none_or(|state| state == "open");
        let Some(request_id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !ours || !open {
            continue;
        }
        let question = ["question", "summary", "topic"]
            .iter()
            .find_map(|key| item.get(*key).and_then(Value::as_str))
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "A decision is waiting on this task".to_owned());
        out.push(json!({"kind":"decision","text":question,"task":task,"workspace":workspace,"cites":[request_id,task]}));
    }
    Ok(out)
}

fn report(title: &str, decided: &[Value], shipped: &[Value], needs_you: &[Value]) -> String {
    let cites = |item: &Value| {
        item["cites"]
            .as_array()
            .map(|c| {
                c.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = format!("# Since {title}\n\n## Decided\n\n");
    for item in decided {
        out.push_str(&format!(
            "- {} [{}]\n",
            item["text"].as_str().unwrap_or(""),
            cites(item)
        ));
    }
    if decided.is_empty() {
        out.push_str("- None.\n");
    }
    out.push_str("\n## Shipped\n\n");
    for item in shipped {
        let mut evidence = Vec::new();
        if let Some(number) = item["pr"]["number"].as_u64() {
            evidence.push(match item["pr"]["url"].as_str() {
                Some(url) => format!("[PR #{number}]({url})"),
                None => format!("PR #{number}"),
            });
        }
        if let Some(sha) = item["merge"]["commit"].as_str() {
            let short = sha.get(..7).unwrap_or(sha);
            evidence.push(match item["merge"]["url"].as_str() {
                Some(url) => format!("merged [{short}]({url})"),
                None => format!("merged {short}"),
            });
        }
        if evidence.is_empty() {
            evidence.push("no delivery record".into());
        }
        out.push_str(&format!(
            "- {}: {} [{}]\n",
            item["title"].as_str().unwrap_or(""),
            evidence.join(", "),
            cites(item)
        ));
    }
    if shipped.is_empty() {
        out.push_str("- None.\n");
    }
    out.push_str("\n## Needs you\n\n");
    for item in needs_you {
        let label = match item["kind"].as_str() {
            Some("contradiction") => "Contradiction",
            Some("decision") => "Decision",
            _ => "To-do",
        };
        out.push_str(&format!(
            "- {label}: {} [{}]\n",
            item["text"].as_str().unwrap_or(""),
            cites(item)
        ));
    }
    if needs_you.is_empty() {
        out.push_str("- None.\n");
    }
    out
}
