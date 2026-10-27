//! Public calendar dispatch with fixed clock, fake gateway and private storage.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::{
    domain::metrics::timestamp,
    ports::{Calendar, Library, Ports},
};
use office_testkit::{
    envelope,
    fakes::{FakeCalendar, FakeLibrary, FakeOrbit, PortsBuilder},
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

fn call(p: &Ports<'_>, tool: &str, input: Value) -> Value {
    let request = office_core::protocol::parse(&envelope::request(tool, input, json!({}))).unwrap();
    envelope::response(&office_tools::serve(&request, p)).unwrap()
}
fn setup() -> (Ports<'static>, FakeCalendar, FakeOrbit) {
    let now = UNIX_EPOCH
        + Duration::from_secs(u64::try_from(timestamp("2026-10-27T00:00:00Z").unwrap()).unwrap());
    let calendar = FakeCalendar::default();
    let orbit = FakeOrbit::default();
    let mut ports = PortsBuilder::default().now(now).build();
    ports.calendar = Box::new(calendar.clone());
    ports.orbit = Box::new(orbit.clone());
    (ports, calendar, orbit)
}
fn sync(p: &Ports<'_>) -> Value {
    call(p, "office.calendar-sync", json!({"ics_file":"fixture.ics"}))
}
fn list(p: &Ports<'_>) -> Value {
    call(
        p,
        "office.calendar-list",
        json!({"from":"2026-10-13T00:00:00Z","until":"2026-11-24T00:00:00Z","include_cancelled":true}),
    )
}
fn feed(body: &str) -> String {
    format!("BEGIN:VCALENDAR\nVERSION:2.0\n{body}\nEND:VCALENDAR\n")
}
fn event(uid: &str, start: &str, end: &str, extra: &str) -> String {
    format!(
        "BEGIN:VEVENT\nUID:{uid}\nDTSTART{start}\nDTEND{end}\nSUMMARY:Fixture\n{extra}\nEND:VEVENT"
    )
}
fn demo(p: &Ports<'_>, calendar: &FakeCalendar) {
    calendar.file(
        "fixture.ics",
        include_str!("../../../demo/synthetic/calendar.ics"),
    );
    let r = sync(p);
    assert_eq!(r["ok"], true, "{r}");
}
#[test]
fn demo_expands_cancellations_moved_keys_dst_and_matches_contact_refs_without_emails() {
    let (mut p, c, _) = setup();
    let library = FakeLibrary::initialized();
    library.write_document(Path::new("knowledge/domain/dom-person-mira-vale.md"),"---\nid: dom-person-mira-vale\ntype: domain\ntemplate: person\ntitle: Mira Vale\nrefs: [{ label: Work, contact: 'mira@acme.example' }]\n---\n# Role\nInvented owner.\n").unwrap();
    p.library = Box::new(library);
    demo(&p, &c);
    let (snapshot, error) = c.load().unwrap();
    assert!(error.is_none());
    assert_eq!(snapshot.events.len(), 6);
    let weekly: Vec<_> = snapshot
        .events
        .iter()
        .filter(|e| e.series.starts_with("lantern-weekly"))
        .collect();
    assert_eq!(weekly.len(), 4);
    let moved = weekly.iter().find(|e| e.title.contains("moved")).unwrap();
    assert!(moved.start.starts_with("2026-10-27T10:00:00-04:00"));
    assert!(moved.key.ends_with(":2026-10-26T13:00:00Z"));
    assert!(
        weekly
            .iter()
            .any(|e| e.status == "cancelled" && e.start.starts_with("2026-10-19"))
    );
    assert!(
        weekly
            .iter()
            .any(|e| e.start == "2026-11-02T09:00:00-05:00")
    );
    let r = list(&p);
    assert_eq!(r["ok"], true, "{r}");
    let rows = r["output"]["events"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        assert_eq!(
            row["people"],
            json!([{"id":"dom-person-mira-vale","title":"Mira Vale"}])
        );
        assert!(row["prep"].is_null());
        assert!(row["capture"].is_null());
        assert!(row["attendees"][0].get("email").is_none());
    }
    assert!(!r.to_string().contains("mira@acme.example"));
    let r = call(
        &p,
        "office.calendar-list",
        json!({"from":"2026-10-13T00:00:00Z","until":"2026-11-24T00:00:00Z"}),
    );
    assert_eq!(r["output"]["events"].as_array().unwrap().len(), 5);
}
#[test]
fn unsupported_rules_keep_only_dtstart_unknown_zones_skip_and_exdate_removes_instances() {
    let (p, c, _) = setup();
    c.file(
        "fixture.ics",
        &feed(
            &[
                event(
                    "unsupported",
                    ";TZID=Eastern Standard Time:20261027T090000",
                    ";TZID=Eastern Standard Time:20261027T100000",
                    "RRULE:FREQ=YEARLY",
                ),
                event(
                    "unknown",
                    ";TZID=Made/Up:20261027T090000",
                    ";TZID=Made/Up:20261027T100000",
                    "",
                ),
                event(
                    "daily",
                    ":20261027T090000Z",
                    ":20261027T100000Z",
                    "RRULE:FREQ=DAILY;INTERVAL=2;COUNT=4\nEXDATE:20261029T090000Z",
                ),
                event(
                    "ordinal",
                    ":20261027T090000Z",
                    ":20261027T100000Z",
                    "RRULE:FREQ=MONTHLY;COUNT=3;BYDAY=-1TU",
                ),
                event(
                    "monthday",
                    ":20261027T090000Z",
                    ":20261027T100000Z",
                    "RRULE:FREQ=MONTHLY;UNTIL=20261130T235959Z;BYMONTHDAY=-1",
                ),
            ]
            .join("\n"),
        ),
    );
    let r = sync(&p);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["output"]["skipped"], 1);
    let (snapshot, _) = c.load().unwrap();
    let unsupported: Vec<_> = snapshot
        .events
        .iter()
        .filter(|e| e.series == "unsupported")
        .collect();
    assert_eq!(unsupported.len(), 1);
    assert!(unsupported[0].recurrence_unsupported);
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|e| e.series == "daily")
            .map(|e| e.start[..10].to_string())
            .collect::<Vec<_>>(),
        vec!["2026-10-27", "2026-10-31", "2026-11-02"]
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|e| e.series == "ordinal")
            .count(),
        2
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|e| e.series == "monthday")
            .count(),
        2
    );
}
fn connections() -> Value {
    json!({"connections":[{"name":"outlook","base_url":"https://graph.microsoft.com/v1.0/","auth":{"scheme":"oauth2_device"},"max_response_bytes":1048576,"disabled":false}]})
}
fn response(body: Value) -> Value {
    json!({"status":200,"bytes":123,"truncated":false,"body":{"kind":"json","value":body}})
}
fn graph(uid: &str) -> Value {
    json!({"iCalUId":uid,"subject":"Graph meeting","start":{"dateTime":"2026-10-28T10:00:00.0000000","timeZone":"Eastern Standard Time"},"end":{"dateTime":"2026-10-28T11:00:00.0000000","timeZone":"Eastern Standard Time"},"originalStart":"2026-10-27T14:00:00Z","isCancelled":true,"sensitivity":"confidential","isOnlineMeeting":true,"onlineMeeting":{"joinUrl":"https://join.example/secret"},"body":{"content":"<p>Hello MIRA@ACME.EXAMPLE</p> https://join.example/secret\nPasscode: 123456"},"attendees":[{"emailAddress":{"name":"Mira","address":"mira@acme.example"},"status":{"response":"accepted"}}]})
}
fn configured(p: &Ports<'_>) {
    p.library
        .set_setting("calendar.connection", json!("outlook"))
        .unwrap();
}
#[test]
fn graph_paging_uses_only_gateway_callbacks_and_preserves_original_start() {
    let (p, c, o) = setup();
    configured(&p);
    o.push_response(Ok(connections()));
    o.push_response(Ok(response(json!({"value":[graph("first")],"@odata.nextLink":"https://graph.microsoft.com/v1.0/me/calendarView?%24skiptoken=page2"}))));
    o.push_response(Ok(response(json!({"value":[graph("second")]}))));
    let r = call(&p, "office.calendar-sync", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["output"]["events"], 2);
    let calls = o.calls();
    assert_eq!(
        calls.iter().map(|c| c.tool).collect::<Vec<_>>(),
        vec![
            "gateway.connection-list",
            "gateway.http-get",
            "gateway.http-get"
        ]
    );
    assert_eq!(calls[1].input["path"], "me/calendarView");
    assert_eq!(calls[2].input["query"]["$skiptoken"], "page2");
    let (snapshot, _) = c.load().unwrap();
    assert_eq!(snapshot.events[0].key, "first:2026-10-27T14:00:00Z");
    assert_eq!(snapshot.events[0].status, "cancelled");
    assert_eq!(snapshot.events[0].sensitivity, "private");
    let r = list(&p);
    assert_eq!(r["ok"], true, "{r}");
    assert!(
        !r.to_string()
            .to_ascii_lowercase()
            .contains("mira@acme.example")
    );
    assert!(!r.to_string().contains("join.example"));
    assert!(!r.to_string().contains("123456"));
}
#[test]
fn malicious_next_links_failed_fetch_parse_and_truncation_preserve_snapshot() {
    for next in [
        "https://evil.example/v1.0/me/calendarView",
        "https://graph.microsoft.com/v1.0evil/me/calendarView",
        "https://graph.microsoft.com/v1.0/../beta/me/calendarView",
        "https://graph.microsoft.com/v1.0/me/calendarView#fragment",
        "https://user@graph.microsoft.com/v1.0/me/calendarView",
    ] {
        let (p, c, o) = setup();
        demo(&p, &c);
        let before = serde_json::to_value(c.load().unwrap().0).unwrap();
        configured(&p);
        o.push_response(Ok(connections()));
        o.push_response(Ok(response(json!({"value":[],"@odata.nextLink":next}))));
        let r = call(&p, "office.calendar-sync", json!({}));
        assert_eq!(r["error"]["code"], "calendar_next_link_refused", "{r}");
        assert_eq!(o.calls().len(), 2);
        assert_eq!(serde_json::to_value(c.load().unwrap().0).unwrap(), before);
        assert_eq!(
            c.load().unwrap().1.as_deref(),
            Some("calendar_next_link_refused")
        );
    }
    let (p, c, o) = setup();
    demo(&p, &c);
    let before = serde_json::to_value(c.load().unwrap().0).unwrap();
    c.file("fixture.ics", "not a calendar");
    assert_eq!(sync(&p)["error"]["code"], "calendar_invalid");
    assert_eq!(serde_json::to_value(c.load().unwrap().0).unwrap(), before);
    configured(&p);
    o.push_response(Ok(connections()));
    o.push_response(Err(office_core::Error::OrbitUnavailable));
    assert_eq!(
        call(&p, "office.calendar-sync", json!({}))["error"]["code"],
        "calendar_fetch_failed"
    );
    assert_eq!(serde_json::to_value(c.load().unwrap().0).unwrap(), before);
    o.push_response(Ok(connections()));
    let mut truncated = response(json!({"value":[]}));
    truncated["truncated"] = json!(true);
    o.push_response(Ok(truncated));
    assert_eq!(
        call(&p, "office.calendar-sync", json!({}))["error"]["code"],
        "calendar_too_large"
    );
    assert_eq!(serde_json::to_value(c.load().unwrap().0).unwrap(), before);
    demo(&p, &c);
    assert!(c.load().unwrap().1.is_none());
}
#[test]
fn paging_stops_at_ten_and_feed_fetch_is_an_empty_gateway_path() {
    let (p, c, o) = setup();
    configured(&p);
    o.push_response(Ok(connections()));
    for n in 0..10 {
        o.push_response(Ok(response(json!({"value":[],"@odata.nextLink":format!("https://graph.microsoft.com/v1.0/me/calendarView?$skiptoken={n}")}))));
    }
    assert_eq!(
        call(&p, "office.calendar-sync", json!({}))["error"]["code"],
        "calendar_too_large"
    );
    assert_eq!(o.calls().len(), 11);
    assert!(c.load().unwrap().0.synced_at.is_none());
    o.push_response(Ok(json!({"connections":[{"name":"outlook","base_url":"https://feed.example/","auth":{"scheme":"secret_path"},"max_response_bytes":1048576,"disabled":false}]})));
    o.push_response(Ok(json!({"status":200,"bytes":55,"truncated":false,"body":{"kind":"text","text":"BEGIN:VCALENDAR\nVERSION:2.0\nEND:VCALENDAR\n"}})));
    let r = call(&p, "office.calendar-sync", json!({}));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        o.calls().last().unwrap().input,
        json!({"connection":"outlook","path":""})
            .as_object()
            .unwrap()
            .clone()
    );
    assert!(c.load().unwrap().0.synced_at.is_some());
}
#[test]
fn calendar_config_bounds_types_and_list_ranges_are_refused_through_dispatch() {
    let (p, _, _) = setup();
    for (key, lo, hi) in [
        ("window_days_past", 1, 90),
        ("window_days_ahead", 1, 90),
        ("prep_lead_minutes", 15, 1440),
        ("prep_max_per_run", 1, 20),
    ] {
        for value in [json!(lo), json!(hi)] {
            let r = call(
                &p,
                "office.config-set",
                json!({"key":format!("calendar.{key}"),"value":value}),
            );
            assert_eq!(r["ok"], true, "{r}");
        }
        for value in [
            json!(lo - 1),
            json!(hi + 1),
            json!("10"),
            json!(null),
            json!(1.5),
        ] {
            let r = call(
                &p,
                "office.config-set",
                json!({"key":format!("calendar.{key}"),"value":value}),
            );
            assert_eq!(r["error"]["code"], "invalid_library_setting", "{r}");
        }
    }
    for value in [json!("bad/path"), json!("x".repeat(49)), json!(true)] {
        assert_eq!(
            call(
                &p,
                "office.config-set",
                json!({"key":"calendar.connection","value":value})
            )["error"]["code"],
            "invalid_library_setting"
        );
    }
    assert_eq!(
        call(
            &p,
            "office.config-set",
            json!({"key":"calendar.connection","value":null})
        )["ok"],
        true
    );
    for (from, until) in [
        ("2026-10-01T00:00:00Z", "2026-11-12T00:00:01Z"),
        ("2026-10-02T00:00:00Z", "2026-10-01T00:00:00Z"),
        ("2026-02-30T00:00:00Z", "2026-03-01T00:00:00Z"),
    ] {
        assert_eq!(
            call(
                &p,
                "office.calendar-list",
                json!({"from":from,"until":until})
            )["error"]["code"],
            "invalid_argument"
        );
    }
}

