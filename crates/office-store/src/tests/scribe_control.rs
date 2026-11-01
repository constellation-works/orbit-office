//! Scribe control persistence against a temporary `scribe_control.sqlite`:
//! records survive reopening, changes are atomic, a refused change writes
//! nothing, and old records are dropped.

use std::time::{Duration, SystemTime};

use office_core::domain::scribe_control::{Change, RETENTION};

use crate::scribe_control::{DATABASE_FILE, ScribeControlStore};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

#[test]
fn records_round_trip_across_connections_and_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ScribeControlStore::open(dir.path()).expect("open");
    assert_eq!(store.path(), dir.path().join(DATABASE_FILE));
    assert_eq!(store.get("s1").expect("get"), Default::default());

    store
        .apply("s1", &Change::SetSwitch(true), at(100))
        .expect("switch");
    store
        .apply("s1", &Change::RequestNow, at(101))
        .expect("request");
    store
        .apply("s1", &Change::SetTitle("Release".into()), at(102))
        .expect("title");
    let (control, consumed) = store
        .apply("s1", &Change::Heartbeat, at(103))
        .expect("beat");
    assert!(!consumed);

    let reopened = ScribeControlStore::open(dir.path()).expect("reopen");
    let read = reopened.get("s1").expect("get");
    assert_eq!(read, control);
    assert_eq!(read.on, Some(true));
    assert_eq!(read.now_requested_at, Some(at(101)));
    assert_eq!(read.title.as_deref(), Some("Release"));
    assert_eq!(read.heartbeat_at, Some(at(103)));
    assert!(read.window_open(at(200)));
    assert!(!read.window_open(at(223)));

    let (_, consumed) = reopened
        .apply("s1", &Change::ConsumeNow(Some(at(101))), at(104))
        .expect("consume");
    assert!(consumed);
    let (_, again) = reopened
        .apply("s1", &Change::ConsumeNow(None), at(105))
        .expect("consume again");
    assert!(!again, "consumed once");
    assert_eq!(
        reopened.get("s2").expect("other session"),
        Default::default()
    );
}

#[test]
fn a_refused_change_writes_nothing_and_old_records_are_dropped() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = ScribeControlStore::open(dir.path()).expect("open");
    store
        .apply("old", &Change::SetSwitch(false), at(10))
        .expect("old");
    assert!(
        store
            .apply("s1", &Change::SetTitle("two\nlines".into()), at(11))
            .is_err()
    );
    assert_eq!(store.get("s1").expect("get"), Default::default());

    let later = at(10) + RETENTION;
    store.apply("s1", &Change::Heartbeat, later).expect("beat");
    assert_eq!(
        store.get("old").expect("get"),
        Default::default(),
        "expired"
    );
    assert!(store.get("s1").expect("get").heartbeat_at.is_some());
}

#[cfg(unix)]
#[test]
fn state_directory_and_database_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("plugin_state");
    let store = ScribeControlStore::open(&state).expect("open");
    let mode = |path: &std::path::Path| {
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(store.path()), 0o600);
}
