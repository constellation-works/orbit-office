use super::{SPEC_PAGE, page};
use crate::domain::knowledge::RefTarget;

#[test]
fn the_model_view_keeps_contact_labels_and_drops_their_values() {
    let page = page(&SPEC_PAGE.replace(
        "Renews in March",
        "Renews in March; ask jane.park@acme.example or +1 555 123 4567",
    ));
    let view = page.model_view();
    let contact = &view.refs[3];
    assert_eq!(contact.label, "Jane Park");
    assert_eq!(contact.target, RefTarget::Contact);
    assert_eq!(contact.value, None);
    assert_eq!(view.refs[1].value.as_deref(), Some("jira:ACME"));
    assert!(
        view.body
            .contains("ask [contact withheld] or [contact withheld]")
    );
    let markdown = view.to_markdown();
    assert!(!markdown.contains("acme.example"), "{markdown}");
    assert!(!markdown.contains("4567"), "{markdown}");
    assert!(markdown.contains(r#"{ label: "Jane Park", contact: null }"#));
    let json = serde_json::to_value(&view).expect("view serializes");
    assert_eq!(json["refs"][3]["value"], serde_json::Value::Null);
    assert_eq!(json["type"], "domain");
}
