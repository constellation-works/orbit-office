use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who made a decision (D13).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DecidedBy {
    /// A user's decision.
    Human,
    /// An agent's decision.
    Agent,
    /// A decision model's answer, written only by the precedent feedback path.
    Model,
}

/// One verbatim span and the ref it was read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cited {
    /// The verbatim text.
    pub text: String,
    /// Where it was said.
    #[serde(rename = "ref")]
    pub reference: String,
    /// The option's stable id; set only by Orbit ingestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The option's one-line consequence; set only by Orbit ingestion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consequence: Option<String>,
}

/// The model citation on an ingested entry answered by the model.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrbitModel {
    /// The assessment id.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "assessment")]
    pub assessment_id: Option<String>,
    /// The policy digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_digest: Option<String>,
    /// The model identity.
    #[serde(default, skip_serializing_if = "Option::is_none", alias = "identity")]
    pub model: Option<String>,
    /// The model's probability, kept as its written decimal text.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "de_probability",
        serialize_with = "se_probability"
    )]
    pub probability: Option<String>,
    /// The precedents the model cited.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "cited",
        alias = "precedents"
    )]
    pub cited_precedents: Option<Vec<String>>,
}

fn de_probability<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(text),
        Some(Value::Number(number)) => Some(number.to_string()),
        Some(other) => Some(other.to_string()),
    })
}

fn se_probability<S>(value: &Option<String>, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        None => serializer.serialize_none(),
        Some(text) => match text.parse::<serde_json::Number>() {
            Ok(number) => serializer.serialize_some(&Value::Number(number)),
            Err(_) => serializer.serialize_some(&Value::String(text.clone())),
        },
    }
}

/// The Orbit request an ingested entry records; ingestion only.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrbitInfo {
    /// The answered request's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The request's gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// The request's category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The request's topic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub topic: Option<String>,
    /// Who answered the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered_by: Option<String>,
    /// The channel the answer arrived on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// The model citation, when the model answered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<OrbitModel>,
}

/// The options considered: listed verbatim, or `"unrecoverable"`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Value", into = "Value")]
pub enum Options {
    /// The turns do not state the options; never invented.
    Unrecoverable,
    /// Each option verbatim with its ref.
    Listed(Vec<Cited>),
}

impl TryFrom<Value> for Options {
    type Error = String;
    fn try_from(value: Value) -> std::result::Result<Self, String> {
        match value {
            Value::String(text) if text == "unrecoverable" => Ok(Self::Unrecoverable),
            Value::Array(_) => serde_json::from_value(value)
                .map(Self::Listed)
                .map_err(|_| "options items must be {text, ref}".into()),
            Value::Null
            | Value::Bool(_)
            | Value::Number(_)
            | Value::String(_)
            | Value::Object(_) => {
                Err("options must be \"unrecoverable\" or a list of {text, ref}".into())
            }
        }
    }
}

impl From<Options> for Value {
    fn from(options: Options) -> Self {
        match options {
            Options::Unrecoverable => Value::String("unrecoverable".into()),
            Options::Listed(items) => Value::Array(
                items
                    .into_iter()
                    .map(|item| {
                        let mut object =
                            serde_json::json!({"text": item.text, "ref": item.reference});
                        if let Some(id) = item.id {
                            object["id"] = Value::String(id);
                        }
                        if let Some(consequence) = item.consequence {
                            object["consequence"] = Value::String(consequence);
                        }
                        object
                    })
                    .collect(),
            ),
        }
    }
}

/// The session's Orbit assignment; each field null when unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    /// Orbit task type.
    pub task_type: Option<String>,
    /// Orbit workspace id.
    pub workspace: Option<String>,
    /// Orbit task id.
    pub task: Option<String>,
    /// Risk tier.
    pub risk_tier: Option<String>,
}
