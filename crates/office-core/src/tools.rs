//! Tool handlers. Each takes validated input and the call context and
//! returns its output payload.

use serde_json::{Map, Value, json};

use crate::VERSION;

/// The names `spec.secrets` declares in plugin.yaml; tests hold the two equal.
pub const SECRET_NAMES: [&str; 2] = ["meta_api_key", "baseten_token"];

/// `office.status`: the plugin version and which declared secrets are set;
/// never a secret value (STD-05: secrets never cross the tool boundary out).
///
/// Filesystem and index health join this output with the `reindex` slice,
/// once the Library layout exists to check.
pub fn status(_input: &Map<String, Value>, context: &Map<String, Value>) -> Value {
    let present: Vec<&str> = context
        .get("secrets")
        .and_then(Value::as_object)
        .map(|secrets| secrets.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let secrets: Map<String, Value> = SECRET_NAMES
        .iter()
        .map(|name| {
            (
                (*name).to_string(),
                json!(if present.contains(name) {
                    "set"
                } else {
                    "unset"
                }),
            )
        })
        .collect();
    json!({
        "version": VERSION,
        "secrets": secrets,
    })
}
