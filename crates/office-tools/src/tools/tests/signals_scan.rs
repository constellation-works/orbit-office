//! `office.signals-scan` through dispatch against the Orbit and Library
//! fakes (STD-04): fixture histories produce the expected candidates.

use office_core::domain::settings::Settings;
use office_core::ports::{Library, LibraryText};
use office_core::{Error, Result};
use serde_json::{Map, Value, json};

use crate::registry::Call;
use crate::tools::proposals::signals_scan::run;

fn ports_with(
    tasks: Value,
    frictions: Value,
) -> (
    office_core::ports::Ports<'static>,
    office_testkit::fakes::FakeOrbit,
) {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(tasks));
    orbit.push_response(Ok(frictions));
    let observer = orbit.clone();
    (
        office_core::ports::Ports {
            scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
            library: Box::new(office_testkit::fakes::FakeLibrary::initialized()),
            models: Box::new(office_testkit::fakes::FakeModels::new()),
            orbit: Box::new(orbit),
            clock: Box::new(office_testkit::fakes::FakeClock(
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 40),
            )),
            selections: Box::new(office_testkit::fakes::FakeSelections::default()),
        },
        observer,
    )
}

fn call_run(ports: &office_core::ports::Ports<'_>) -> Value {
    let input = Map::new();
    let context = Map::new();
    run(&Call {
        ports,
        input: &input,
        context: &context,
    })
    .expect("scan succeeds")
}

#[test]
fn scan_returns_repetition_and_friction_candidates_with_evidence() {
    let tasks = json!({"tasks": [
        {"id": "t1", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 3},
        {"id": "t2", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 10},
        {"id": "t3", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 20},
        {"id": "t4", "title": "one off errand", "status": "done", "tags": ["owner:you"], "completed_days_ago": 2}
    ]});
    let frictions = json!({"hits": [
        {"record": {"id": "f1", "text": "deploy script fails on macos"}},
        {"record": {"id": "f2", "text": "deploy script fails again on macos"}}
    ]});
    let (ports, orbit) = ports_with(tasks, frictions);
    let output = call_run(&ports);
    assert_eq!(output["count"], json!(2));
    let signals: Vec<&str> = output["candidates"]
        .as_array()
        .expect("candidates")
        .iter()
        .map(|c| c["signal"].as_str().expect("signal"))
        .collect();
    assert_eq!(signals, vec!["friction_recurrence", "repetition"]);
    assert_eq!(
        output["candidates"][1]["evidence"],
        json!(["t1", "t2", "t3"])
    );
    assert_eq!(output["candidates"][0]["evidence"], json!(["f1", "f2"]));
    assert_eq!(output["candidates"][1]["kind"], "auto_task");
    // Both reads went through the Orbit port.
    let tools: Vec<&str> = orbit.calls().iter().map(|c| c.tool).collect();
    assert_eq!(tools, vec!["orbit.task.list", "orbit.search"]);
}

#[test]
fn scan_below_threshold_returns_no_candidates() {
    let tasks = json!({"tasks": [
        {"id": "t1", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 3}
    ]});
    let (ports, _) = ports_with(tasks, json!({"hits": []}));
    let output = call_run(&ports);
    assert_eq!(output["count"], json!(0));
    assert_eq!(output["candidates"], json!([]));
}

#[test]
fn scan_refuses_an_uninitialized_library() {
    let ports = office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library: Box::new(office_testkit::fakes::FakeLibrary::default()),
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(office_testkit::fakes::FakeOrbit::default()),
        clock: Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH,
        )),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    };
    let input = Map::new();
    let context = Map::new();
    let error = run(&Call {
        ports: &ports,
        input: &input,
        context: &context,
    })
    .expect_err("uninitialized library refuses");
    assert_eq!(error.code(), "library_not_initialized");
}

fn clock() -> office_testkit::fakes::FakeClock {
    office_testkit::fakes::FakeClock(
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 40),
    )
}

