//! Filing extracted to-dos: yours in the office workspace, and, when
//! `meetings.agent_workspace` is set, agent-owned engineering to-dos as
//! ordinary code-change tasks in that workspace.
use super::ledger::Hold;
use super::{Call, Error, Result, Value, invalid_reply, json, object};

/// The tag on an agent code to-do held for a contested decision.
pub(crate) const HELD_TAG: &str = "contested";
/// Held to-do description line naming the contested entry it acts on.
pub(crate) const ACTS_ON: &str = "Acts on: ";
/// Held to-do description line naming the conflict to-do holding it.
pub(crate) const HELD_UNTIL: &str = "Held until ";

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

/// The `orbit.task.add` input for one extracted item.
pub(super) fn prepare(
    id: &str,
    profile: &str,
    item: &Value,
    agent_workspace: Option<&str>,
    hold: Option<&Hold>,
) -> Result<Value> {
    let text = item["text"].as_str().ok_or_else(invalid_reply)?;
    if let Some(workspace) = agent_workspace
        && hold.is_none()
        && let Some((kind, criteria)) = agent_code(item)
    {
        let input = json!({"workspace":workspace,"title":text,
            "description":agent_description(id, item),
            "type":kind,"complexity":"medium","origin":"automation",
            "tags":["owner:agent",format!("source:{id}")],"acceptance_criteria":criteria});
        return Ok(input);
    }
    let mut tags = vec!["owner:you".to_string(), format!("source:{id}")];
    if profile == "kt" {
        tags.push("kt-question".into());
    }
    let description = format!("From {id}: {}", item["cites"]);
    if let Some(hold) = hold
        && agent_workspace.is_some()
        && let Some((kind, criteria)) = agent_code(item)
    {
        // Held, not dropped: `office.decision-resolve` files it for agents
        // when the decision it acts on stands. The last two lines are read
        // back by that tool.
        tags.push(HELD_TAG.into());
        let input = json!({"title":text,
            "description":format!("{}\n\nHeld from agents: it acts on a contested decision.\n\n{ACTS_ON}{}\n{HELD_UNTIL}{} is resolved.", agent_description(id, item), hold.entry, hold.conflict),
            "type":kind,"complexity":"low","origin":"automation","tags":tags,"acceptance_criteria":criteria});
        return Ok(input);
    }
    Ok(
        json!({"title":text,"description":description,"type":"chore","complexity":"low","origin":"automation","tags":tags}),
    )
}

/// File one prepared item. The reply is Orbit's on success; the outcome
/// always says what happened to the item, so a failure mid-batch leaves the
/// rest filed and this one reconcilable.
pub(super) fn file(call: &Call<'_>, index: usize, input: &Value) -> (Option<Value>, Value) {
    let mut outcome =
        json!({"index":index,"title":input["title"],"workspace":input.get("workspace")});
    let reply = object(input).and_then(|input| call.ports.orbit.task_add(input));
    match reply {
        Ok(reply) => {
            outcome["status"] = json!(FILED);
            outcome["task"] = reply["id"].clone();
            (Some(reply), outcome)
        }
        Err(error) => {
            failed(&mut outcome, &error);
            (None, outcome)
        }
    }
}

/// An outcome's status once its task exists.
pub(super) const FILED: &str = "filed";

/// Record why an item is not filed. `outcome_unknown` may have committed;
/// reconcile looks for it before filing again.
fn failed(outcome: &mut Value, error: &Error) {
    let (status, code) = if let Error::OrbitRefused { code } = error {
        ("refused", code.clone())
    } else if matches!(error, Error::OrbitOutcomeUnknown) {
        ("outcome_unknown", error.code().to_owned())
    } else {
        ("failed", error.code().to_owned())
    };
    outcome["status"] = json!(status);
    outcome["code"] = json!(code);
}

/// The `filing.json` sidecar: each item's outcome and hold, the ledger
/// report and the extraction provenance, enough to finish filing later
/// without transcribing, extracting or feeding the ledger again.
pub(super) fn record(
    outcomes: &[Value],
    holds: &[Option<&Hold>],
    ledger: &Value,
    derived: &Value,
) -> Value {
    let holds: Vec<Value> = holds
        .iter()
        .map(|hold| {
            hold.map_or(
                Value::Null,
                |h| json!({"entry":h.entry,"conflict":h.conflict}),
            )
        })
        .collect();
    let complete = outcomes.iter().all(|o| o["status"] == FILED);
    json!({"complete":complete,"items":outcomes,"holds":holds,"ledger":ledger,"derived_by":derived})
}

/// Holds as `filing.json` records them.
pub(super) fn holds_from(record: &Value) -> Vec<Option<Hold>> {
    record["holds"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|hold| {
            Some(Hold {
                entry: hold["entry"].as_str()?.to_owned(),
                conflict: hold["conflict"].as_str()?.to_owned(),
            })
        })
        .collect()
}

/// An existing task for this source with the item's title, in the item's
/// workspace: an `outcome_unknown` write that did commit, or a to-do filed
/// by hand from the returned input. Listing errors propagate: an unread
/// workspace never proves absence.
pub(super) fn existing(call: &Call<'_>, id: &str, input: &Value) -> Result<Option<Value>> {
    let mut query = json!({"tag":[format!("source:{id}")],"limit":1000,"fields":["id","title"]});
    if let Some(workspace) = input.get("workspace") {
        query["workspace"] = workspace.clone();
    }
    let listed = call.ports.orbit.task_list(object(&query)?)?;
    Ok(listed["tasks"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|task| task["title"] == input["title"])
        .cloned())
}

/// Finish one unfiled item: adopt its task when it exists, else file it.
pub(super) fn settle(
    call: &Call<'_>,
    id: &str,
    index: usize,
    input: &Value,
) -> (Option<Value>, Value) {
    match existing(call, id, input) {
        Ok(Some(task)) => {
            let outcome = json!({"index":index,"title":input["title"],"workspace":input.get("workspace"),
                "status":FILED,"task":task["id"],"reconciled":true});
            (Some(task), outcome)
        }
        Ok(None) => file(call, index, input),
        Err(error) => {
            let mut outcome =
                json!({"index":index,"title":input["title"],"workspace":input.get("workspace")});
            failed(&mut outcome, &error);
            (None, outcome)
        }
    }
}

/// An agent code task's description: what it is and each cited anchor.
fn agent_description(id: &str, item: &Value) -> String {
    let cited = item["cites"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|anchor| format!("- {id}{anchor}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("A code change the meeting {id} assigned to an agent.\n\nCited:\n{cited}")
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
