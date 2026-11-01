use super::library_support::{DOMAIN, Env};

#[test]
fn status_counts_off_template_pages_and_names_a_broken_template() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    let quiet = env.tool("office.status", serde_json::json!({}));
    assert_eq!(quiet["ok"], serde_json::json!(true), "{quiet}");
    assert_eq!(
        quiet["output"]["library"]["pages_off_template"],
        serde_json::json!(0)
    );
    assert_eq!(
        quiet["output"]["library"]["template_invalid"],
        serde_json::json!([])
    );

    let library = env.library();
    std::fs::write(
        library.join("knowledge/domain/ada.md"),
        "---\nid: dom-person-ada\ntype: domain\ntemplate: person\ntitle: Ada\n---\n\n## Role\nKeeps the notes.\n",
    )
    .unwrap();
    std::fs::write(
        library.join("knowledge/domain/bev.md"),
        "---\nid: dom-person-bev\ntype: domain\ntemplate: person\ntitle: Bev\n---\n\n## History\nNo role.\n",
    )
    .unwrap();
    std::fs::write(
        library.join("knowledge/domain/free.md"),
        "---\nid: dom-free\ntype: domain\ntitle: Free\n---\n\n## Role\nUnbound.\n",
    )
    .unwrap();
    std::fs::write(
        library.join(".office/templates/notes.md"),
        "not a template\n",
    )
    .unwrap();
    let reindex = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(
        reindex["output"]["indexed_documents"],
        serde_json::json!(3),
        "{reindex}"
    );
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(true), "{status}");
    assert!(status.get("error").is_none());
    assert_eq!(
        status["output"]["library"]["initialized"],
        serde_json::json!(true)
    );
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(3)
    );
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["domain"],
        serde_json::json!(3)
    );
    assert_eq!(
        status["output"]["library"]["pages_off_template"],
        serde_json::json!(1)
    );
    assert_eq!(
        status["output"]["library"]["template_invalid"],
        serde_json::json!([".office/templates/notes.md"])
    );
    assert_eq!(
        status["output"]["secrets"]["meta_api_key"],
        serde_json::json!("unset")
    );
    assert_eq!(status["output"]["version"], serde_json::json!("0.1.0"));
}

#[test]
fn status_reports_an_invalid_config_as_its_typed_error_not_as_uninitialized() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    std::fs::write(env.library().join(".office/config.yaml"), "bogus: true\n").unwrap();

    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(false), "{status}");
    assert_eq!(
        status["error"]["code"],
        serde_json::json!("invalid_library_setting"),
        "{status}"
    );
    assert!(
        status["error"]["message"]
            .as_str()
            .is_some_and(|m| m.contains("bogus")),
        "{status}"
    );
    assert!(status.get("output").is_none(), "{status}");
}

#[test]
fn status_on_an_absent_library_reports_uninitialized_without_error_or_state() {
    let env = Env::new();
    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(true), "{status}");
    assert!(status.get("error").is_none(), "{status}");
    assert_eq!(
        status["output"]["library"]["initialized"],
        serde_json::json!(false)
    );
    assert_eq!(
        status["output"]["library"]["documents"],
        serde_json::json!(0)
    );
    assert_eq!(
        status["output"]["library"]["documents_by_kind"]["source"],
        serde_json::json!(0)
    );
    assert!(!env.library().exists(), "status created the Library root");
}

#[test]
fn status_reports_an_unreplayable_write_journal_as_its_typed_error_and_keeps_it() {
    let env = Env::new();
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    // A journal from a newer backend version is never replayed: status refuses
    // with its typed error and leaves the pending decision in place.
    let journal = env.state.join("library.pending-write.json");
    let text = serde_json::json!({
        "version": 2,
        "root": env.library().canonicalize().expect("Library root"),
        "relative": "knowledge/domain/recovered.md",
        "text": DOMAIN,
        "token": "123-456"
    })
    .to_string();
    std::fs::write(&journal, &text).expect("interrupted write decision");

    let status = env.tool("office.status", serde_json::json!({}));
    assert_eq!(status["ok"], serde_json::json!(false), "{status}");
    assert_eq!(
        status["error"]["code"],
        serde_json::json!("library_journal_invalid"),
        "{status}"
    );
    assert!(status.get("output").is_none(), "{status}");
    assert_eq!(
        std::fs::read_to_string(&journal).expect("kept journal"),
        text
    );
}