fn ports_for(
    orbit: office_testkit::fakes::FakeOrbit,
    library: Box<dyn Library>,
) -> office_core::ports::Ports<'static> {
    office_core::ports::Ports {
        scripts: Box::new(office_testkit::fakes::FakeScripts::default()),
        library,
        models: Box::new(office_testkit::fakes::FakeModels::new()),
        orbit: Box::new(orbit),
        clock: Box::new(clock()),
        selections: Box::new(office_testkit::fakes::FakeSelections::default()),
    }
}

/// `office.signals-scan` through the registered serve entry point.
fn scan(ports: &office_core::ports::Ports<'_>) -> Value {
    let raw = office_testkit::envelope::request("office.signals-scan", json!({}), json!({}));
    let request = office_core::protocol::parse(&raw).expect("request is valid");
    office_testkit::envelope::response(&crate::serve(&request, ports)).expect("response is JSON")
}

/// Full documents, including entries beyond a search excerpt and sources
/// ordered after more than one search page of knowledge documents.
fn library_signal_fixture() -> (office_testkit::fakes::FakeLibrary, Vec<String>) {
    use office_core::ports::{CaptureSourceRequest, ConversationEntry};
    use std::path::Path;

    let mut library = office_testkit::fakes::FakeLibrary::initialized();
    library.settings.thresholds.stale_days = 30;
    for index in 0..101 {
        library
            .write_document(
                Path::new(&format!("knowledge/domain/filler-{index}.md")),
                &format!("---\nid: dom-filler-{index}\ntitle: Filler {index}\n---\nUnrelated."),
            )
            .expect("seed page");
    }
    library.write_document(
        Path::new("knowledge/domain/acme.md"),
        "---\nid: dom-acme\ntitle: Billing\naliases: [Acme]\ntags: [project:meridian]\n---\nKnown subjects.",
    ).expect("seed covered subjects");
    library.write_document(
        Path::new("knowledge/domain/standing-directions.md"),
        "---\nid: dom-standing-directions\ntitle: Standing directions\nlast_verified: 1970-01-01\nupdated_at: 1970-02-09\n---\nExisting directions.",
    ).expect("seed stale page");

    let mut ids = Vec::new();
    for index in 0..2 {
        let mut entries = vec![ConversationEntry {
            kind: "fact".into(),
            text: "Context preceding the signal. ".repeat(10),
        }];
        if index == 0 {
            entries.push(ConversationEntry {
                kind: "direction".into(),
                text: "Cite primary evidence.".into(),
            });
        }
        let captured = library
            .capture_source(&CaptureSourceRequest {
                kind: Some("conversation".into()),
                title: Some(format!("Planning {index}")),
                captured_at: "1970-02-01T10:00:00Z".into(),
                participants: vec!["Priya Nair".into()],
                tags: vec!["client:acme".into(), "project:meridian".into()],
                entries,
                ..CaptureSourceRequest::default()
            })
            .expect("capture real conversation shape");
        let path = Path::new(&captured.path);
        let markdown = library.read_text(path).expect("captured Markdown");
        // Incoming evidence must not be confused with the standing page's
        // outgoing citations when deciding whether a direction was recorded.
        let mut markdown =
            markdown.replacen("\n---\n", "\ncites: [dom-standing-directions]\n---\n", 1);
        if index == 0 {
            markdown.push_str("\nrhythm: Weekly briefing — every Monday\n");
        }
        library
            .write_document(path, &markdown)
            .expect("link source to page");
        ids.push(captured.id);
    }
    (library, ids)
}

fn scan_library(library: Box<dyn Library>) -> Value {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks": [], "truncated": false})));
    orbit.push_response(Ok(json!({"hits": []})));
    scan(&ports_for(orbit, library))
}

#[test]
fn library_documents_produce_all_four_signals_through_dispatch() {
    let (library, mut ids) = library_signal_fixture();
    let response = scan_library(Box::new(LibraryWithoutSearch(library)));
    assert_eq!(response["ok"], json!(true), "{response}");
    let candidates = response["output"]["candidates"]
        .as_array()
        .expect("candidates");
    let signals: Vec<_> = candidates
        .iter()
        .map(|candidate| candidate["signal"].as_str().expect("signal"))
        .collect();
    assert_eq!(
        signals,
        [
            "declared_cadence",
            "new_subject",
            "staleness",
            "unrecorded_direction"
        ],
        "{response}"
    );
    assert_eq!(candidates[0]["evidence"], json!([ids[0]]));
    assert_eq!(candidates[1]["evidence"], json!(ids));
    assert!(
        candidates[1]["summary"]
            .as_str()
            .expect("summary")
            .contains("priya nair")
    );
    assert_eq!(candidates[3]["evidence"], json!([ids[0]]));
    ids.push("dom-standing-directions".into());
    ids.sort();
    assert_eq!(candidates[2]["evidence"], json!(ids));
}

