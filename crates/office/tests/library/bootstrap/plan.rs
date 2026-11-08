use super::super::library_support::Env;

#[test]
fn batch_plan_exec_persists_a_digest_and_confirms_across_processes() {
    use serde_json::json;
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let source = env.tool("office.capture",json!({"kind":"doc","title":"Bootstrap design","text":"# Ingest\nDesign context.","tags":["bootstrap:docs"]}));
    assert_eq!(source["ok"], true, "{source}");
    let response = env.tool("office.batch-plan", json!({"phase":"docs"}));
    assert_eq!(response["ok"], true, "{response}");
    let batch = &response["output"];
    assert_eq!(batch["plan"]["counts"]["digest"], 1);
    assert_eq!(batch["plan"]["counts"]["careful"], 1);
    assert!(batch["plan"]["estimate"].is_null());
    let ledger: serde_json::Value = serde_json::from_slice(
        &std::fs::read(env.library().join(".office/bootstrap.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(ledger["docs"], *batch);
    let refused = env.tool(
        "office.batch-plan",
        json!({"phase":"docs","confirm":true,"digest":"old"}),
    );
    assert_eq!(refused["error"]["code"], "plan_changed");
    let confirmed = env.tool(
        "office.batch-plan",
        json!({"phase":"docs","confirm":true,"digest":batch["plan"]["digest"]}),
    );
    assert_eq!(confirmed["ok"], true, "{confirmed}");
    assert!(confirmed["output"]["confirmed_at"].is_string());
}
