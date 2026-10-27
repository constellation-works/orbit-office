//! Bootstrap plans through the public dispatcher.
use super::video_plan::call;
use office_core::ports::{Library, Ports};
use office_testkit::fakes::{FakeLibrary, FakeOrbit, PortsBuilder};
use serde_json::json;
use std::path::Path;

pub(super) fn fixture() -> Ports<'static> {
    let library = FakeLibrary::initialized();
    library.write_document(Path::new("sources/1970/01/src-doc/source.md"),
        "---\nid: src-doc\nkind: doc\ntitle: Design\ntags: [bootstrap:docs]\nsensitivity: normal\n---\n# Ingest\nDesign details.\n").unwrap();
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(library);
    let orbit = FakeOrbit::default();
    orbit.push_response(Ok(json!({"tasks":[],"total":0,"truncated":false})));
    ports.orbit = Box::new(orbit);
    ports
}

#[test]
fn batch_preview_counts_documents_and_refuses_a_stale_digest() {
    let ports = fixture();
    let response = call(&ports, "office.batch-plan", json!({"phase":"docs"}));
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["plan"]["counts"]["digest"], 1);
    assert_eq!(response["output"]["plan"]["counts"]["careful"], 1);
    assert!(response["output"]["plan"]["estimate"].is_null());
    assert!(
        !response["output"]["plan"]["rates_unset"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let refused = call(
        &ports,
        "office.batch-plan",
        json!({"phase":"docs","confirm":true,"digest":"stale"}),
    );
    assert_eq!(refused["error"]["code"], "plan_changed");
}

pub(super) fn source(library: &FakeLibrary, id: &str, phase: &str, body: &str) {
    library.write_document(Path::new(&format!("sources/1970/01/{id}/source.md")),
        &format!("---\nid: {id}\nkind: doc\ntitle: {id}\ntags: [bootstrap:{phase}]\nsensitivity: normal\n---\n{body}\n")).unwrap();
}
pub(super) fn digest(subjects: serde_json::Value) -> serde_json::Value {
    json!({"summary":"Fixture digest","kind_hint":"design-doc","as_of":"1970-01-01","subjects":subjects,"claims":[],"procedures":[],"terms":[],"refs":[]})
}
pub(super) fn ports(library: FakeLibrary) -> Ports<'static> {
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(library);
    ports
}
pub(super) fn preview(ports: &Ports<'_>, input: serde_json::Value) -> serde_json::Value {
    let response = call(ports, "office.batch-plan", input);
    assert_eq!(response["ok"], true, "{response}");
    response["output"].clone()
}
pub(super) fn confirm(ports: &Ports<'_>, phase: &str) -> serde_json::Value {
    let before = preview(ports, json!({"phase":phase}));
    preview(
        ports,
        json!({"phase":phase,"confirm":true,"digest":before["plan"]["digest"]}),
    )
}

#[test]
fn batch_reuses_video_counts_and_existing_digests_need_no_calls() {
    let (ports, models) = super::video_plan::fixture(
        office_core::domain::settings::Settings::default(),
        true,
        false,
    );
    let path = Path::new(super::video_plan::PATH);
    let text = ports.library.read_text(path).unwrap().replacen(
        "kind: video",
        "kind: video\ntags: [bootstrap:kt]",
        1,
    );
    ports.library.write_document(path, &text).unwrap();
    ports
        .library
        .write_document(
            Path::new("sources/1970/01/src-doc/source.md"),
            "---\nid: src-doc\nkind: doc\ntags: [bootstrap:kt]\n---\n# Ingest\nOne document.",
        )
        .unwrap();
    let video = super::video_plan::preview(&ports);
    let batch = preview(&ports, json!({"phase":"kt"}));
    for kind in ["audio", "frames", "extract"] {
        assert_eq!(batch["plan"]["counts"][kind], video["plan"]["counts"][kind]);
    }
    assert_eq!(batch["plan"]["counts"]["digest"], 1);
    assert_eq!(batch["plan"]["videos"].as_array().unwrap().len(), 1);
    assert!(batch["plan"]["estimate"].is_null());
    assert!(models.sensitivities().is_empty());

    let library = FakeLibrary::initialized();
    source(&library, "done", "docs", "# Old");
    library.insert_digest("done", digest(json!([]))).unwrap();
    source(&library, "next", "docs", "# New");
    let output = preview(&self::ports(library), json!({"phase":"docs"}));
    assert_eq!(output["plan"]["sources"], json!(["next"]));
    assert_eq!(output["plan"]["counts"]["digest"], 1);
}

