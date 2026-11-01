//! `office.scribe-query` through public exec dispatch, over fixture SCR
//! files in an in-memory Library, with a fixed clock in a fixed zone
//! (specs/scribe.md, "Answering"). The tool must make no model call, proven
//! with a Models fake that refuses every call.
//! style note: SCR ids in fixtures are `SCR-…` file ids.
//! Lines are 1-based file lines: the 20 front-matter lines come first.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use office_core::ports::{Clock, Models, Ports};
use office_core::{Error, Result};
use office_testkit::{envelope, fakes::FakeLibrary};
use serde_json::{Value, json};

const ID_A: &str = "SCR-31-2007-scribe-v1-design";
const ID_B: &str = "SCR-29-0900-second-session";
const PATH_A: &str = "sources/conversation/2026/10/SCR-31-2007-scribe-v1-design.md";
const PATH_B: &str = "sources/conversation/2026/10/SCR-29-0900-second-session.md";

/// 2026-11-20T12:00:00Z, which is 04:00 on 20 November in Los Angeles.
struct ZonedClock;
impl Clock for ZonedClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_176_000)
    }
    fn time_zone(&self) -> Option<String> {
        Some("America/Los_Angeles".into())
    }
}

/// A Models port that refuses (and records) every call.
#[derive(Clone, Default)]
struct RefusingModels {
    calls: Rc<Cell<usize>>,
}

impl RefusingModels {
    fn refuse<T>(&self) -> Result<T> {
        self.calls.set(self.calls.get() + 1);
        Err(Error::Model {
            code: "model_unavailable",
            message: "refusing fake".into(),
            retryable: false,
        })
    }
}

impl Models for RefusingModels {
    fn transcribe(
        &self,
        _request: &office_core::ports::TranscribeRequest,
    ) -> Result<office_core::ports::Routed<office_core::ports::Transcript>> {
        self.refuse()
    }
    fn extract(
        &self,
        _request: &office_core::ports::ExtractRequest,
    ) -> Result<office_core::ports::Routed<Value>> {
        self.refuse()
    }
    fn read_image(
        &self,
        _request: &office_core::ports::ImageRequest,
    ) -> Result<office_core::ports::Routed<office_core::ports::ImageReading>> {
        self.refuse()
    }
    fn check_private_route(&self) -> Result<office_core::ports::RouteCheck> {
        self.refuse()
    }
}

fn scr(id: &str, session: &str, counterpart: &str, entries: &[String]) -> String {
    let mut text = format!(
        "---\nid: \"{id}\"\ntitle: null\nsession_id: \"{session}\"\nprovider: \"claude-code\"\nmodel: null\ncounterpart: \"{counterpart}\"\ncounterpart_name: null\nhost: \"macbook\"\nproject: \"constellation\"\ncwd: \"/work/constellation\"\nbranch: \"agent-main\"\ntranscript: \"t.jsonl\"\nparsed: \"library/.office/state/scribe/{session}.jsonl\"\nstarted_at: \"2026-10-29T09:00:00-07:00\"\nlast_scribed_at: null\nscribed_through: null\ntopics: [\"scribe\"]\naliases: []\n---\n"
    );
    for entry in entries {
        text.push_str(entry);
        text.push('\n');
    }
    text
}