#[test]
fn source_tags_find_new_subjects_while_page_aliases_and_tags_cover_known_ones() {
    use std::path::Path;

    let library = office_testkit::fakes::FakeLibrary::initialized();
    for id in ["src-first", "src-second"] {
        library.write_document(
            Path::new(&format!("sources/{id}/source.md")),
            &format!("---\nid: {id}\nkind: text\ntags: [client:acme, project:meridian, topic:invoices]\n---\nEvidence"),
        ).expect("seed tagged source");
    }
    library.write_document(
        Path::new("knowledge/domain/billing.md"),
        "---\nid: dom-billing\ntitle: Billing\naliases: [Acme]\ntags: [project:meridian]\n---\nKnown subjects.",
    ).expect("seed coverage");
    let response = scan_library(Box::new(LibraryWithoutSearch(library)));
    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(response["output"]["count"], json!(1), "{response}");
    let candidate = &response["output"]["candidates"][0];
    assert_eq!(candidate["signal"], json!("new_subject"));
    assert_eq!(candidate["evidence"], json!(["src-first", "src-second"]));
    assert!(
        candidate["summary"]
            .as_str()
            .expect("summary")
            .ends_with("invoices")
    );
}

/// A Library that serves documents but refuses every search, including an
/// empty match-all request. Enumeration must be sufficient for this tool.
struct LibraryWithoutSearch(office_testkit::fakes::FakeLibrary);

impl Library for LibraryWithoutSearch {
    fn is_initialized(&self) -> Result<bool> {
        self.0.is_initialized()
    }
    fn settings(&self) -> Result<Settings> {
        self.0.settings()
    }
    fn source_documents(&self) -> Result<Vec<LibraryText>> {
        self.0.source_documents()
    }
    fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        self.0.knowledge_pages()
    }
    fn backlinks(&self, id: &str) -> Result<Vec<office_core::domain::knowledge::Backlink>> {
        self.0.backlinks(id)
    }
}

fn assert_named_refusal(response: &Value, code: &str, read: &str, cause: &str) {
    assert_eq!(response["ok"], json!(false), "{response}");
    assert!(
        response.get("output").is_none(),
        "a failed or partial read must not return a candidate count: {response}"
    );
    assert_eq!(response["error"]["code"], json!(code), "{response}");
    let message = response["error"]["message"]
        .as_str()
        .expect("error message");
    assert!(
        message.contains(read),
        "response should name {read}, got {message}"
    );
    assert!(
        message.contains(cause),
        "response should keep the underlying cause {cause}, got {message}"
    );
}

fn repetition_tasks(truncated: bool, total: u64) -> Value {
    json!({"tasks": [
        {"id": "t1", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 3},
        {"id": "t2", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 10},
        {"id": "t3", "title": "weekly status report", "status": "done", "tags": ["owner:you", "report"], "completed_days_ago": 20}
    ], "total": total, "truncated": truncated})
}

#[test]
fn failed_task_list_is_named_and_not_a_candidate_count() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Err(Error::OrbitUnavailable));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "orbit_unavailable",
        "orbit.task.list",
        "orbit_unavailable",
    );
}

#[test]
fn refused_task_list_keeps_its_refusal_code() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Err(Error::OrbitRefused {
        code: "rate_limited".to_string(),
    }));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "orbit_refused",
        "orbit.task.list",
        "orbit_refused",
    );
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("rate_limited"),
        "response should keep Orbit's refusal code, got {message}"
    );
}

