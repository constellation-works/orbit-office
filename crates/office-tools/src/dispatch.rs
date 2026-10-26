//! Envelope in, envelope out: find the tool, refuse unknown input keys, run
//! the handler. The one chokepoint every call crosses (STD-02 R24).

use office_core::protocol::{self, Request};
use office_core::{Error, NAMESPACE, Result};
use serde_json::Value;

use crate::registry::{Call, tools};

/// Answer one raw request with one serialized response envelope.
///
/// ```
/// let response = office_tools::serve(r#"{"schema_version": 1, "tool": "office.nope"}"#);
/// let response: serde_json::Value = serde_json::from_str(&response).unwrap();
/// assert_eq!(response["error"]["code"], "unknown_tool");
/// ```
#[must_use]
pub fn serve(raw: &str) -> String {
    protocol::respond(protocol::parse(raw).and_then(|request| dispatch(&request)))
}

fn dispatch(request: &Request) -> Result<Value> {
    let tool = request.tool.as_str();
    let prefix = format!("{NAMESPACE}.");
    let Some(verb) = tool.strip_prefix(&prefix) else {
        return Err(Error::UnknownTool(format!(
            "{tool:?} (this backend serves only {NAMESPACE}.* tools)"
        )));
    };
    let Some(entry) = tools().iter().find(|entry| entry.spec().name == verb) else {
        return Err(Error::UnknownTool(format!(
            "{tool} is not a {NAMESPACE} tool"
        )));
    };
    let Some(handler) = entry.handler() else {
        return Err(Error::UnknownTool(format!(
            "{tool} is not available in this version"
        )));
    };

    let accepted = entry.accepts();
    let mut unknown: Vec<&str> = request
        .input
        .keys()
        .map(String::as_str)
        .filter(|key| !accepted.contains(key))
        .collect();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        return Err(Error::InvalidArgument(format!(
            "{tool} does not accept: {}",
            unknown.join(", ")
        )));
    }

    handler(&Call {
        input: &request.input,
        context: &request.context,
    })
}
