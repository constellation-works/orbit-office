//! Settings through the public, pure parsing/deserialization boundary.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::{Error, domain::settings::Settings};
use serde_json::json;

const BLOCK: &str = "open_weights:\n  endpoint: https://muse.internal.acme.example/v1\n  model: meta-models/Muse-Spark-Open-816B-A42B\n  kind: company\n  label: Acme's server\n";

fn block_value() -> serde_json::Value {
    json!({
        "endpoint": "https://muse.internal.acme.example/v1",
        "model": "meta-models/Muse-Spark-Open-816B-A42B",
        "kind": "company",
        "label": "Acme's server",
    })
}

fn refused_key(result: office_core::Result<Settings>) -> String {
    let error = result.expect_err("refused");
    let Error::InvalidSetting { key, .. } = error else {
        panic!("wrong error: {error:?}")
    };
    key
}

mod endpoints;
mod route_check;
mod shared;
mod updates;
mod yaml;