fn fixture_library() -> FakeLibrary {
    let library = FakeLibrary::initialized();
    let a = scr(
        ID_A,
        "session-a",
        "orchestrator",
        &[
            "- note 2026-10-30T09:00-07:00 #pending #scribe \"older open item\" refs: ORB-100 by: opus".into(),
            "- m-0002 2026-10-31T10:00-07:00 #pending #scribe \"waiting on CI\" refs: ORB-20780 by: opus".into(),
            "- m-0003 2026-10-31T11:00-07:00 #pending #scribe \"closed item\" refs: ORB-101 by: opus".into(),
            format!("- m-0004 2026-10-31T12:00-07:00 #resolved \"CI is green\" resolves: {ID_A}#L23 by: opus"),
            "- m-0005 2026-10-31T13:00-07:00 #preference \"ship early\" matter: release-with-open-defects review_by: 2026-12-01".into(),
            format!("- m-0006 2026-11-01T09:00-07:00 #preference \"hold for review\" matter: release-with-open-defects review_by: 2026-11-15 supersedes: {ID_A}#L25"),
            "- m-0007 2026-11-01T09:30-07:00 #preference \"review weekly\" matter: review-cadence review_by: 2027-06-01".into(),
            "- m-0008 2026-11-01T11:00-07:00 #decision #no-why #scribe \"lets go one step at a time\" refs: ORB-20690".into(),
            format!("- note 2026-11-01T11:05-07:00 #why \"because the file must stay grep-able\" resolves: {ID_A}#L28 by: user"),
            "- m-0009 2026-11-01T12:00-07:00 #decision #recall \"keep recall\" why: m-0001 \"reason inline\"".into(),
            "- m-0010 2026-11-01T13:00-07:00 #landed #scribe \"merged as 7266911\" refs: PR#268".into(),
            "- m-0011 2026-10-31T13:30-07:00 #landed #scribe \"landed the day before\" refs: ORB-102".into(),
        ],
    );
    let b = scr(
        ID_B,
        "session-b",
        "self",
        &["- note 2026-10-29T09:00-07:00 #pending #scribe \"earliest open item\" refs: ORB-99 by: wraith".into()],
    );
    library.insert_scribe_file(PATH_A, &a);
    library.insert_scribe_file(PATH_B, &b);
    library
}

fn ports() -> (Ports<'static>, RefusingModels) {
    let models = RefusingModels::default();
    let mut ports = office_testkit::fakes::PortsBuilder::default().build();
    ports.library = Box::new(fixture_library());
    ports.clock = Box::new(ZonedClock);
    ports.models = Box::new(models.clone());
    (ports, models)
}

fn query(ports: &Ports<'_>, input: Value) -> Value {
    let request =
        office_core::protocol::parse(&envelope::request("office.scribe-query", input, json!({})))
            .unwrap();
    let response = envelope::response(&office_tools::serve(&request, ports)).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}

