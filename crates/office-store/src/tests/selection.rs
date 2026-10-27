//! Selection tray persistence: consume-once, expiry, eviction, clear, and
//! private quotes against a temporary `selections.sqlite`.

use std::time::{Duration, SystemTime};

use office_common::Sensitivity;
use office_core::domain::selection::{EXPIRY, NewSelection, PutRequest, View, normalize};
use rusqlite::Connection;

use crate::selection::SelectionStore;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn draft(object: &str, quote: &str, sensitivity: Sensitivity) -> NewSelection {
    normalize(
        object,
        Some("#L1".into()),
        Some(quote.into()),
        None,
        View::Source,
        sensitivity,
    )
    .expect("draft")
}

fn open() -> (tempfile::TempDir, SelectionStore) {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = SelectionStore::open(dir.path()).expect("open");
    (dir, store)
}

fn put(
    store: &SelectionStore,
    object: &str,
    quote: &str,
    sensitivity: Sensitivity,
    now: SystemTime,
) {
    store
        .put(
            &PutRequest {
                clear: false,
                selection: Some(draft(object, quote, sensitivity)),
            },
            now,
        )
        .expect("put");
}

#[test]
fn get_returns_each_selection_once() {
    let (dir, store) = open();
    let now = at(1_700_000_000);
    put(&store, "src-a", "alpha", Sensitivity::Normal, now);
    put(
        &store,
        "src-b",
        "beta",
        Sensitivity::Normal,
        now + Duration::from_secs(1),
    );
    let peek = store.peek(now + Duration::from_secs(2)).expect("peek");
    let first = store.get(now + Duration::from_secs(2)).expect("get");
    assert_eq!(peek, first);
    assert_eq!(
        first
            .iter()
            .map(|selection| selection.object.as_str())
            .collect::<Vec<_>>(),
        ["src-a", "src-b"]
    );
    let reopened = SelectionStore::open(dir.path()).expect("reopen");
    assert!(
        reopened
            .peek(now + Duration::from_secs(2))
            .expect("consumed peek")
            .is_empty()
    );
}

#[test]
fn expired_selections_are_never_returned() {
    let (_dir, store) = open();
    let now = at(1_700_000_000);
    put(&store, "src-old", "stale", Sensitivity::Normal, now);
    let later = now + EXPIRY;
    assert!(store.peek(later).expect("expired peek").is_empty());
    assert!(store.get(later).expect("expired").is_empty());
    put(&store, "src-new", "fresh", Sensitivity::Normal, later);
    let got = store.get(later).expect("fresh get");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].object, "src-new");
}

#[test]
fn sixth_put_evicts_the_oldest() {
    let (_dir, store) = open();
    let now = at(1_700_000_000);
    for index in 1..=6 {
        let result = store
            .put(
                &PutRequest {
                    clear: false,
                    selection: Some(draft(&format!("obj-{index}"), "quote", Sensitivity::Normal)),
                },
                now + Duration::from_secs(index),
            )
            .expect("put");
        if index == 6 {
            assert_eq!(result.evicted, vec!["sel-1".to_string()]);
        }
    }
    let got = store.get(now + Duration::from_secs(10)).expect("get");
    assert_eq!(
        got.iter()
            .map(|selection| selection.id.as_str())
            .collect::<Vec<_>>(),
        ["sel-2", "sel-3", "sel-4", "sel-5", "sel-6"]
    );
}

#[test]
fn clear_empties_the_tray() {
    let (_dir, store) = open();
    let now = at(1_700_000_000);
    put(&store, "src-a", "alpha", Sensitivity::Normal, now);
    let cleared = store
        .put(
            &PutRequest {
                clear: true,
                selection: None,
            },
            now,
        )
        .expect("clear");
    assert!(cleared.cleared);
    assert!(cleared.selection.is_none());
    assert!(store.get(now).expect("empty").is_empty());
}

#[test]
fn private_selection_stores_no_quote() {
    let (_dir, store) = open();
    let now = at(1_700_000_000);
    let mut drafted = draft("src-private", "secret words", Sensitivity::Private);
    drafted.quote = Some("secret words".into());
    drafted.quote_sha256 = Some("not-stored".into());
    let result = store
        .put(
            &PutRequest {
                clear: false,
                selection: Some(drafted),
            },
            now,
        )
        .expect("put");
    let selection = result.selection.expect("stored");
    assert_eq!(selection.quote, None);
    assert_eq!(selection.quote_sha256, None);
    let quote: Option<String> = Connection::open(store.path())
        .expect("db")
        .query_row(
            "SELECT quote FROM selections WHERE id = ?1",
            [selection.id.as_str()],
            |row| row.get(0),
        )
        .expect("row");
    assert_eq!(quote, None);
    let got = store.get(now).expect("get");
    assert_eq!(got[0].quote, None);
    assert_eq!(got[0].quote_sha256, None);
}

#[test]
fn same_timestamp_eviction_uses_insertion_order_after_ten_puts() {
    let (_dir, store) = open();
    let now = at(1_700_000_000);
    for index in 1..=11 {
        put(
            &store,
            &format!("src-{index}"),
            "quote",
            Sensitivity::Normal,
            now,
        );
    }
    let got = store.get(now).expect("get");
    let objects: Vec<_> = got.iter().map(|item| item.object.as_str()).collect();
    assert_eq!(objects, ["src-7", "src-8", "src-9", "src-10", "src-11"]);
}
