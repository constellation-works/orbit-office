//! Calendar tools through the built binary against an isolated workspace.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use serde_json::{Value, json};
use std::io::Write;
use std::process::{Command, Stdio};

struct Env {
    root: tempfile::TempDir,
}
impl Env {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
        }
    }
    fn tool(&self, tool: &str, input: Value) -> Value {
        let mut child = Command::new(env!("CARGO_BIN_EXE_office"))
            .arg("orbit-tool")
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("ORBIT_BIN", self.root.path().join("stub-orbit"))
            .env("PATH", std::env::var_os("PATH").unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let request = json!({"schema_version":1,"tool":tool,"input":input,"context":{
            "workspace_root":self.root.path(),"plugin_state":self.root.path().join("state")}});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.to_string().as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
}
#[test]
fn never_synced_calendar_is_read_only_and_distinct_from_an_empty_sync() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let response = env.tool(
        "office.calendar-list",
        json!({"from":"2026-10-26T00:00:00Z","until":"2026-11-02T00:00:00Z"}),
    );
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(
        response["output"],
        json!({"events":[],"synced_at":null,"last_error":null})
    );
    assert!(!env.root.path().join("state/calendar").exists());
}

#[test]
fn offline_sync_is_owner_only_and_failed_parse_or_fetch_keeps_exact_snapshot_bytes() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let feed = env.root.path().join("calendar.ics");
    std::fs::write(&feed, include_str!("../../../demo/synthetic/calendar.ics")).unwrap();
    let response = env.tool("office.calendar-sync", json!({"ics_file":"calendar.ics"}));
    assert_eq!(response["ok"], true, "{response}");
    let snapshot = env.root.path().join("state/calendar/events.json");
    let before = std::fs::read(&snapshot).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&snapshot).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(snapshot.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    let range = json!({"from":"2026-10-13T00:00:00Z","until":"2026-11-24T00:00:00Z","include_cancelled":true});
    for invalid in [
        "not ICS",
        "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:missing-dates\nEND:VEVENT\nEND:VCALENDAR",
    ] {
        std::fs::write(&feed, invalid).unwrap();
        let response = env.tool("office.calendar-sync", json!({"ics_file":"calendar.ics"}));
        assert_eq!(response["error"]["code"], "calendar_invalid");
        assert_eq!(std::fs::read(&snapshot).unwrap(), before);
        assert_eq!(
            env.tool("office.calendar-list", range.clone())["output"]["last_error"],
            "calendar_invalid"
        );
    }
    for invalid in ["../calendar.ics", "/calendar.ics"] {
        assert_eq!(
            env.tool("office.calendar-sync", json!({"ics_file":invalid}))["error"]["code"],
            "calendar_invalid"
        );
        assert_eq!(std::fs::read(&snapshot).unwrap(), before);
    }
    assert_eq!(
        env.tool(
            "office.config-set",
            json!({"key":"calendar.connection","value":"outlook"})
        )["ok"],
        true
    );
    // The stub callback program does no network I/O or access to host state.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let program = env.root.path().join("stub-orbit");
        std::fs::write(&program,"#!/usr/bin/env python3\nimport sys,json\nif sys.argv[3]=='gateway.connection-list':\n print(json.dumps({'connections':[{'name':'outlook','base_url':'https://graph.microsoft.com/v1.0/','auth':{'scheme':'oauth2_device'},'max_response_bytes':1048576,'disabled':False}]}))\nelse:\n print(json.dumps({'error':'secret-fixture-value','code':'gateway_refused'}),file=sys.stderr);sys.exit(1)\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).unwrap();
        let response = env.tool("office.calendar-sync", json!({}));
        assert_eq!(
            response["error"]["code"], "calendar_fetch_failed",
            "{response}"
        );
        assert!(!response.to_string().contains("secret-fixture-value"));
        assert_eq!(std::fs::read(&snapshot).unwrap(), before);
        assert_eq!(
            env.tool("office.calendar-list", range.clone())["output"]["last_error"],
            "calendar_fetch_failed"
        );
    }
    std::fs::write(&feed, include_str!("../../../demo/synthetic/calendar.ics")).unwrap();
    assert_eq!(
        env.tool("office.calendar-sync", json!({"ics_file":"calendar.ics"}))["ok"],
        true
    );
    assert_eq!(
        env.tool("office.calendar-list", range)["output"]["last_error"],
        Value::Null
    );
}

