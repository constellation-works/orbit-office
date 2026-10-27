use crate::domain::selection::{
    self, EXPIRY, NewSelection, PutRequest, Selection, Tray, expired, normalize,
};
use office_common::Sensitivity;
use std::time::{Duration, SystemTime};

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn draft(object: &str, sensitivity: Sensitivity) -> NewSelection {
    normalize(
        object,
        Some("#L1".into()),
        Some(format!("quote {object}")),
        None,
        selection::View::Source,
        sensitivity,
    )
    .expect("draft")
}

#[test]
fn private_normalization_drops_quote_and_hash() {
    let selection = normalize(
        "src-private",
        None,
        Some("secret words".into()),
        Some("abc".into()),
        selection::View::Page,
        Sensitivity::Private,
    )
    .expect("private");
    assert_eq!(selection.quote, None);
    assert_eq!(selection.quote_sha256, None);
    assert_eq!(selection.object, "src-private");
}

#[test]
fn normal_quote_without_a_hash_records_the_quote_digest() {
    let selection = draft("src-1", Sensitivity::Normal);
    assert_eq!(selection.quote.as_deref(), Some("quote src-1"));
    assert!(selection.quote_sha256.is_some());
}

#[test]
fn expiry_starts_at_fifteen_minutes() {
    let selected = at(1_000);
    assert!(!expired(
        selected,
        selected + EXPIRY - Duration::from_secs(1)
    ));
    assert!(expired(selected, selected + EXPIRY));
}

#[test]
fn sixth_put_evicts_the_oldest_and_get_consumes_once() {
    let mut tray = Tray::default();
    let start = at(50_000);
    for index in 1..=6 {
        let result = tray.put(
            &PutRequest {
                clear: false,
                selection: Some(draft(&format!("obj-{index}"), Sensitivity::Normal)),
            },
            start + Duration::from_secs(index),
        );
        if index < 6 {
            assert!(result.evicted.is_empty());
        } else {
            assert_eq!(result.evicted, vec!["sel-1".to_string()]);
        }
    }
    let first = tray.get(start + Duration::from_secs(10));
    let objects: Vec<&str> = first
        .iter()
        .map(|selection| selection.object.as_str())
        .collect();
    assert_eq!(objects, ["obj-2", "obj-3", "obj-4", "obj-5", "obj-6"]);
    assert!(tray.get(start + Duration::from_secs(10)).is_empty());
}

#[test]
fn expired_selections_are_not_returned_or_counted() {
    let mut tray = Tray::default();
    let start = at(80_000);
    let _ = tray.put(
        &PutRequest {
            clear: false,
            selection: Some(draft("old", Sensitivity::Normal)),
        },
        start,
    );
    assert!(tray.get(start + EXPIRY).is_empty());
    // The expired row is still stored and does not occupy a tray slot.
    assert_eq!(tray.rows.len(), 1);
    let _ = tray.put(
        &PutRequest {
            clear: false,
            selection: Some(draft("new", Sensitivity::Normal)),
        },
        start + EXPIRY,
    );
    let got = tray.get(start + EXPIRY);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].object, "new");
}

#[test]
fn clear_empties_the_tray_before_an_optional_put() {
    let mut tray = Tray::default();
    let _ = tray.put(
        &PutRequest {
            clear: false,
            selection: Some(draft("keep", Sensitivity::Normal)),
        },
        at(10),
    );
    let cleared = tray.put(
        &PutRequest {
            clear: true,
            selection: None,
        },
        at(11),
    );
    assert!(cleared.cleared);
    assert!(cleared.selection.is_none());
    assert!(tray.rows.is_empty());
}

#[test]
fn private_put_stores_no_quote_even_if_the_draft_had_one() {
    let mut tray = Tray::default();
    let mut drafted = draft("src-private", Sensitivity::Private);
    drafted.quote = Some("should not stick".into());
    drafted.quote_sha256 = Some("hash".into());
    let result = tray.put(
        &PutRequest {
            clear: false,
            selection: Some(drafted),
        },
        at(10),
    );
    let selection: &Selection = result.selection.as_ref().expect("stored");
    assert_eq!(selection.quote, None);
    assert_eq!(selection.quote_sha256, None);
}