#[test]
fn list_joins_existing_prep_and_capture_and_reports_summary_presence() {
    for summarized in [false, true] {
        let (mut p, c, _) = setup();
        demo(&p, &c);
        let key = "lantern-qbr@calendar.example.invalid:2026-10-28T14:00:00Z";
        let library = FakeLibrary::initialized();
        library.write_document(Path::new("artifacts/art-prep/artifact.md"),&format!("---\nid: art-prep\ntitle: Prep\nseries: meeting-prep\nevent_key: '{key}'\n---\nPrepared.\n")).unwrap();
        library.write_document(Path::new("sources/2026/10/src-20261028-captured/source.md"),&format!("---\nid: src-20261028-captured\nkind: meeting\ntitle: Meeting notes\nevent: {{ key: '{key}', series: 'lantern-qbr@calendar.example.invalid' }}\n---\nNotes.\n")).unwrap();
        if summarized {
            library
                .insert_source_sidecar("src-20261028-captured", "summary.md", "A cited summary.")
                .unwrap();
        }
        p.library = Box::new(library);
        let response = list(&p);
        assert_eq!(response["ok"], true, "{response}");
        let event = response["output"]["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["key"] == key)
            .unwrap();
        assert_eq!(event["prep"], "art-prep");
        assert_eq!(
            event["capture"],
            json!({"source":"src-20261028-captured","summarized":summarized})
        );
    }
}

#[test]
fn calendar_setting_changes_preserve_an_existing_bootstrap_plan() {
    let (p, _, _) = setup();
    p.library.write_document(Path::new("sources/2026/10/src-doc/source.md"),"---\nid: src-doc\nkind: doc\ntitle: Design\ntags: [bootstrap:docs]\nsensitivity: normal\n---\n# Ingest\nDesign details.\n").unwrap();
    let preview = call(&p, "office.batch-plan", json!({"phase":"docs"}));
    assert_eq!(preview["ok"], true, "{preview}");
    assert_eq!(
        call(
            &p,
            "office.config-set",
            json!({"key":"calendar.connection","value":"fixture"})
        )["ok"],
        true
    );
    let confirmed = call(
        &p,
        "office.batch-plan",
        json!({"phase":"docs","confirm":true,"digest":preview["output"]["plan"]["digest"]}),
    );
    assert_eq!(confirmed["ok"], true, "{confirmed}");
}