#[test]
fn batch_confirmation_is_idempotent_and_content_membership_and_rate_changes_refuse() {
    for change in ["content", "membership", "rates"] {
        let ports = fixture();
        let batch = confirm(&ports, "docs");
        let input = json!({"phase":"docs","confirm":true,"digest":batch["plan"]["digest"]});
        let head = ports.library.head_commit().unwrap();
        assert_eq!(preview(&ports, input.clone()), batch);
        assert_eq!(ports.library.head_commit().unwrap(), head);
        match change {
            "content" => {
                let path = Path::new("sources/1970/01/src-doc/source.md");
                let text = ports.library.read_text(path).unwrap();
                ports
                    .library
                    .write_document(path, &format!("{text}Changed.\n"))
                    .unwrap();
            }
            "membership" => {
                ports
                    .library
                    .write_document(
                        Path::new("sources/1970/01/added/source.md"),
                        "---\nid: added\nkind: doc\ntags: [bootstrap:docs]\n---\nMore.",
                    )
                    .unwrap();
            }
            "rates" => ports
                .library
                .set_setting("costs.text_1k_tokens", json!(0.2))
                .unwrap(),
            _ => unreachable!(),
        }
        let head = ports.library.head_commit().unwrap();
        let response = call(&ports, "office.batch-plan", input);
        assert_eq!(response["error"]["code"], "plan_changed", "{response}");
        assert_eq!(ports.library.head_commit().unwrap(), head);
    }
}

#[test]
fn batch_private_preview_marks_every_pending_source_and_prices_only_with_all_rates() {
    use office_core::domain::endpoint::Capability;
    let mut library = FakeLibrary::initialized();
    library.settings = super::video_plan::private_settings(&Capability::ALL);
    for id in ["one", "two"] {
        source(&library, id, "docs", "# Ingest\nDesign.");
    }
    let ports = ports(library);
    let private = preview(&ports, json!({"phase":"docs","sensitivity":"private"}));
    assert_eq!(private["plan"]["sources"], json!(["one", "two"]));
    for id in ["one", "two"] {
        let doc = ports.library.get(id).unwrap().unwrap();
        assert_eq!(doc.sensitivity, "private");
    }
    for group in private["plan"]["groups"].as_array().unwrap() {
        assert_eq!(group["route"]["route"], "private:company");
        assert_eq!(group["route"]["label"], "Private: Private");
    }
    let response = preview(
        &ports,
        json!({"phase":"docs","confirm":true,"digest":private["plan"]["digest"]}),
    );
    assert!(response["confirmed_at"].is_string());
    ports
        .library
        .set_setting("costs.currency", json!("USD"))
        .unwrap();
    ports
        .library
        .set_setting("costs.text_1k_tokens", json!(0.5))
        .unwrap();
    let priced = preview(&ports, json!({"phase":"docs"}));
    assert_eq!(priced["plan"]["rates_unset"], json!([]));
    assert_eq!(priced["plan"]["estimate"]["currency"], "USD");
    assert!(priced["plan"]["estimate"]["amount"].as_f64().unwrap() > 0.0);
}

#[test]
fn invalid_batch_inputs_and_private_route_fail_before_mutation() {
    let ports = fixture();
    for input in [
        json!({}),
        json!({"phase":"bad"}),
        json!({"phase":"docs","confirm":1}),
        json!({"phase":"docs","digest":"x"}),
        json!({"phase":"docs","sensitivity":"normal"}),
        json!({"phase":"docs","extra":true}),
        json!({"phase":"docs","additional_careful":null}),
    ] {
        let before = ports.library.head_commit().unwrap();
        assert_eq!(
            call(&ports, "office.batch-plan", input)["error"]["code"],
            "invalid_argument"
        );
        assert_eq!(ports.library.head_commit().unwrap(), before);
    }
    let before = ports.library.head_commit().unwrap();
    assert_eq!(
        call(
            &ports,
            "office.batch-plan",
            json!({"phase":"docs","sensitivity":"private"})
        )["error"]["code"],
        "private_endpoint_unconfigured"
    );
    assert_eq!(ports.library.head_commit().unwrap(), before);
}

