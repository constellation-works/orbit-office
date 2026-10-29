//! Library listing and history through the built binary and real store.

use serde_json::json;

use super::{Env, git};

#[test]
fn private_bodies_and_snippets_are_withheld_through_the_binary() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let secret = "quixotic-private-marker-7f3";
    let captured = env.tool(
        "office.capture",
        json!({"kind":"note","title":"Private note","text":format!("Body holds {secret}."),"sensitivity":"private"}),
    );
    assert_eq!(captured["ok"], true, "{captured}");
    let id = captured["output"]["id"].clone();
    assert_eq!(captured["output"]["sensitivity"], "private", "{captured}");

    // source-show: metadata returns, the body text never does.
    let shown = env.tool("office.source-show", json!({"id":id}));
    assert_eq!(shown["ok"], true, "{shown}");
    assert_eq!(shown["output"]["sensitivity"], "private", "{shown}");
    assert_eq!(shown["output"]["content_withheld"], true, "{shown}");
    assert_eq!(shown["output"]["body"], "", "{shown}");
    assert_eq!(shown["output"]["text"], "", "{shown}");
    assert_eq!(shown["output"]["span"], serde_json::Value::Null, "{shown}");
    assert!(
        !shown.to_string().contains(secret),
        "private body leaked: {shown}"
    );
    // An anchored read withholds the span too.
    let anchored = env.tool("office.source-show", json!({"id":id,"anchor":"#L1"}));
    assert_eq!(anchored["ok"], true, "{anchored}");
    assert!(
        !anchored.to_string().contains(secret),
        "private span leaked: {anchored}"
    );

    // search: the hit is listed by metadata, its snippet carries no body text.
    let found = env.tool(
        "office.search",
        json!({"query":"quixotic-private-marker-7f3"}),
    );
    assert_eq!(found["ok"], true, "{found}");
    assert_eq!(found["output"]["count"], 1, "{found}");
    let hit = &found["output"]["hits"][0];
    assert_eq!(hit["id"], id, "{found}");
    assert_eq!(hit["sensitivity"], "private", "{found}");
    assert_eq!(hit["content_withheld"], true, "{found}");
    assert_eq!(hit["snippet"], "", "{found}");
    assert!(
        !found.to_string().contains(secret),
        "private snippet leaked: {found}"
    );

    // knowledge-show: a private page returns metadata without its body.
    let page_secret = "page-private-marker-9b1";
    let written = env.tool(
        "office.knowledge-write",
        json!({"markdown":format!("---\nid: dom-topic-private\ntemplate: topic\ntitle: Private topic\nsummary: A page\nsensitivity: private\n---\n## Overview\nBody holds {page_secret}.\n")}),
    );
    assert_eq!(written["ok"], true, "{written}");
    let page = env.tool("office.knowledge-show", json!({"id":"dom-topic-private"}));
    assert_eq!(page["ok"], true, "{page}");
    assert_eq!(page["output"]["content_withheld"], true, "{page}");
    assert_eq!(page["output"]["page"]["body"], "", "{page}");
    assert_eq!(page["output"]["page"]["sensitivity"], "private", "{page}");
    assert!(
        !page.to_string().contains(page_secret),
        "private page body leaked: {page}"
    );

    // Control: a normal source still returns its body and snippet.
    let open = env.tool(
        "office.capture",
        json!({"kind":"note","title":"Open note","text":"Open quixotic body.","sensitivity":"normal"}),
    );
    assert_eq!(open["ok"], true, "{open}");
    let open_id = open["output"]["id"].clone();
    let open_shown = env.tool("office.source-show", json!({"id":open_id}));
    assert_eq!(
        open_shown["output"]["body"], "Open quixotic body.",
        "{open_shown}"
    );
    assert!(
        open_shown["output"].get("content_withheld").is_none(),
        "{open_shown}"
    );
    let open_found = env.tool("office.search", json!({"query":"quixotic","kind":"source"}));
    let open_hit = open_found["output"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hit| hit["id"] == open_id)
        .expect("normal hit listed");
    assert!(
        open_hit["snippet"].as_str().unwrap().contains("Open"),
        "normal snippet missing: {open_found}"
    );
    assert!(open_hit.get("content_withheld").is_none(), "{open_found}");
}

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
        json!({"pages":[{"layer":"own","shared_by":null,"id":"dom-topic-history","template":"topic","title":"Latest","summary":"A page","status":"current","sensitivity":"normal","shareable":false}],"missing_templates":[]})
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
