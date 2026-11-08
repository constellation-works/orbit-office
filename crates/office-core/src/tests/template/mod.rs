//! Template parsing and page validation through the public domain API.

mod assess;
mod bundled;
mod performable;
mod rules;
mod validation;

use crate::Error;
use crate::domain::template::Template;

const PERSON: &str = "---\ntemplate: person\ntype: domain\nid: dom-person-<slug>\ntitle: \"<Full name>\"\none_per_library: false\nattach: participants\nsensitivity: normal\nsections: [Role, Works with, What they care about, History]\nrequired: [Role]\n---\n## Role\nDescribe their work.\n";

fn person() -> Template {
    Template::from_markdown("person.md", PERSON).expect("valid person rules")
}

fn assert_invalid(text: &str, field: &str) {
    let error =
        Template::from_markdown("custom/person.md", text).expect_err("invalid template refused");
    assert_eq!(error.code(), "template_invalid");
    let Error::TemplateInvalid {
        file,
        field: actual,
        ..
    } = &error
    else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(file, "custom/person.md");
    assert_eq!(actual, field);
    assert!(error.to_string().contains("custom/person.md"));
    assert!(error.to_string().contains(field));
    assert!(!error.retryable());
}
