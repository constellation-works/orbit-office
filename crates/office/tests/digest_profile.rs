//! Public extraction/search/reindex with a real temporary Library and offline Models.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::ports::{
    Clock, DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models, Orbit, Ports, RouteCheck,
    Routed, ScriptRunner, TranscribeRequest, Transcript,
};
use office_core::{Error, Result};
use office_store::LibraryStore;
use serde_json::{Value, json};
use std::path::Path;

struct Offline;
impl Orbit for Offline {}
impl ScriptRunner for Offline {}
impl Clock for Offline {
    fn now(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH
    }
}
impl Models for Offline {
    fn extract(&self, request: &ExtractRequest) -> Result<Routed<Value>> {
        assert!(request.text.contains("#L1 Evidence."));
        Ok(Routed {
            value: json!({"summary":"Digest","kind_hint":"design-doc","as_of":null,
            "subjects":[{"name":"SidecarSubject","template":"topic","aliases":[]}],
            "claims":[{"subject":"SidecarSubject","text":"Grounded","cites":["#L1"],"volatile":false}],
            "procedures":[],"terms":[],"refs":[]}),
            derived_by: DerivedBy {
                route: office_core::domain::endpoint::ModelRoute::Hosted,
                model: "offline".into(),
            },
        })
    }
    fn transcribe(&self, _: &TranscribeRequest) -> Result<Routed<Transcript>> {
        Err(Error::OrbitUnavailable)
    }
    fn read_image(&self, _: &ImageRequest) -> Result<Routed<ImageReading>> {
        Err(Error::OrbitUnavailable)
    }
    fn check_private_route(&self) -> Result<RouteCheck> {
        Err(Error::OrbitUnavailable)
    }
}
fn call(ports: &Ports<'_>, tool: &str, input: Value) -> Value {
    let raw = json!({"schema_version":1,"tool":tool,"input":input,"context":{}}).to_string();
    serde_json::from_str(&office_tools::serve(
        &office_core::protocol::parse(&raw).unwrap(),
        ports,
    ))
    .unwrap()
}

#[test]
fn extracted_digest_subject_is_searchable_under_the_source_before_and_after_reindex() {
    let temp = tempfile::tempdir().unwrap();
    let workspace = temp.path().join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let root = workspace.join("library");
    let state = temp.path().join("state");
    LibraryStore::initialize_at(&root, &state, None, &workspace).unwrap();
    let store = LibraryStore::open(&root, &state).unwrap();
    let path = Path::new("sources/1970/01/src-doc/source.md");
    store
        .write_document(
            path,
            "---\nid: src-doc\nkind: doc\ntitle: Design\nsensitivity: normal\n---\nEvidence.\n",
        )
        .unwrap();
    let ports = Ports {
        library: Box::new(store),
        models: Box::new(Offline),
        orbit: Box::new(Offline),
        scripts: Box::new(Offline),
        clock: Box::new(Offline),
        calendar: Box::new(office_core::ports::UnavailableCalendar),
        selections: Box::new(office_core::ports::UnavailableSelections),
    };
    let extracted = call(
        &ports,
        "office.extract",
        json!({"id":"src-doc","profile":"digest"}),
    );
    assert_eq!(extracted["ok"], true, "{extracted}");
    let saved: Value = serde_json::from_str(
        &std::fs::read_to_string(root.join(path.with_file_name("digest.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(saved, extracted["output"]["result"]);
    for reindex in [false, true] {
        if reindex {
            let output = call(&ports, "office.reindex", json!({}));
            assert_eq!(output["output"]["indexed_documents"], 1, "{output}");
        }
        let found = call(
            &ports,
            "office.search",
            json!({"query":"SidecarSubject","kind":"source"}),
        );
        assert_eq!(found["ok"], true, "{found}");
        assert_eq!(found["output"]["count"], 1, "{found}");
        assert_eq!(found["output"]["hits"][0]["id"], "src-doc");
        assert_eq!(
            found["output"]["hits"][0]["path"],
            path.to_string_lossy().as_ref()
        );
    }
}