#[test]
fn failed_friction_search_is_named_and_not_a_candidate_count() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks": [], "total": 0, "truncated": false})));
    orbit.push_response(Err(Error::Orbit {
        code: "orbit_command_failed",
        message: "callback exited unsuccessfully".to_string(),
    }));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "orbit_command_failed",
        "orbit.search",
        "orbit_command_failed",
    );
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("friction"),
        "response should name the friction search, got {message}"
    );
}

/// Each integrity read can fail after both Orbit reads succeed.
struct LibraryReadFails(&'static str);

impl Library for LibraryReadFails {
    fn is_initialized(&self) -> Result<bool> {
        Ok(true)
    }

    fn settings(&self) -> Result<Settings> {
        Ok(Settings::default())
    }

    fn source_documents(&self) -> Result<Vec<LibraryText>> {
        if self.0 == "library.source_documents" {
            Err(read_failure())
        } else {
            Ok(Vec::new())
        }
    }

    fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
        if self.0 == "library.knowledge_pages" {
            Err(read_failure())
        } else {
            Ok(vec![LibraryText {
                path: "knowledge/domain/example.md".into(),
                markdown: Some("---\nid: dom-example\ntitle: Example\n---\nBody".into()),
            }])
        }
    }

    fn backlinks(&self, _id: &str) -> Result<Vec<office_core::domain::knowledge::Backlink>> {
        Err(read_failure())
    }
}

fn read_failure() -> Error {
    Error::Library {
        code: "io_error",
        message: "Library unreadable".into(),
    }
}

#[test]
fn failed_library_reads_are_named_and_not_a_candidate_count() {
    for read in [
        "library.source_documents",
        "library.knowledge_pages",
        "library.backlinks",
    ] {
        let response = scan_library(Box::new(LibraryReadFails(read)));
        assert_named_refusal(&response, "io_error", read, "io_error");
    }
}

#[test]
fn malformed_library_metadata_is_named_instead_of_an_empty_scan() {
    for (path, markdown, read) in [
        (
            "sources/bad/source.md",
            "---\nid: src-bad\nparticipants: Priya\n---\nEvidence",
            "library.source_documents",
        ),
        (
            "knowledge/domain/bad.md",
            "---\nid: dom-bad\ntitle: [Bad]\n---\nPage",
            "library.knowledge_pages",
        ),
    ] {
        let library = office_testkit::fakes::FakeLibrary::initialized();
        library
            .write_document(std::path::Path::new(path), markdown)
            .expect("seed malformed Markdown");
        let response = scan_library(Box::new(library));
        assert_named_refusal(&response, "invalid_frontmatter", read, path);
    }
}

#[test]
fn standing_page_outgoing_anchor_citation_records_the_direction() {
    for inline in [false, true] {
        let (library, ids) = library_signal_fixture();
        let citation = if inline {
            format!("---\nRecorded direction. [{}#L1]", ids[0])
        } else {
            format!("cites: [{}#L1]\n---\nRecorded direction.", ids[0])
        };
        library.write_document(
            std::path::Path::new("knowledge/domain/standing-directions.md"),
            &format!("---\nid: dom-standing-directions\ntitle: Standing directions\nlast_verified: 1970-01-01\n{citation}"),
        ).expect("record source");
        let response = scan_library(Box::new(library));
        assert_eq!(response["ok"], json!(true), "{response}");
        let signals: Vec<_> = response["output"]["candidates"]
            .as_array()
            .expect("candidates")
            .iter()
            .map(|candidate| candidate["signal"].as_str().expect("signal"))
            .collect();
        assert_eq!(signals, ["declared_cadence", "new_subject", "staleness"]);
    }
}

#[test]
fn unavailable_document_text_is_named_instead_of_an_empty_scan() {
    struct UnreadableDocument(bool);
    impl Library for UnreadableDocument {
        fn is_initialized(&self) -> Result<bool> {
            Ok(true)
        }
        fn settings(&self) -> Result<Settings> {
            Ok(Settings::default())
        }
        fn source_documents(&self) -> Result<Vec<LibraryText>> {
            Ok(if self.0 {
                vec![LibraryText {
                    path: "sources/bad/source.md".into(),
                    markdown: None,
                }]
            } else {
                vec![]
            })
        }
        fn knowledge_pages(&self) -> Result<Vec<LibraryText>> {
            Ok(vec![LibraryText {
                path: "knowledge/domain/bad.md".into(),
                markdown: None,
            }])
        }
    }
    for (source, read, path) in [
        (true, "library.source_documents", "sources/bad/source.md"),
        (false, "library.knowledge_pages", "knowledge/domain/bad.md"),
    ] {
        let response = scan_library(Box::new(UnreadableDocument(source)));
        assert_named_refusal(&response, "library_document_unreadable", read, path);
    }
}

