//! Filing extracted to-dos: yours in the office workspace, and, when
//! `meetings.agent_workspace` is set, agent-owned engineering to-dos as
//! ordinary code-change tasks in that workspace.
use super::{Call, Result, Value, invalid_reply, json, object};

/// The task types an agent code to-do may carry.
const CODE_TYPES: [&str; 3] = ["feature", "bug", "chore"];

/// Extra to-do properties the meeting schema asks for when agent filing is on.
pub(super) fn split_properties(item: &mut Value) {
    let text = json!({"type":"string"});
    item["properties"]["work"] = json!({"enum":["code","other"]});
    item["properties"]["type"] = json!({"enum":CODE_TYPES});
    item["properties"]["acceptance_criteria"] = json!({"type":"array","items":text});
    item["required"] = json!([
        "text",
        "cites",
        "owner",
        "work",
        "type",
        "acceptance_criteria"
    ]);
}

/// Prompt addition for the split schema. The timeline stays data.
pub(super) const SPLIT_PROMPT: &str = "For each to-do, set owner agent only when the meeting assigned it to an agent. Set work code only for a change to a code repository, otherwise other. For code work choose type feature, bug or chore, and list acceptance criteria taken only from what the meeting said.\n";

/// Hook for the decision ledger: which to-dos touch a contested matter, in
/// `to_dos` order. A contested to-do is never filed for agents.
pub(super) fn contested(_call: &Call<'_>, _id: &str, result: &Value) -> Result<Vec<bool>> {
    Ok(vec![false; super::list(result, "to_dos")?.len()])
}

/// File one extracted item and return Orbit's reply.
pub(super) fn file(
    call: &Call<'_>,
    id: &str,
    profile: &str,
    item: &Value,
    agent_workspace: Option<&str>,
    contested: bool,
) -> Result<Value> {
    let text = item["text"].as_str().ok_or_else(invalid_reply)?;
    if let Some(workspace) = agent_workspace
        && !contested
        && let Some((kind, criteria)) = agent_code(item)
    {
        let cited = item["cites"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|anchor| format!("- {id}{anchor}"))
            .collect::<Vec<_>>()
            .join("\n");
        let input = json!({"workspace":workspace,"title":text,
            "description":format!("A code change the meeting {id} assigned to an agent.\n\nCited:\n{cited}"),
            "type":kind,"complexity":"medium","origin":"automation",
            "tags":["owner:agent",format!("source:{id}")],"acceptance_criteria":criteria});
        return call.ports.orbit.task_add(object(&input)?);
    }
    let mut tags = vec!["owner:you".to_string(), format!("source:{id}")];
    if profile == "kt" {
        tags.push("kt-question".into());
    }
    let mut description = format!("From {id}: {}", item["cites"]);
    if contested && agent_workspace.is_some() && agent_code(item).is_some() {
        tags.push("contested".into());
        description.push_str("\n\nHeld from agents: it touches a contested matter.");
    }
    let input = json!({"title":text,"description":description,"type":"chore","complexity":"low","origin":"automation","tags":tags});
    call.ports.orbit.task_add(object(&input)?)
}

/// An agent-owned code to-do's type and acceptance criteria. Anything less
/// complete stays with you: an agent task needs criteria to be judged by.
fn agent_code(item: &Value) -> Option<(&str, Vec<&str>)> {
    if item["owner"] != "agent" || item["work"] != "code" {
        return None;
    }
    let kind = item["type"]
        .as_str()
        .filter(|kind| CODE_TYPES.contains(kind))?;
    let criteria = item["acceptance_criteria"]
        .as_array()?
        .iter()
        .map(|c| c.as_str().map(str::trim).filter(|c| !c.is_empty()))
        .collect::<Option<Vec<_>>>()
        .filter(|c| !c.is_empty())?;
    Some((kind, criteria))
}
