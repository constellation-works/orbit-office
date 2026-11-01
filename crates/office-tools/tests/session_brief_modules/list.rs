use super::*;

/// Four conversations: superseded/resolved/current project handoffs; open,
/// expired and resolved globals; a private source; one structured handoff.
fn list_library() -> FakeLibrary {
    let l = FakeLibrary::initialized();
    let add = |spec: &str| one(&l, spec);
    add(
        "src-a|3008|orbit-office/claude|09 handoff: Office next: first plan.\n09:01 handoff: (global) Upgrade orbit to 0.9.\n09:02 handoff: (global until 2026-10-30T10:00:00Z) Drain paused.\n09:03 handoff: (global) Old defect.\n09:04 handoff: Office next: review ORB-41001.\n",
    );
    add(
        "src-b|3010|/home/me/work/orbit/codex|10:01 resolved: src-a#L4 fixed in orbit 0.9.1.\n10:02 handoff: Orbit next: cut 0.9.2.\n10:03 handoff: Orbit release in progress.\n10:04 resolved: #L3 released.\n",
    );
    l.write_document(Path::new("sources/2026/10/src-c/source.md"), "---\nid: src-c\nkind: conversation\ntitle: Private planning\ncaptured_at: \"2026-10-30T11:00:00Z\"\nsensitivity: private\nproject: orbit-office\norchestrator: claude\nsession_id: session-src-c\n---\n[2026-10-30T11:00:00Z] handoff: Secret plan for ORB-41002.\n").unwrap();
    source_in(
        &l,
        "src-d",
        "2026-10-30T11:30:00Z",
        "orbit-office",
        "claude",
        &format!(
            "[2026-10-30T11:30:00Z] handoff: (structured) {}\n",
            json!({"objective":"Deliver the release for ORB-12345.","next":"Wait for the review report."})
        ),
    );
    l
}

fn listing(item: &Value) -> &Value {
    &item["listing"]
}