fn mutate(
    ports: &Ports<'_>,
    mutation: office_core::domain::bootstrap::Mutation,
) -> office_core::Result<office_core::domain::bootstrap::Batch> {
    let snapshot = ports.library.bootstrap_snapshot()?;
    ports
        .library
        .mutate_batch(&snapshot.revision(&ports.library.settings()?)?, mutation)
}

struct BorrowedLibrary<'a>(&'a FakeLibrary);
impl Library for BorrowedLibrary<'_> {
    fn is_initialized(&self) -> office_core::Result<bool> {
        self.0.is_initialized()
    }
    fn settings(&self) -> office_core::Result<office_core::domain::settings::Settings> {
        self.0.settings()
    }
    fn bootstrap_snapshot(&self) -> office_core::Result<office_core::domain::bootstrap::Snapshot> {
        self.0.bootstrap_snapshot()
    }
    fn mutate_batch(
        &self,
        revision: &str,
        mutation: office_core::domain::bootstrap::Mutation,
    ) -> office_core::Result<office_core::domain::bootstrap::Batch> {
        self.0.mutate_batch(revision, mutation)
    }
}

#[test]
fn extra_careful_calls_have_a_second_digest_confirmation_and_enforced_budget() {
    use office_core::domain::bootstrap::Mutation;
    let library = FakeLibrary::initialized();
    source(&library, "one", "docs", "# Topic");
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(BorrowedLibrary(&library));
    let first = preview(&ports, json!({"phase":"docs"}));
    let reserve = |group| Mutation::Reserve {
        phase: "docs".into(),
        digest: first["plan"]["digest"].as_str().unwrap().into(),
        group,
        extra: false,
    };
    assert_eq!(
        mutate(&ports, reserve(0)).unwrap_err().code(),
        "plan_unconfirmed"
    );
    preview(
        &ports,
        json!({"phase":"docs","confirm":true,"digest":first["plan"]["digest"]}),
    );
    mutate(&ports, reserve(0)).unwrap();
    assert_eq!(
        mutate(&ports, reserve(0)).unwrap_err().code(),
        "plan_exceeded"
    );
    assert_eq!(
        call(
            &ports,
            "office.batch-plan",
            json!({"phase":"docs","additional_careful":true})
        )["error"]["code"],
        "digests_incomplete"
    );
    library.insert_digest("one",digest(json!([{"name":"Topic A","template":"topic"},{"name":"Topic B","template":"topic"},{"name":"Topic C","template":"topic"}]))).unwrap();
    let extra = preview(&ports, json!({"phase":"docs","additional_careful":true}));
    assert_eq!(extra["extra"]["calls"], 2);
    assert!(extra["extra"]["estimate"].is_null());
    let reserve_extra = || Mutation::Reserve {
        phase: "docs".into(),
        digest: extra["extra"]["digest"].as_str().unwrap().into(),
        group: 0,
        extra: true,
    };
    assert_eq!(
        mutate(&ports, reserve_extra()).unwrap_err().code(),
        "plan_changed"
    );
    assert_eq!(
        call(
            &ports,
            "office.batch-plan",
            json!({"phase":"docs","additional_careful":true,"confirm":true,"digest":"stale"})
        )["error"]["code"],
        "plan_changed"
    );
    preview(
        &ports,
        json!({"phase":"docs","additional_careful":true,"confirm":true,"digest":extra["extra"]["digest"]}),
    );
    mutate(&ports, reserve_extra()).unwrap();
    mutate(&ports, reserve_extra()).unwrap();
    assert_eq!(
        mutate(&ports, reserve_extra()).unwrap_err().code(),
        "plan_exceeded"
    );
}

