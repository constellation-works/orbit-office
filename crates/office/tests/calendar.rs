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
        json!({"events":[],"synced_at":null,"last_error":null,"sources":{"local":false,"external":true}})
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

#[test]
fn local_events_persist_across_processes_and_external_sync_never_overwrites_them() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let event = json!({"title":"Local planning", "start":"2026-10-30T14:00:00Z", "end":"2026-10-30T15:00:00Z", "attendees":["Jane Park"]});
    let created = env.tool(
        "office.calendar-sync",
        json!({"local":{"action":"create","event":event}}),
    );
    assert_eq!(created["ok"], true, "{created}");
    let key = created["output"]["key"].as_str().unwrap();
    let range = json!({"from":"2026-10-26T00:00:00Z","until":"2026-11-02T00:00:00Z"});
    let listed = env.tool("office.calendar-list", range.clone());
    assert_eq!(listed["output"]["events"][0]["key"], key);
    assert_eq!(listed["output"]["events"][0]["local"], true);
    assert_eq!(
        listed["output"]["sources"],
        json!({"local":true,"external":true})
    );
    let file = env.root.path().join("calendar.ics");
    std::fs::write(file, "BEGIN:VCALENDAR\nBEGIN:VEVENT\nUID:external\nDTSTART:20261030T100000Z\nDTEND:20261030T110000Z\nSUMMARY:External meeting\nEND:VEVENT\nEND:VCALENDAR").unwrap();
    assert_eq!(
        env.tool("office.calendar-sync", json!({"ics_file":"calendar.ics"}))["ok"],
        true
    );
    let merged = env.tool("office.calendar-list", range.clone());
    assert_eq!(merged["output"]["events"].as_array().unwrap().len(), 2);
    for (local, external, count) in [
        (true, false, 1),
        (false, true, 1),
        (false, false, 0),
        (true, true, 2),
    ] {
        assert_eq!(
            env.tool(
                "office.calendar-sync",
                json!({"local":{"action":"sources","local":local,"external":external}})
            )["ok"],
            true
        );
        assert_eq!(
            env.tool("office.calendar-list", range.clone())["output"]["events"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
    }
    let edited = json!({"title":"Moved planning", "start":"2026-10-30T16:00:00Z", "end":"2026-10-30T17:00:00Z"});
    assert_eq!(
        env.tool(
            "office.calendar-sync",
            json!({"local":{"action":"update","key":key,"event":edited}})
        )["ok"],
        true
    );
    let listed = env.tool("office.calendar-list", range.clone());
    let local = listed["output"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["local"] == true)
        .unwrap();
    assert_eq!(local["key"], key);
    assert_eq!(local["title"], "Moved planning");
    assert_eq!(
        env.tool(
            "office.calendar-sync",
            json!({"local":{"action":"delete","key":key}})
        )["ok"],
        true
    );
    assert_eq!(
        env.tool("office.calendar-list", range)["output"]["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn invalid_local_writes_preserve_state_and_refuse_external_keys() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let event =
        json!({"title":"Local", "start":"2026-10-30T14:00:00Z","end":"2026-10-30T15:00:00Z"});
    let created = env.tool(
        "office.calendar-sync",
        json!({"local":{"action":"create","event":event}}),
    );
    assert_eq!(created["ok"], true, "{created}");
    let file = env.root.path().join(".office/calendar/local.json");
    let before = std::fs::read(&file).unwrap();
    for change in [
        json!({"action":"delete","key":"external:2026-10-30T10:00:00Z"}),
        json!({"action":"delete","key":"local-missing"}),
        json!({"action":"sources","local":true,"external":false,"unknown":true}),
        json!({"action":"create","event":{"title":"Bad range","start":"2026-10-30T15:00:00Z","end":"2026-10-30T14:00:00Z"}}),
        json!({"action":"create","event":{"title":"Unknown field","start":"2026-10-30T14:00:00Z","end":"2026-10-30T15:00:00Z","local":false}}),
    ] {
        assert_eq!(
            env.tool("office.calendar-sync", json!({"local":change}))["error"]["code"],
            "calendar_invalid"
        );
        assert_eq!(std::fs::read(&file).unwrap(), before);
    }
    assert!(
        !env.root.path().join("state/calendar").exists(),
        "local writes never sync or record external failures"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(file.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[cfg(unix)]
#[test]
fn local_storage_symlinks_and_future_versions_refuse_writes() {
    use std::os::unix::fs::symlink;
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let outside = tempfile::tempdir().unwrap();
    let sentinel = outside.path().join("sentinel");
    std::fs::write(&sentinel, b"unchanged").unwrap();
    let dir = env.root.path().join(".office/calendar");
    std::fs::create_dir_all(&dir).unwrap();
    symlink(&sentinel, dir.join("local.json")).unwrap();
    let change = json!({"local":{"action":"sources","local":true,"external":false}});
    assert_eq!(
        env.tool("office.calendar-sync", change.clone())["error"]["code"],
        "calendar_storage_unavailable"
    );
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"unchanged");
    std::fs::remove_file(dir.join("local.json")).unwrap();
    let future = br#"{"schema_version":2,"sources":{"local":true,"external":false},"events":[]}"#;
    std::fs::write(dir.join("local.json"), future).unwrap();
    assert_eq!(
        env.tool("office.calendar-sync", change)["error"]["code"],
        "calendar_storage_unavailable"
    );
    assert_eq!(std::fs::read(dir.join("local.json")).unwrap(), future);
}

#[test]
fn local_only_calendar_works_without_reading_or_writing_imported_state() {
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let change = json!({"local":{"action":"sources","local":true,"external":false}});
    assert_eq!(env.tool("office.calendar-sync", change)["ok"], true);
    assert!(!env.root.path().join("state/calendar").exists());
    let dir = env.root.path().join("state/calendar");
    std::fs::create_dir_all(&dir).unwrap();
    let foreign = b"unreadable imported snapshot";
    std::fs::write(dir.join("events.json"), foreign).unwrap();
    let range = json!({"from":"2026-10-26T00:00:00Z","until":"2026-11-02T00:00:00Z"});
    assert_eq!(env.tool("office.calendar-list", range)["ok"], true);
    let skipped = env.tool("office.calendar-sync", json!({}));
    assert_eq!(skipped["ok"], true, "{skipped}");
    assert_eq!(
        skipped["output"],
        json!({"key":null,"sources":{"local":true,"external":false}})
    );
    assert_eq!(std::fs::read(dir.join("events.json")).unwrap(), foreign);
    assert!(!dir.join("last_error.json").exists());
}
