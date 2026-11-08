use super::page;
use crate::Error;

#[test]
fn contact_detection_ignores_timestamps_dates_ids_and_amounts() {
    let ok = "---\nid: dom-a\ntitle: A\n---\nAt [00:12:31] on 2026-10-28 to 2026-11-03, see [src-20261028-acme-qbr#L14-20], ACME-1234567890, budget $1,250,000 and v2.10.3.\n";
    page(ok).check_base_rules().expect("no contact detail");
    for body in [
        "mail jane@acme.example",
        "call (555) 123-4567",
        "call 555.123.4567",
        "call +44 20 7946 0958",
        "call 5551234567",
    ] {
        let markdown = format!("---\nid: dom-a\ntitle: A\n---\n{body}\n");
        assert!(
            matches!(
                page(&markdown).check_base_rules(),
                Err(Error::ContactInBody { .. })
            ),
            "{body}"
        );
    }
}