#[test]
fn private_batch_uses_private_video_groups_and_media_refusals_leave_every_source_unchanged() {
    use office_core::domain::endpoint::Capability;
    for capabilities in [vec![Capability::Text], Capability::ALL.to_vec()] {
        let (ports, models) = super::video_plan::fixture(
            super::video_plan::private_settings(&capabilities),
            false,
            false,
        );
        let path = Path::new(super::video_plan::PATH);
        let raw = ports.library.read_text(path).unwrap().replacen(
            "kind: video",
            "kind: video\ntags: [bootstrap:kt]",
            1,
        );
        ports.library.write_document(path, &raw).unwrap();
        let doc_path = Path::new("sources/1970/01/doc/source.md");
        ports.library.write_document(doc_path,"---\nid: doc\nkind: doc\ntags: [bootstrap:kt]\nsensitivity: normal\n---\n# Ingest\nDocument.\n").unwrap();
        let before = ports.library.head_commit().unwrap();
        let response = call(
            &ports,
            "office.batch-plan",
            json!({"phase":"kt","sensitivity":"private"}),
        );
        if capabilities.len() == 1 {
            assert_eq!(
                response["error"]["code"], "private_media_unsupported",
                "{response}"
            );
            assert_eq!(ports.library.head_commit().unwrap(), before);
            assert_eq!(ports.library.read_text(path).unwrap(), raw);
            assert_eq!(
                ports.library.get("doc").unwrap().unwrap().sensitivity,
                "normal"
            );
        } else {
            assert_eq!(response["ok"], true, "{response}");
            assert_eq!(
                ports.library.get("doc").unwrap().unwrap().sensitivity,
                "private"
            );
            assert_eq!(
                ports
                    .library
                    .get(super::video_plan::ID)
                    .unwrap()
                    .unwrap()
                    .sensitivity,
                "private"
            );
            for group in response["output"]["plan"]["groups"].as_array().unwrap() {
                assert_eq!(group["route"]["route"], "private:company");
            }
            for kind in ["audio", "frames", "extract"] {
                assert_eq!(
                    response["output"]["plan"]["videos"][0]["routes"][kind]["route"],
                    "private:company"
                );
            }
        }
        assert!(models.sensitivities().is_empty());
    }
}

#[test]
fn earlier_phase_aliases_estimate_one_group_and_keep_additional_calls_on_private_route() {
    use office_core::domain::endpoint::Capability;
    let mut library = FakeLibrary::initialized();
    library.settings = super::video_plan::private_settings(&Capability::ALL);
    source(&library, "old", "docs", "Earlier context");
    let old_path = Path::new("sources/1970/01/old/source.md");
    let old = library
        .read_text(old_path)
        .unwrap()
        .replace("sensitivity: normal", "sensitivity: private");
    library.write_document(old_path, &old).unwrap();
    library
        .insert_digest(
            "old",
            digest(json!([{"name":"Ingest","template":"topic","aliases":["Pipeline"]}])),
        )
        .unwrap();
    source(&library, "new", "kt", "# Pipeline");
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(BorrowedLibrary(&library));
    let batch = confirm(&ports, "kt");
    assert_eq!(batch["plan"]["counts"]["careful"], 1);
    assert_eq!(
        batch["plan"]["groups"][1]["route"]["route"],
        "private:company"
    );
    assert_eq!(batch["plan"]["groups"][0]["route"]["route"], "hosted");
    library
        .insert_digest(
            "new",
            digest(
                json!([{"name":"Ingest","template":"topic"},{"name":"Other","template":"topic"}]),
            ),
        )
        .unwrap();
    let extra = preview(&ports, json!({"phase":"kt","additional_careful":true}));
    assert_eq!(extra["extra"]["calls"], 1);
    assert_eq!(extra["extra"]["route"]["route"], "private:company");
}

#[test]
fn earlier_digest_changes_invalidate_confirmed_careful_budget() {
    use office_core::domain::bootstrap::Mutation;
    let library = FakeLibrary::initialized();
    source(&library, "old", "docs", "Earlier context");
    library
        .insert_digest("old", digest(json!([{"name":"Ingest","template":"topic"}])))
        .unwrap();
    source(&library, "new", "kt", "# Ingest");
    let mut ports = PortsBuilder::default().build();
    ports.library = Box::new(BorrowedLibrary(&library));
    let batch = confirm(&ports, "kt");
    library
        .insert_digest(
            "old",
            digest(json!([{"name":"Changed evidence","template":"topic"}])),
        )
        .unwrap();
    let response = mutate(
        &ports,
        Mutation::Reserve {
            phase: "kt".into(),
            digest: batch["plan"]["digest"].as_str().unwrap().into(),
            group: 1,
            extra: false,
        },
    )
    .unwrap_err();
    assert_eq!(response.code(), "plan_changed");
}