/// Every item and listing in the output uses only the shipped schema's keys.
fn assert_closed(response: &Value) {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../plugin/.orbit-plugin/schemas/session-brief.response.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let section_schema = &schema["properties"]["sections"]["items"];
    let item_schema = &section_schema["properties"]["items"]["items"];
    let listing_schema = &schema["$defs"]["listing"];
    for section in response["output"]["sections"].as_array().unwrap() {
        let kinds = section_schema["properties"]["kind"]["enum"]
            .as_array()
            .unwrap();
        assert!(kinds.contains(&section["kind"]), "{section}");
        for key in section.as_object().unwrap().keys() {
            assert!(section_schema["properties"].get(key).is_some(), "{key}");
        }
        for item in section["items"].as_array().unwrap() {
            for key in item.as_object().unwrap().keys() {
                assert!(item_schema["properties"].get(key).is_some(), "{key}");
            }
            let mut keys: Vec<&str> = item["listing"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let mut required: Vec<&str> = listing_schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .map(|k| k.as_str().unwrap())
                .collect();
            keys.sort_unstable();
            required.sort_unstable();
            assert_eq!(keys, required);
        }
    }
}

#[test]
fn list_view_returns_current_threads_globals_and_paged_filtered_history() {
    let (ports, orbit) = at(list_library(), "2026-10-30T12:00:00Z", vec![]);
    let response = brief(&ports, json!({"view": "list"}));
    expect_kinds(&response, "threads,global,history");
    // Current: each thread's newest handoff, newest first; orbit thread closed by resolved entry.
    expect_ids(&response, "threads|src-d#L1,src-c#L1,src-a#L5");
    let lead = &items(&response, "threads")[2];
    assert_eq!(
        listing(lead),
        &json!({"scope":"project","thread":"task:ORB-41001","project":"orbit-office","orchestrator":"claude","assignment":"ORB-41001","session_id":"session-src-a","source_title":"src-a","superseded":false,"state":"open","until":null,"resolved_by":null,"withheld":false})
    );
    assert_eq!(lead["legacy"], true);
    assert_eq!(lead["link"], "sources/2026/10/src-a/source.md#L5");
    // Private source: listed by reference only.
    let private = &items(&response, "threads")[1];
    assert_eq!(private["text"], "");
    assert!(private.get("legacy").is_none());
    assert_eq!(listing(private)["withheld"], true);
    assert_eq!(listing(private)["source_title"], "");
    assert_eq!(listing(private)["assignment"], "ORB-41002");
    assert!(
        !response.to_string().contains("Secret plan")
            && !response.to_string().contains("Private planning")
    );
    // Structured: fields in order, with the structured object.
    let structured = &items(&response, "threads")[0];
    assert_eq!(
        structured["structured_handoff"]["objective"],
        "Deliver the release for ORB-12345."
    );
    assert!(
        structured["text"]
            .as_str()
            .unwrap()
            .starts_with("Objective: Deliver the release for ORB-12345.\nNext: Wait")
    );
    expect_ids(&response, "global|src-a#L2");
    // History: every project handoff newest first, superseded ones marked.
    let history = items(&response, "history");
    expect_ids(
        &response,
        "history|src-d#L1,src-c#L1,src-b#L3,src-b#L2,src-a#L5,src-a#L1",
    );
    let superseded: Vec<bool> = history
        .iter()
        .map(|i| listing(i)["superseded"].as_bool().unwrap())
        .collect();
    assert_eq!(superseded, [false, false, false, true, false, true]);
    assert_eq!(listing(&history[2])["state"], "resolved");
    assert_eq!(listing(&history[2])["resolved_by"]["id"], "src-b#L4");
    assert_eq!(
        listing(&history[5])["thread"],
        "project:orbit-office orchestrator:claude"
    );
    expect_page(&response, "history|6|false|0");
    assert!(orbit.calls().is_empty(), "the list view reads no tasks");
    assert_closed(&response);
}

#[test]
fn list_view_reveals_closed_globals_with_their_closing_entry() {
    let response = brief_at(
        list_library(),
        "2026-10-30T12:00:00Z",
        vec![],
        json!({"view": "list", "include_closed": true}),
    );
    // brief_at requires replies; empty reply queue is fine as list reads no tasks.
    let globals = items(&response, "global");
    expect_ids(&response, "global|src-a#L4,src-a#L3,src-a#L2");
    let states: Vec<&str> = globals
        .iter()
        .map(|g| listing(g)["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["resolved", "expired", "open"]);
    assert_eq!(
        listing(&globals[0])["resolved_by"],
        json!({"id":"src-b#L1","link":"sources/2026/10/src-b/source.md#L1","text":"src-a#L4 fixed in orbit 0.9.1.","time":"2026-10-30T10:01:00Z"})
    );
    assert_eq!(listing(&globals[1])["until"], "2026-10-30T10:00:00Z");
    assert_eq!(listing(&globals[1])["scope"], "global");
    assert_eq!(listing(&globals[1])["thread"], Value::Null);
    assert_closed(&response);
}

#[test]
fn list_view_filters_and_pages_history_only() {
    let (ports, _) = at(list_library(), "2026-10-30T12:00:00Z", vec![]);
    for (input, expected) in [
        (json!({"project": "orbit"}), vec!["src-b#L3", "src-b#L2"]),
        (
            json!({"orchestrator": "codex"}),
            vec!["src-b#L3", "src-b#L2"],
        ),
        (
            json!({"thread": "project:orbit-office orchestrator:claude"}),
            vec!["src-a#L1"],
        ),
        (
            json!({"project": "orbit-office", "thread": "task:ORB-41001"}),
            vec!["src-a#L5"],
        ),
        (json!({"project": "nowhere"}), vec![]),
    ] {
        let mut request = input.clone();
        request["view"] = json!("list");
        let response = brief(&ports, request);
        assert_eq!(ids(&response, "history"), expected, "{input}");
        // Current and global are never filtered.
        assert_eq!(ids(&response, "threads").len(), 3, "{input}");
    }
    let response = brief(&ports, json!({"view": "list", "offset": 2, "limit": 2}));
    expect_ids(&response, "history|src-b#L3,src-b#L2");
    expect_page(&response, "history|6|true|2");
    let response = brief(&ports, json!({"view": "list", "offset": 4, "limit": 2}));
    assert_eq!(section(&response, "history").unwrap()["truncated"], false);
    for input in [
        json!({"view": "list", "limit": 0}),
        json!({"view": "list", "limit": 101}),
        json!({"view": "list", "offset": -1}),
        json!({"view": "list", "offset": 100_001}),
        json!({"view": "list", "include_closed": "yes"}),
        json!({"view": "list", "thread": ""}),
        json!({"view": "list", "session_start": true}),
        json!({"view": "handoffs", "limit": 5}),
        json!({"thread": "task:ORB-41001"}),
    ] {
        let response = call(&ports, input.clone());
        assert_eq!(
            response["error"]["code"], "invalid_argument",
            "{input}: {response}"
        );
    }
}
