//! The status handler: secret presence is reported, never a value.

use crate::registry::Call;
use crate::tools::status::run;
use serde_json::{Map, json};

#[test]
fn status_reports_set_and_unset_secrets_without_values() {
    let input = Map::new();
    let mut context = Map::new();
    context.insert(
        "secrets".to_string(),
        json!({ "meta_api_key": "do-not-echo-this" }),
    );
    let output = run(&Call {
        ports: &office_testkit::fakes::PortsBuilder::default().build(),
        input: &input,
        context: &context,
    })
    .expect("status succeeds");
    assert_eq!(output["secrets"]["meta_api_key"], json!("set"));
    assert_eq!(output["secrets"]["baseten_token"], json!("unset"));
    assert!(
        !output.to_string().contains("do-not-echo-this"),
        "a secret value must never appear in output"
    );
}
