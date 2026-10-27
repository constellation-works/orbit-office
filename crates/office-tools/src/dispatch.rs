//! Envelope in, envelope out: find the tool, refuse unknown input keys, run
//! the handler. The one chokepoint every call crosses (STD-02 R24).

use office_core::protocol::{self, Request};
use office_core::{Error, NAMESPACE, Result};
use serde_json::Value;

use crate::registry::{Call, tools};

/// Dispatch a parsed request using the supplied services and render its response.
#[must_use]
pub fn serve(request: &Request, ports: &office_core::ports::Ports<'_>) -> String {
    protocol::respond(dispatch(request, ports))
}

fn dispatch(request: &Request, ports: &office_core::ports::Ports<'_>) -> Result<Value> {
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

    // The one Library-readiness check (STD-02 R24): every built tool other
    // than `init` (which creates the Library) and `status` (which reports
    // its health) refuses an uninitialized Library here, in the shared
    // path, rather than once per handler.
    if verb != "init" && verb != "status" {
        match ports.library.is_initialized() {
            Ok(true) => {}
            Ok(false) => return Err(office_core::ports::not_initialized_error()),
            Err(error) => return Err(error),
        }
    }

    handler(&Call {
        ports,
        input: &request.input,
        context: &request.context,
    })
}