fn quotes(out: &Value) -> Vec<&str> {
    out["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["quote"].as_str().unwrap())
        .collect()
}

#[test]
fn pending_returns_open_items_oldest_first_with_citations() {
    let (ports, models) = ports();
    let out = query(&ports, json!({"mode":"pending"}));
    assert_eq!(
        quotes(&out),
        ["earliest open item", "older open item", "waiting on CI"],
        "closed item is resolved and so is omitted: {out}"
    );
    let first = &out["results"][0];
    assert_eq!(first["path"], PATH_B);
    assert_eq!(first["line"], 21);
    assert_eq!(first["file_line"], format!("{PATH_B}:21"));
    assert_eq!(first["scr_id"], ID_B);
    assert_eq!(first["by"], "wraith");
    assert_eq!(first["refs"], json!(["ORB-99"]));
    assert_eq!(first["counterpart"], "self");
    assert_eq!(first["resume"], "claude --resume session-b");
    let waiting = &out["results"][2];
    assert_eq!(waiting["path"], PATH_A);
    assert_eq!(waiting["line"], 22);
    assert_eq!(waiting["message_id"], "m-0002");
    assert_eq!(waiting["by"], "opus");
    assert_eq!(waiting["refs"], json!(["ORB-20780"]));
    assert_eq!(models.calls.get(), 0, "scribe-query makes no model call");
}

#[test]
fn pref_returns_current_omits_superseded_flags_stale_and_none_for_unknown() {
    let (ports, _) = ports();
    let out = query(
        &ports,
        json!({"mode":"pref","matter":"release-with-open-defects"}),
    );
    assert_eq!(quotes(&out), ["hold for review"], "{out}");
    assert_eq!(
        out["results"][0]["stale"], true,
        "past review_by 2026-11-15"
    );
    assert_eq!(out["results"][0]["review_by"], "2026-11-15");
    assert_eq!(out["results"][0]["matter"], "release-with-open-defects");

    let fresh = query(&ports, json!({"mode":"pref","matter":"review-cadence"}));
    assert_eq!(quotes(&fresh), ["review weekly"]);
    assert_eq!(fresh["results"][0]["stale"], false);

    let none = query(&ports, json!({"mode":"pref","matter":"no-such-matter"}));
    assert_eq!(none["results"], json!([]));
    assert_eq!(none["count"], 0);
    assert_eq!(none["files_searched"], 2);
    assert_eq!(none["file_count"], 2);
    assert_eq!(none["time_zone"], "America/Los_Angeles");
}

#[test]
fn landed_filters_by_local_date_range_and_defaults_to_today() {
    let (ports, _) = ports();
    let out = query(
        &ports,
        json!({"mode":"landed","since":"2026-11-01","until":"2026-11-01"}),
    );
    assert_eq!(quotes(&out), ["merged as 7266911"], "{out}");
    assert_eq!(out["since"], "2026-11-01");
    assert_eq!(out["until"], "2026-11-01");
    assert_eq!(out["date_range"]["since"], "2026-11-01");

    let range = query(
        &ports,
        json!({"mode":"landed","since":"2026-10-31","until":"2026-11-01"}),
    );
    assert_eq!(
        quotes(&range),
        ["landed the day before", "merged as 7266911"]
    );

    // No range: landed means today in the stated zone, and today has none.
    let today = query(&ports, json!({"mode":"landed"}));
    assert_eq!(today["results"], json!([]));
    assert_eq!(today["since"], "2026-11-20");
    assert_eq!(today["until"], "2026-11-20");
    assert_eq!(today["files_searched"], 2);
}

#[test]
fn why_filters_by_matter_or_topic_and_fills_no_why_from_a_why_note() {
    let (ports, _) = ports();
    let out = query(&ports, json!({"mode":"why","topic":"scribe"}));
    assert_eq!(quotes(&out), ["lets go one step at a time"], "{out}");
    let decision = &out["results"][0];
    assert_eq!(decision["why"], "because the file must stay grep-able");
    assert_eq!(decision["reason"], "because the file must stay grep-able");
    assert_eq!(decision["why_filled_by"], format!("{ID_A}#L29"));
    assert_eq!(decision["no_why"], false);

    // In why mode a matter names the topic a decision is about.
    let by_matter = query(&ports, json!({"mode":"why","matter":"scribe"}));
    assert_eq!(quotes(&by_matter), ["lets go one step at a time"]);

    let inline = query(&ports, json!({"mode":"why","topic":"recall"}));
    assert_eq!(quotes(&inline), ["keep recall"]);
    assert_eq!(inline["results"][0]["why"], "reason inline");
}

#[test]
fn general_filters_combine_and_no_match_reports_the_search() {
    let (ports, models) = ports();
    let out = query(
        &ports,
        json!({"tag":"pending","by":"opus","refs":"ORB-20780","session_id":"session-a"}),
    );
    assert_eq!(quotes(&out), ["waiting on CI"]);

    let empty = query(
        &ports,
        json!({"tag":"incident","since":"2026-11-01","until":"2026-11-02"}),
    );
    assert_eq!(empty["results"], json!([]));
    assert_eq!(empty["files_searched"], 2);
    assert_eq!(empty["since"], "2026-11-01");
    assert_eq!(empty["until"], "2026-11-02");
    assert_eq!(empty["time_zone"], "America/Los_Angeles");
    assert_eq!(models.calls.get(), 0);
}

#[test]
fn invalid_mode_and_dates_are_refused_without_a_model_call() {
    let (ports, models) = ports();
    for input in [
        json!({"mode":"bogus"}),
        json!({"since":"2026-13-01"}),
        json!({"since":"2026-11-02","until":"2026-11-01"}),
        json!({"limit":0}),
    ] {
        let request = office_core::protocol::parse(&envelope::request(
            "office.scribe-query",
            input,
            json!({}),
        ))
        .unwrap();
        let response = envelope::response(&office_tools::serve(&request, &ports)).unwrap();
        assert_eq!(response["ok"], false, "{response}");
        assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    }
    assert_eq!(models.calls.get(), 0);
}

#[test]
fn general_query_keeps_newest_at_and_over_limit_canned_keeps_oldest() {
    // Five same-time pending lines in one file: (time, path, line) is the
    // deterministic order, so the newest are the highest line numbers.
    let entries: Vec<String> = (0..5)
        .map(|i| {
            format!("- m-{i:04} 2026-11-01T09:00-07:00 #pending #scribe \"event {i}\" by: opus")
        })
        .collect();
    let library = FakeLibrary::initialized();
    library.insert_scribe_file(PATH_A, &scr(ID_A, "session-a", "orchestrator", &entries));
    let (mut ports, _) = ports();
    ports.library = Box::new(library);

    // Exact cap: every line is returned, oldest first.
    let exact = query(&ports, json!({"since": "2026-11-01", "limit": 5}));
    assert_eq!(exact["count"], 5);
    assert_eq!(
        quotes(&exact),
        ["event 0", "event 1", "event 2", "event 3", "event 4"]
    );

    // Over cap: the general query keeps the newest bounded rows, still in
    // ascending (time, path, line) order; count reports the full total.
    let over = query(&ports, json!({"since": "2026-11-01", "limit": 3}));
    assert_eq!(over["count"], 5, "{over}");
    assert_eq!(quotes(&over), ["event 2", "event 3", "event 4"], "{over}");

    // Canned modes preserve their semantics: pending keeps the oldest first.
    let canned = query(&ports, json!({"mode": "pending", "limit": 2}));
    assert_eq!(canned["count"], 5);
    assert_eq!(quotes(&canned), ["event 0", "event 1"], "{canned}");
}

#[test]
fn each_line_carries_its_session_title_project_and_cursor() {
    let library = fixture_library();
    let titled = scr(
        "SCR-01-0800-web-screen",
        "session-c",
        "orchestrator",
        &["- m-0002 2026-11-01T08:10-07:00 #todo #scribe \"build the screen\"".into()],
    )
    .replace("title: null", "title: \"Web screen\"")
    .replace("project: \"constellation\"", "project: \"orbit-office\"")
    .replace("scribed_through: null", "scribed_through: \"m-0004\"");
    library.insert_scribe_file(
        "sources/conversation/2026/11/SCR-01-0800-web-screen.md",
        &titled,
    );
    let (mut ports, models) = ports();
    ports.library = Box::new(library);

    let out = query(&ports, json!({"session_id":"session-c"}));
    let line = &out["results"][0];
    assert_eq!(line["quote"], "build the screen", "{out}");
    assert_eq!(line["title"], "Web screen");
    assert_eq!(line["project"], "orbit-office");
    assert_eq!(line["scribed_through"], "m-0004");

    // A file still untitled, never advanced, reports both as null.
    let untitled = query(&ports, json!({"session_id":"session-b"}));
    assert_eq!(untitled["results"][0]["title"], Value::Null);
    assert_eq!(untitled["results"][0]["project"], "constellation");
    assert_eq!(untitled["results"][0]["scribed_through"], Value::Null);
    assert_eq!(models.calls.get(), 0);
}

/// Two SCR files, one session each, in two projects: `alpha` and `beta`.
fn two_project_ports() -> (Ports<'static>, RefusingModels) {
    let library = FakeLibrary::initialized();
    let alpha = scr(
        "SCR-40-0900-alpha-session",
        "session-alpha",
        "orchestrator",
        &[
            "- m-0001 2026-10-30T09:00-07:00 #landed #scribe \"alpha landed earlier\" by: opus"
                .into(),
            "- m-0002 2026-11-01T09:00-07:00 #pending #scribe \"alpha open item\" by: opus".into(),
            "- m-0003 2026-11-01T10:00-07:00 #landed #scribe \"alpha landed today\" by: opus"
                .into(),
        ],
    )
    .replace("project: \"constellation\"", "project: \"alpha\"");
    let beta = scr(
        "SCR-41-0900-beta-session",
        "session-beta",
        "orchestrator",
        &[
            "- m-0001 2026-11-01T09:30-07:00 #pending #scribe \"beta open item\" by: opus".into(),
            "- m-0002 2026-11-01T11:00-07:00 #landed #scribe \"beta landed today\" by: opus".into(),
        ],
    )
    .replace("project: \"constellation\"", "project: \"beta\"");
    library.insert_scribe_file(
        "sources/conversation/2026/11/SCR-40-0900-alpha-session.md",
        &alpha,
    );
    library.insert_scribe_file(
        "sources/conversation/2026/11/SCR-41-0900-beta-session.md",
        &beta,
    );
    let (mut ports, models) = ports();
    ports.library = Box::new(library);
    (ports, models)
}

#[test]
fn project_selects_only_that_projects_lines_and_composes_with_other_filters() {
    let (ports, models) = two_project_ports();

    let alpha = query(&ports, json!({"project": "alpha"}));
    assert_eq!(
        quotes(&alpha),
        [
            "alpha landed earlier",
            "alpha open item",
            "alpha landed today"
        ],
        "{alpha}"
    );
    assert_eq!(alpha["count"], 3, "{alpha}");
    assert!(
        alpha["results"]
            .as_array()
            .unwrap()
            .iter()
            .all(|line| line["project"] == "alpha"),
        "{alpha}"
    );

    let beta = query(&ports, json!({"project": "beta"}));
    assert_eq!(
        quotes(&beta),
        ["beta open item", "beta landed today"],
        "{beta}"
    );

    // Composes with the local date range.
    let dated = query(
        &ports,
        json!({"project": "alpha", "since": "2026-11-01", "until": "2026-11-01"}),
    );
    assert_eq!(
        quotes(&dated),
        ["alpha open item", "alpha landed today"],
        "{dated}"
    );

    // Composes with the session: alpha's session holds no beta line.
    let same_session = query(
        &ports,
        json!({"project": "alpha", "session_id": "session-alpha"}),
    );
    assert_eq!(same_session["count"], 3, "{same_session}");
    let crossed = query(
        &ports,
        json!({"project": "beta", "session_id": "session-alpha"}),
    );
    assert_eq!(crossed["results"], json!([]), "{crossed}");

    // Composes with the limit: the newest alpha lines are kept, count is total.
    let limited = query(&ports, json!({"project": "alpha", "limit": 2}));
    assert_eq!(limited["count"], 3, "{limited}");
    assert_eq!(
        quotes(&limited),
        ["alpha open item", "alpha landed today"],
        "{limited}"
    );

    // Composes with the canned modes.
    let pending = query(&ports, json!({"mode": "pending", "project": "beta"}));
    assert_eq!(quotes(&pending), ["beta open item"], "{pending}");
    let landed = query(
        &ports,
        json!({
            "mode": "landed",
            "project": "alpha",
            "since": "2026-11-01",
            "until": "2026-11-01"
        }),
    );
    assert_eq!(quotes(&landed), ["alpha landed today"], "{landed}");

    // A project with no lines is an empty answer, not a refusal.
    let none = query(&ports, json!({"project": "gamma"}));
    assert_eq!(none["results"], json!([]), "{none}");
    assert_eq!(none["count"], 0, "{none}");
    assert_eq!(models.calls.get(), 0, "scribe-query makes no model call");
}

#[test]
fn project_must_be_a_string_and_is_refused_without_a_model_call() {
    let (ports, models) = two_project_ports();
    let request = office_core::protocol::parse(&envelope::request(
        "office.scribe-query",
        json!({"project": 7}),
        json!({}),
    ))
    .unwrap();
    let response = envelope::response(&office_tools::serve(&request, &ports)).unwrap();
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["error"]["code"], "invalid_argument", "{response}");
    assert_eq!(models.calls.get(), 0);
}