#[test]
fn staleness_requires_a_newer_citing_source_and_uses_occurrence_before_capture() {
    use std::path::Path;
    let fixture = || {
        let library = office_testkit::fakes::FakeLibrary::initialized();
        // The default stale threshold is 90 days; this page is 100 days old.
        library.write_document(Path::new("knowledge/domain/example.md"),
        "---\nid: dom-example\ntitle: Example\nlast_verified: 1970-01-01\nupdated_at: 1970-04-10\n---\nPage").expect("page");
        for (id, metadata) in [
            (
                "src-old",
                "occurred_at: 1969-12-01\ncaptured_at: 1970-04-01\ncites: [dom-example]",
            ),
            (
                "src-same-day",
                "occurred_at: 1970-01-01\ncites: [dom-example]",
            ),
            ("src-unlinked", "captured_at: 1970-03-01"),
            (
                "src-related",
                "captured_at: 1970-03-01\nrelated: [dom-example]",
            ),
            ("src-undated", "cites: [dom-example]"),
        ] {
            library
                .write_document(
                    Path::new(&format!("sources/{id}/source.md")),
                    &format!("---\nid: {id}\nkind: text\n{metadata}\n---\nEvidence"),
                )
                .expect("source");
        }
        library
    };
    let check = |library| {
        let orbit = office_testkit::fakes::FakeOrbit::default();
        orbit.push_response(Ok(json!({"tasks": []})));
        orbit.push_response(Ok(json!({"hits": []})));
        let mut ports = ports_for(orbit, library);
        ports.clock = Box::new(office_testkit::fakes::FakeClock(
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(86_400 * 100),
        ));
        scan(&ports)
    };
    let before = check(Box::new(fixture()));
    assert_eq!(before["output"]["count"], json!(0), "{before}");
    let library = fixture();
    library.write_document(Path::new("sources/new/source.md"),
        "---\nid: src-new\nkind: text\ncaptured_at: 1970-04-01\ncites: [dom-example#L1]\n---\nNew evidence").expect("new source");
    let response = check(Box::new(library));
    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(response["output"]["count"], json!(1), "{response}");
    assert_eq!(
        response["output"]["candidates"][0]["signal"],
        json!("staleness")
    );
    assert_eq!(
        response["output"]["candidates"][0]["evidence"],
        json!(["dom-example", "src-new"])
    );
}

#[test]
fn truncated_task_list_is_not_presented_as_complete() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    // Three clustered tasks would be a repetition candidate if this page
    // were treated as the whole set. `total` is larger, and `truncated` says so.
    orbit.push_response(Ok(repetition_tasks(true, 5)));
    orbit.push_response(Ok(json!({"hits": []})));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_named_refusal(
        &response,
        "task_list_truncated",
        "orbit.task.list",
        "truncated",
    );
    let message = response["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("complete"),
        "response should say the page is not the complete task set, got {message}"
    );
}

#[test]
fn complete_task_list_with_truncated_false_still_returns_candidates() {
    let orbit = office_testkit::fakes::FakeOrbit::default();
    orbit.push_response(Ok(repetition_tasks(false, 3)));
    orbit.push_response(Ok(json!({"hits": []})));
    let ports = ports_for(
        orbit,
        Box::new(office_testkit::fakes::FakeLibrary::initialized()),
    );
    let response = scan(&ports);
    assert_eq!(response["ok"], json!(true), "{response}");
    assert_eq!(response["output"]["count"], json!(1), "{response}");
    assert_eq!(
        response["output"]["candidates"][0]["signal"],
        json!("repetition")
    );
}