#[test]
#[cfg(unix)]
fn import_and_state_symlinks_cannot_escape_the_host_supplied_roots() {
    use std::os::unix::fs::symlink;
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let outside = tempfile::tempdir().unwrap();
    let feed = outside.path().join("private.ics");
    std::fs::write(&feed, "BEGIN:VCALENDAR\nEND:VCALENDAR").unwrap();
    symlink(&feed, env.root.path().join("linked.ics")).unwrap();
    assert_eq!(
        env.tool("office.calendar-sync", json!({"ics_file":"linked.ics"}))["error"]["code"],
        "calendar_storage_unavailable"
    );
    assert!(!env.root.path().join("state/calendar/events.json").exists());
    // A symlinked snapshot is never read or overwritten.
    let target = outside.path().join("sentinel");
    std::fs::write(&target, "unchanged").unwrap();
    symlink(&target, env.root.path().join("state/calendar/events.json")).unwrap();
    let response = env.tool(
        "office.calendar-list",
        json!({"from":"2026-10-27T00:00:00Z","until":"2026-10-28T00:00:00Z"}),
    );
    assert_eq!(response["error"]["code"], "calendar_storage_unavailable");
    assert_eq!(std::fs::read_to_string(target).unwrap(), "unchanged");
}

#[test]
fn real_library_prep_capture_and_summary_are_joined_by_event_key() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let start = office_core::domain::source::format_rfc3339_utc(std::time::SystemTime::now())
        .replace("+00:00", "Z");
    let end = office_core::domain::source::format_rfc3339_utc(
        std::time::SystemTime::now() + std::time::Duration::from_secs(3600),
    );
    let compact = |s: &str| s.replace(['-', ':'], "");
    let feed = format!(
        "BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:fixture\nDTSTART:{}\nDTEND:{}\nSUMMARY:Meeting\nEND:VEVENT\nEND:VCALENDAR\n",
        compact(&start),
        compact(&end.replace("+00:00", "Z"))
    );
    std::fs::write(env.root.path().join("fixture.ics"), feed).unwrap();
    let synced = env.tool("office.calendar-sync", json!({"ics_file":"fixture.ics"}));
    assert_eq!(
        synced["ok"],
        true,
        "{synced}; feed={}",
        std::fs::read_to_string(env.root.path().join("fixture.ics")).unwrap()
    );
    let key = format!("fixture:{start}");
    let source = env.root.path().join("library/sources/fixture");
    let artifact = env.root.path().join("library/artifacts/prep");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&artifact).unwrap();
    std::fs::write(source.join("source.md"),format!("---\nid: src-fixture\nkind: meeting\ntitle: Notes\nevent: {{ key: '{key}', series: fixture }}\n---\nNotes.\n")).unwrap();
    std::fs::write(
        artifact.join("artifact.md"),
        format!(
            "---\nid: art-prep\ntitle: Prep\nseries: meeting-prep\nevent_key: '{key}'\n---\nPrep.\n"
        ),
    )
    .unwrap();
    assert_eq!(env.tool("office.reindex", json!({}))["ok"], true);
    let until = office_core::domain::source::format_rfc3339_utc(
        std::time::SystemTime::now() + std::time::Duration::from_secs(86400),
    );
    for summarized in [false, true] {
        if summarized {
            std::fs::write(source.join("summary.md"), "Summary.\n").unwrap();
        }
        let response = env.tool("office.calendar-list", json!({"from":start,"until":until}));
        assert_eq!(response["ok"], true, "{response}");
        assert_eq!(response["output"]["events"][0]["prep"], "art-prep");
        assert_eq!(
            response["output"]["events"][0]["capture"],
            json!({"source":"src-fixture","summarized":summarized})
        );
    }
}
