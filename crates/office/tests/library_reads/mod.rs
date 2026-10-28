//! Library listing and history through the built binary and real store.

use serde_json::json;

use super::{Env, git};

#[test]
fn list_and_show_history_read_committed_documents_without_writing() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    for title in ["First", "Latest"] {
        let response = env.tool("office.knowledge-write", json!({"markdown":format!("---\nid: dom-topic-history\ntemplate: topic\ntitle: {title}\nsummary: A page\n---\n## Overview\nA useful note.\n")}));
        assert_eq!(response["ok"], true, "{response}");
    }
    let captured = env.tool(
        "office.capture",
        json!({"kind":"chat","title":"Thread","text":"one\ntwo"}),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let head = git(&env.library(), &["rev-parse", "HEAD"]);
    let status = git(&env.library(), &["status", "--porcelain"]);
    let listed = env.tool("office.knowledge-list", json!({"template":"topic"}));
    assert_eq!(listed["ok"], true, "{listed}");
    assert_eq!(
        listed["output"],
        json!({"pages":[{"layer":"own","shared_by":null,"id":"dom-topic-history","template":"topic","title":"Latest","summary":"A page","status":"current","sensitivity":"normal"}],"missing_templates":[]})
    );
    let absent = env.tool("office.knowledge-list", json!({"template":"systems"}));
    assert_eq!(
        absent["output"]["missing_templates"][0]["id"],
        "dom-systems"
    );
    for (tool, id, count) in [
        ("office.knowledge-show", json!("dom-topic-history"), 2),
        ("office.source-show", captured["output"]["id"].clone(), 1),
    ] {
        let plain = env.tool(tool, json!({"id":id}));
        assert_eq!(plain["ok"], true, "{plain}");
        assert!(plain["output"].get("history").is_none());
        let mut shown = env.tool(tool, json!({"id":id,"history":true}));
        assert_eq!(shown["ok"], true, "{shown}");
        let history = shown["output"]
            .as_object_mut()
            .unwrap()
            .remove("history")
            .unwrap();
        assert_eq!(shown, plain);
        assert_eq!(history["total"], count);
        assert_eq!(history["truncated"], false);
        let path = if tool == "office.knowledge-show" {
            plain["output"]["page"]["path"].as_str().unwrap()
        } else {
            plain["output"]["path"].as_str().unwrap()
        };
        let commits = git(&env.library(), &["log", "--format=%H", "--", path]);
        let returned: Vec<_> = history["revisions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["commit"].as_str().unwrap())
            .collect();
        assert_eq!(returned, commits.lines().collect::<Vec<_>>());
        assert!(history["revisions"][0]["time"].as_str().is_some());
        assert!(history["revisions"][0]["summary"].as_str().is_some());
    }
    assert_eq!(git(&env.library(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&env.library(), &["status", "--porcelain"]), status);
}
