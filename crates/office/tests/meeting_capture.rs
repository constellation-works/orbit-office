//! Meeting capture and the inbox through public dispatch with the real Library.
//! All files are temporary; calendar, models and Orbit are offline fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use office_core::domain::{calendar::Snapshot, endpoint::ModelRoute, source::video::VideoAnalysis};
use office_core::ports::{
    Calendar, Clock, DerivedBy, ExtractRequest, ImageReading, ImageRequest, Models, Orbit, Ports,
    RouteCheck, Routed, ScriptRunner, TranscribeRequest, Transcript, UnavailableSelections,
};
use office_core::{Error, Result};
use office_store::{CalendarStore, LibraryStore};
use serde_json::{Map, Value, json};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Offline {
    transcriptions: Rc<RefCell<usize>>,
    duration_ms: Rc<RefCell<u64>>,
    fail_transcription_at: Rc<RefCell<Option<usize>>>,
    fail_task: Rc<RefCell<bool>>,
    replace_on_add: Rc<RefCell<Option<(std::path::PathBuf, String)>>>,
    extractions: Rc<RefCell<usize>>,
    tasks: Rc<RefCell<Vec<Map<String, Value>>>>,
}
impl Offline {
    fn routed<T>(value: T) -> Routed<T> {
        Routed {
            value,
            derived_by: DerivedBy {
                route: ModelRoute::Hosted,
                model: "offline".into(),
            },
        }
    }
}
impl Models for Offline {
    fn analyze_video(&self, _: &[u8], _: &serde_json::Number) -> Result<VideoAnalysis> {
        Ok(VideoAnalysis {
            duration_ms: (*self.duration_ms.borrow()).max(2000),
            has_audio: true,
            candidates: vec![],
        })
    }
    fn media_audio(&self, bytes: &[u8], _: u64, _: u64) -> Result<Vec<u8>> {
        Ok(bytes.into())
    }
    fn transcribe(&self, _: &TranscribeRequest) -> Result<Routed<Transcript>> {
        let index = *self.transcriptions.borrow();
        *self.transcriptions.borrow_mut() += 1;
        if *self.fail_transcription_at.borrow() == Some(index) {
            return Err(Error::OrbitUnavailable);
        }
        Ok(Self::routed(Transcript {
            text: "Ada: Ship the release.".into(),
            ..Transcript::default()
        }))
    }
    fn extract(&self, _: &ExtractRequest) -> Result<Routed<Value>> {
        *self.extractions.borrow_mut() += 1;
        Ok(Self::routed(
            json!({"summary":"Quasar meeting summary", "decisions":[{"text":"Ship the release", "cites":["#L1"]}], "to_dos":[{"text":"Review release", "owner":"you", "cites":["#L1"]}]}),
        ))
    }
    fn read_image(&self, _: &ImageRequest) -> Result<Routed<ImageReading>> {
        Err(Error::OrbitUnavailable)
    }
    fn check_private_route(&self) -> Result<RouteCheck> {
        Err(Error::OrbitUnavailable)
    }
}
impl Orbit for Offline {
    fn task_add(&self, input: &Map<String, Value>) -> Result<Value> {
        self.tasks.borrow_mut().push(input.clone());
        if let Some((path, text)) = self.replace_on_add.borrow_mut().take() {
            std::fs::write(path, text).unwrap();
        }
        if *self.fail_task.borrow() {
            return Err(Error::OrbitUnavailable);
        }
        Ok(json!({"id":"fixture-todo"}))
    }
}
impl ScriptRunner for Offline {}
impl Clock for Offline {
    fn now(&self) -> std::time::SystemTime {
        std::time::UNIX_EPOCH
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
struct Fixture {
    root: tempfile::TempDir,
    ports: Ports<'static>,
    offline: Offline,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let state = root.path().join("state");
        let library = workspace.join("library");
        LibraryStore::initialize_at(&library, &state, None, &workspace).unwrap();
        let calendar = CalendarStore::new(state.clone(), workspace);
        let snapshot: Snapshot = serde_json::from_value(json!({"events":[{
            "key":"fixture-event", "series":"fixture-series", "title":"Release planning",
            "start":"1970-01-01T10:00:00Z", "end":"1970-01-01T11:00:00Z", "all_day":false,
            "status":"confirmed", "location":"", "online":false, "organizer":{"name":"Ada"},
            "attendees":[{"name":"Ada", "email":"ada@example.invalid", "response":"accepted"}],
            "agenda":"Release", "sensitivity":"private", "recurrence_unsupported":false
        }], "skipped":0, "synced_at":"1970-01-01T00:00:00Z"}))
        .unwrap();
        calendar.replace(&snapshot).unwrap();
        let offline = Offline::default();
        let ports = Ports {
            calendar: Box::new(calendar),
            library: Box::new(LibraryStore::open(&library, &state).unwrap()),
            models: Box::new(offline.clone()),
            orbit: Box::new(offline.clone()),
            scripts: Box::new(offline.clone()),
            clock: Box::new(offline.clone()),
            selections: Box::new(UnavailableSelections),
            shared: Box::new(office_core::ports::UnavailableShared),
        };
        Self {
            root,
            ports,
            offline,
        }
    }
    fn capture(&self, input: Value) -> Value {
        let response = call(&self.ports, "office.capture", input);
        assert_eq!(response["ok"], true, "{response}");
        response["output"].clone()
    }
}
#[test]
fn event_capture_copies_metadata_refuses_unknown_and_allows_repeated_notes() {
    let f = Fixture::new();
    let input = json!({"event_key":"fixture-event", "text":"Ship the release.", "title":"Ignored", "kind":"note", "sensitivity":"normal", "participants":["Ignored"], "occurred_at":"2000-01-01T00:00:00Z"});
    let first = f.capture(input.clone());
    assert_eq!(first["kind"], "meeting");
    assert_eq!(first["sensitivity"], "private");
    let markdown = f
        .ports
        .library
        .read_text(std::path::Path::new(first["path"].as_str().unwrap()))
        .unwrap();
    let m = office_core::domain::metrics::metadata(&markdown).unwrap();
    assert_eq!(m["title"], "Release planning");
    assert_eq!(m["occurred_at"], "1970-01-01T10:00:00Z");
    assert_eq!(m["participants"], json!(["Ada"]));
    assert_eq!(
        m["event"],
        json!({"key":"fixture-event","series":"fixture-series"})
    );
    assert!(!markdown.contains("ada@example.invalid"));
    let second = f.capture(input);
    assert_ne!(first["id"], second["id"]);
    assert_eq!(second["deduplicated"], false);
    let unknown = call(
        &f.ports,
        "office.capture",
        json!({"event_key":"missing", "text":"Notes"}),
    );
    assert_eq!(unknown["error"]["code"], "unknown_event", "{unknown}");
    assert_eq!(f.ports.library.source_documents().unwrap().len(), 2);
}
#[test]
fn meeting_inbox_saves_indexed_anchorable_summary_and_is_a_noop_on_rerun() {
    let f = Fixture::new();
    let source = f.capture(json!({"event_key":"fixture-event", "text":"Ship the release."}));
    let id = source["id"].as_str().unwrap();
    let before = f.ports.library.read_source(id).unwrap().body;
    let run = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(run["ok"], true, "{run}");
    let summary = f
        .ports
        .library
        .source_sidecar(id, "summary.md")
        .unwrap()
        .unwrap();
    assert!(summary.contains(&format!("{id}#L1")), "{summary}");
    assert!(summary.contains("Ship the release"));
    assert_eq!(f.ports.library.read_source(id).unwrap().body, before);
    assert_eq!(*f.offline.transcriptions.borrow(), 0);
    assert_eq!(*f.offline.extractions.borrow(), 1);
    assert_eq!(
        f.offline.tasks.borrow()[0]["tags"],
        json!(["owner:you", format!("source:{id}")])
    );
    let shown = call(
        &f.ports,
        "office.source-show",
        json!({"id":id,"document":"summary","anchor":"#L3"}),
    );
    assert_eq!(shown["ok"], true, "{shown}");
    // The event source is private, so tool callers get its summary withheld;
    // the stored summary sidecar above stays intact for the Library itself.
    assert_eq!(shown["output"]["sensitivity"], "private", "{shown}");
    assert_eq!(shown["output"]["content_withheld"], true, "{shown}");
    assert_eq!(shown["output"]["body"], "", "{shown}");
    assert_eq!(
        shown["output"]["summary"],
        serde_json::Value::Null,
        "{shown}"
    );
    assert!(!shown.to_string().contains("Quasar"), "{shown}");
    let outside = call(
        &f.ports,
        "office.source-show",
        json!({"id":id,"document":"summary","anchor":"#L99999"}),
    );
    assert_eq!(outside["error"]["code"], "anchor_out_of_range", "{outside}");
    let search = || call(&f.ports, "office.search", json!({"query":"Quasar"}));
    assert_eq!(search()["output"]["hits"][0]["id"], id);
    f.ports.library.reindex().unwrap();
    assert_eq!(search()["output"]["hits"][0]["id"], id);
    let calendar = call(
        &f.ports,
        "office.calendar-list",
        json!({"from":"1970-01-01T00:00:00Z","until":"1970-01-02T00:00:00Z"}),
    );
    assert_eq!(
        calendar["output"]["events"][0]["capture"],
        json!({"source":id,"summarized":true}),
        "{calendar}"
    );
    let again = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(again["output"]["processed"], json!([]), "{again}");
    assert_eq!(*f.offline.extractions.borrow(), 1);
    assert_eq!(f.offline.tasks.borrow().len(), 1);
}
#[test]
fn meeting_inbox_imports_vtt_and_transcribes_only_empty_audio() {
    for ext in ["vtt", "wav", "txt"] {
        let f = Fixture::new();
        let upload = f
            .root
            .path()
            .join("workspace")
            .join(format!("meeting.{ext}"));
        std::fs::write(
            &upload,
            if ext == "vtt" {
                "WEBVTT\n\n00:00:00.000 --> 00:00:02.000\nAda: Ship the release.\n"
            } else if ext == "txt" {
                "Ada: Ship the release."
            } else {
                "fixture audio"
            },
        )
        .unwrap();
        let source = f.capture(json!({"event_key":"fixture-event","upload_path":upload}));
        let id = source["id"].as_str().unwrap();
        let before = f.ports.library.read_source(id).unwrap().body;
        let run = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
        assert_eq!(run["ok"], true, "{ext}: {run}");
        assert_eq!(
            *f.offline.transcriptions.borrow(),
            usize::from(ext == "wav")
        );
        assert_eq!(*f.offline.extractions.borrow(), 1);
        let after = f.ports.library.read_source(id).unwrap().body;
        if ext == "wav" {
            assert!(after.contains("Ship the release"));
        } else {
            assert_eq!(after, before);
        }
        let again = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
        assert_eq!(again["output"]["processed"], json!([]));
        assert_eq!(f.ports.library.read_source(id).unwrap().body, after);
    }
}

#[test]
fn meeting_inbox_holds_uncertain_todo_filing_and_partial_transcription() {
    let f = Fixture::new();
    f.capture(json!({"event_key":"fixture-event", "text":"Ship the release."}));
    *f.offline.fail_task.borrow_mut() = true;
    let failed = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(failed["ok"], false, "{failed}");
    *f.offline.fail_task.borrow_mut() = false;
    let retry = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(
        retry["error"]["code"], "meeting_processing_incomplete",
        "{retry}"
    );
    assert_eq!(f.offline.tasks.borrow().len(), 1);
    assert_eq!(*f.offline.extractions.borrow(), 1);

    let f = Fixture::new();
    let upload = f.root.path().join("workspace/meeting.wav");
    std::fs::write(&upload, b"fixture audio").unwrap();
    let source = f.capture(json!({"event_key":"fixture-event", "upload_path":upload}));
    *f.offline.duration_ms.borrow_mut() = 601_000;
    *f.offline.fail_transcription_at.borrow_mut() = Some(1);
    let failed = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(failed["ok"], false, "{failed}");
    assert_eq!(*f.offline.transcriptions.borrow(), 2);
    assert_eq!(*f.offline.extractions.borrow(), 0);
    let retry = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(
        retry["error"]["code"], "transcription_incomplete",
        "{retry}"
    );
    let direct = call(
        &f.ports,
        "office.extract",
        json!({"profile":"meeting", "id":source["id"]}),
    );
    assert_eq!(
        direct["error"]["code"], "transcription_incomplete",
        "{direct}"
    );
    assert_eq!(*f.offline.extractions.borrow(), 0);
    assert!(f.offline.tasks.borrow().is_empty());
}

#[test]
fn meeting_summary_refuses_a_source_edited_during_todo_filing() {
    let f = Fixture::new();
    let source = f.capture(json!({"event_key":"fixture-event", "text":"Ship the release."}));
    let path = std::path::Path::new(source["path"].as_str().unwrap());
    let markdown = f.ports.library.read_text(path).unwrap();
    let replacement = markdown.replace("Ship the release.", "Hold the release.");
    *f.offline.replace_on_add.borrow_mut() = Some((
        f.root.path().join("workspace/library").join(path),
        replacement,
    ));
    let run = call(&f.ports, "office.extract", json!({"profile":"meeting"}));
    assert_eq!(run["error"]["code"], "plan_changed", "{run}");
    let id = source["id"].as_str().unwrap();
    assert!(
        f.ports
            .library
            .source_sidecar(id, "summary.md")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.ports.library.read_source(id).unwrap().body,
        "Hold the release."
    );
}
