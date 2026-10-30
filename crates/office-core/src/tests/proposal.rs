use crate::domain::proposal::*;
use serde_json::json;

#[test]
fn dismissal_identity_is_kind_and_evidence_set_and_expires_at_thirty_days() {
    let tag = cluster(
        ProposalKind::AutoTask,
        &["b".into(), "a".into(), "a".into()],
    )
    .unwrap();
    assert_eq!(
        tag,
        cluster(ProposalKind::AutoTask, &["a".into(), "b".into()]).unwrap()
    );
    assert_ne!(
        tag,
        cluster(ProposalKind::Retire, &["a".into(), "b".into()]).unwrap()
    );
    let task = json!({"status":"rejected","tags":[tag],"updated_at":"1970-01-11T00:00:00Z"});
    assert!(recently_dismissed(&task, &tag, 40 * 86400 - 1).unwrap());
    assert!(!recently_dismissed(&task, &tag, 40 * 86400).unwrap());
    let mut invalid = task.clone();
    invalid["updated_at"] = json!("unknown");
    assert!(recently_dismissed(&invalid, &tag, 40 * 86400).is_err());
    assert!(cluster(ProposalKind::AutoTask, &[]).is_err());
}

#[test]
fn proposal_payloads_refuse_routines_and_unknown_nested_fields() {
    let base = json!({"kind":"auto_task","definition":{"name":"weekly","schedule":{"cron":"0 9 * * 1"},"template":{"title":"Report"}}});
    for value in [
        json!({"kind":"routine","definition":{}}),
        json!({"kind":"auto_task","definition":{"name":"weekly","schedule":{"cron":"0 9 * * 1","every_minutes":5},"template":{"title":"Report"}}}),
        json!({"kind":"knowledge_add","markdown":"x","routine":{}}),
        json!({"kind":"retire","target":{"type":"routine","name":"weekly"}}),
    ] {
        assert!(serde_json::from_value::<Payload>(value).is_err());
    }
    let mut extra = base.clone();
    extra["definition"]["enabled"] = json!(true);
    assert!(serde_json::from_value::<Payload>(extra).is_err());
    let mut extra = base.clone();
    extra["definition"]["template"]["routine"] = json!({});
    let Payload::AutoTask { definition } = serde_json::from_value(extra).unwrap() else {
        panic!()
    };
    assert!(definition.validate().is_err());
    assert!(serde_json::from_value::<Payload>(base).is_ok());
}

#[test]
fn callback_pins_steps_in_task_instructions_using_only_supported_orbit_keys() {
    let Payload::AutoTask{definition} = serde_json::from_value(json!({"kind":"auto_task","definition":{"name":"weekly","schedule":{"every_minutes":60},"template":{"title":"Report"},"steps":[{"kind":"plugin.tool_call","tool":"office.digest","input":{}}]}})).unwrap() else { panic!() };
    let callback = definition.callback().unwrap();
    assert!(!callback.contains_key("steps"));
    assert_eq!(
        callback["template"]["required_tools"],
        json!(["office.digest"])
    );
    assert!(
        callback["template"]["description"]
            .as_str()
            .unwrap()
            .contains("office.digest")
    );
}

#[test]
fn malformed_schedules_are_refused_before_callbacks() {
    for cron in [
        "61 9 * * *",
        "* * * *",
        "bad cron has five fields",
        "*/0 * * * *",
        "* * 0 * *",
        "* * * 13 *",
    ] {
        let Payload::AutoTask{definition}=serde_json::from_value(serde_json::json!({"kind":"auto_task","definition":{"name":"weekly","schedule":{"cron":cron},"template":{"title":"Report"}}})).unwrap() else {panic!()};
        assert!(definition.validate().is_err(), "{cron}");
    }
}

#[test]
fn proposal_pages_live_in_status_folders_dated_by_closing_and_flat_pages_stay_readable() {
    use std::path::Path;
    let open = parse_record_path(Path::new("proposals/open/ORB-1.md")).unwrap();
    assert_eq!(open.id, "ORB-1");
    assert_eq!(open.location.status(), Some(ProposalStatus::Open));
    let closed = parse_record_path(Path::new("proposals/dismissed/2026/10/ORB-1.md")).unwrap();
    assert_eq!(
        closed.location,
        RecordLocation::Closed {
            status: ProposalStatus::Dismissed,
            year: "2026".into(),
            month: "10".into()
        }
    );
    let flat = parse_record_path(Path::new("proposals/ORB-1.md")).unwrap();
    assert_eq!(flat.location, RecordLocation::Flat);
    assert_eq!(flat.location.status(), None);
    for refused in [
        "proposals/open/2026/10/ORB-1.md",
        "proposals/accepted/ORB-1.md",
        "proposals/accepted/26/10/ORB-1.md",
        "proposals/accepted/2026/13/ORB-1.md",
        "proposals/closed/2026/10/ORB-1.md",
        "proposals/open/ORB 1.md",
        "proposals/open/ORB-1.txt",
        "proposals/../ORB-1.md",
        "knowledge/ORB-1.md",
    ] {
        assert!(parse_record_path(Path::new(refused)).is_err(), "{refused}");
    }
    assert_eq!(
        record_path("ORB-1", ProposalStatus::Accepted, "2026-10-30T12:00:00Z").unwrap(),
        "proposals/accepted/2026/10/ORB-1.md"
    );
    assert_eq!(
        record_path("ORB-1", ProposalStatus::Open, "").unwrap(),
        "proposals/open/ORB-1.md"
    );
    assert!(record_path("ORB-1", ProposalStatus::Dismissed, "soon").is_err());
    assert!(record_path("../x", ProposalStatus::Open, "").is_err());
    for (status, folder) in [
        ("proposed", Some(ProposalStatus::Open)),
        ("done", Some(ProposalStatus::Accepted)),
        ("rejected", Some(ProposalStatus::Dismissed)),
        ("in-progress", None),
        ("archived", None),
    ] {
        assert_eq!(ProposalStatus::from_orbit(status), folder, "{status}");
    }
}

#[test]
fn an_outcome_is_recorded_once() {
    let page = with_outcome("# Evidence\n\n", "Accepted.");
    assert_eq!(page, "# Evidence\n\n## Outcome\n\nAccepted.\n");
    assert_eq!(with_outcome(&page, "Dismissed."), page);
}
