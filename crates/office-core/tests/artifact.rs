//! Public artifact rules and persisted metadata, without host state.
#![allow(clippy::expect_used, clippy::unwrap_used)]
use office_core::domain::artifact::{
    Approval, Approver, Artifact, ArtifactFile, ArtifactManifest, MAX_FILE_BYTES, Producer,
    decode_payload,
};
use serde_json::json;

fn artifact(
    kind: &str,
    series: &str,
    by: Approver,
) -> Result<Artifact, office_core::domain::artifact::ArtifactError> {
    let manifest: ArtifactManifest = serde_json::from_value(json!({
        "kind":kind,"title":"Artifact: quoted \"title\"","files":["output.bin"],"series":series,
        "cites":["src-19700101-evidence#L1"],"tags":["client:example"],"runbook":"rb-produce"
    }))
    .unwrap();
    Artifact::approved(
        manifest,
        "<id>".into(),
        Producer {
            crew: Some("fixture".into()),
            runbook: Some("rb-produce".into()),
            at: None,
        },
        Approval {
            by,
            at: "1970-01-01T00:00:00Z".into(),
            edited: true,
        },
    )
}

#[test]
fn unkeyed_routine_approval_is_accepted_only_for_the_daily_digest_report() {
    for kind in [
        "report", "doc", "draft", "script", "data", "review", "other",
    ] {
        for series in ["digest-daily", "other-report"] {
            let result = artifact(kind, series, Approver::Routine);
            if kind == "report" && series == "digest-daily" {
                assert!(result.is_ok());
                let a = result.unwrap();
                assert_eq!(
                    Artifact::from_markdown(&a.to_markdown().unwrap()).unwrap(),
                    a
                );
            } else {
                assert_eq!(
                    result.unwrap_err().code(),
                    "artifact_routine_approval_refused"
                );
            }
            assert!(artifact(kind, series, Approver::User).is_ok());
        }
    }
}

#[test]
fn meeting_prep_routine_approval_requires_a_keyed_report_and_roundtrips_the_key() {
    for kind in [
        "report", "doc", "draft", "script", "data", "review", "other",
    ] {
        let mut candidate = artifact(kind, "meeting-prep", Approver::User).unwrap();
        candidate.approved.by = Approver::Routine;
        for key in [
            None,
            Some(""),
            Some(" "),
            Some("weekly:2026-10-28T14:00:00Z"),
        ] {
            candidate.event_key = key.map(str::to_string);
            if kind == "report" && key.is_some_and(|key| !key.trim().is_empty()) {
                candidate.validate().unwrap();
                assert_eq!(
                    Artifact::from_markdown(&candidate.to_markdown().unwrap()).unwrap(),
                    candidate
                );
            } else {
                assert_eq!(
                    candidate.validate().unwrap_err().code(),
                    "artifact_routine_approval_refused"
                );
            }
        }
    }
}

#[test]
fn yaml_roundtrips_nested_provenance_approval_and_versions_without_inventing_times() {
    let first = artifact("report", "reports", Approver::User).unwrap();
    assert_eq!(
        Artifact::from_markdown(&first.to_markdown().unwrap()).unwrap(),
        first
    );
    let revised = first
        .next_version(std::slice::from_ref(&first), |_| false)
        .unwrap();
    assert_eq!(revised.version, 2);
    assert_eq!(revised.supersedes.as_deref(), Some(first.id.as_str()));
    assert_eq!(
        Artifact::from_markdown(&revised.to_markdown().unwrap()).unwrap(),
        revised
    );
    let invalid = first
        .to_markdown()
        .unwrap()
        .replace("by: \"user\"", "by: routine");
    assert_eq!(
        Artifact::from_markdown(&invalid).unwrap_err().code(),
        "artifact_routine_approval_refused"
    );
}

#[test]
fn markdown_bytes_are_exact_for_nested_empty_escaped_unicode_null_bool_and_numbers() {
    let mut a = artifact("report", "reports", Approver::User).unwrap();
    a.title = "Café \"quoted\" \\ 🌍".into();
    a.cites = Vec::new();
    a.tags = Vec::new();
    a.produced_by.crew = None;
    a.produced_by.runbook = None;
    let markdown = a.to_markdown().unwrap();
    assert_eq!(
        markdown,
        "---\n\
         approved: { at: \"1970-01-01T00:00:00Z\", by: \"user\", edited: true }\n\
         cites: []\n\
         files: [\"output.bin\"]\n\
         id: \"art-19700101-artifact-quoted-title\"\n\
         kind: \"report\"\n\
         produced_by: { at: null, crew: null, runbook: null }\n\
         sensitivity: \"normal\"\n\
         series: \"reports\"\n\
         shareable: false\n\
         summary: \"\"\n\
         supersedes: null\n\
         tags: []\n\
         task: \"<id>\"\n\
         title: \"Café \\\"quoted\\\" \\\\ 🌍\"\n\
         version: 1\n\
         ---\n"
    );
    assert_eq!(Artifact::from_markdown(&markdown).unwrap(), a);
}

#[test]
fn payloads_preserve_text_and_binary_and_refuse_ambiguous_oversize_or_invalid_encodings() {
    assert_eq!(
        decode_payload(&json!({"content":"hello 🌍","size":10})).unwrap(),
        "hello 🌍".as_bytes()
    );
    for (encoded, bytes) in [
        ("", vec![]),
        ("AA==", vec![0]),
        ("AP8=", vec![0, 255]),
        ("AP+A", vec![0, 255, 128]),
    ] {
        assert_eq!(
            decode_payload(&json!({"content_base64":encoded})).unwrap(),
            bytes
        );
    }
    for value in [
        json!({}),
        json!({"content":false}),
        json!({"content":"x","content_base64":"eA=="}),
        json!({"content":"x","size":2}),
    ] {
        assert_eq!(
            decode_payload(&value).unwrap_err().code(),
            "artifact_payload_invalid"
        );
    }
    for encoded in [
        "AA=", "AA===", "A===", "=AAA", "AA=A", "AB==", "AAB=", "AA==AAAA", "éé", "AA\n=",
    ] {
        assert_eq!(
            decode_payload(&json!({"content_base64":encoded}))
                .unwrap_err()
                .code(),
            "artifact_payload_invalid",
            "{encoded}"
        );
    }
    let value = json!({"content":"x".repeat(MAX_FILE_BYTES+1)});
    assert_eq!(
        decode_payload(&value).unwrap_err().code(),
        "artifact_too_large"
    );
    assert_eq!(
        decode_payload(&json!({"content":"x".repeat(MAX_FILE_BYTES)}))
            .unwrap()
            .len(),
        MAX_FILE_BYTES
    );
}

#[test]
fn every_bundle_file_is_bounded_and_paths_are_contained_and_unambiguous() {
    let mut a = artifact("data", "results", Approver::User).unwrap();
    for path in [
        "../outside",
        "/absolute",
        "a//b",
        "a/./b",
        "a\\b",
        "artifact.md",
        "nested/artifact.md",
        "a\nb",
        "C:file",
        "a/.git/config",
    ] {
        a.files = vec![path.into()];
        assert!(a.validate().is_err(), "{path}");
    }
    a.files = vec!["output.bin".into()];
    assert!(a.validate_bundle(&[]).is_err());
    let file = ArtifactFile {
        path: "output.bin".into(),
        bytes: vec![0; MAX_FILE_BYTES + 1],
    };
    assert_eq!(
        a.validate_bundle(&[file]).unwrap_err().code(),
        "artifact_too_large"
    );
    a.files = vec!["a".into(), "a/b".into()];
    assert!(a.validate().is_err());
    a.files = vec!["a".into(), "a".into()];
    assert!(a.validate().is_err());
}

#[test]
fn metadata_stays_at_its_canonical_path_and_versions_come_from_the_series() {
    let first = artifact("report", "reports", Approver::User).unwrap();
    let second = first
        .next_version(std::slice::from_ref(&first), |_| false)
        .unwrap();
    let allocated = second.next_version(&[], |_| false).unwrap();
    assert_eq!(allocated.version, 1);
    assert_eq!(allocated.supersedes, None);
    assert!(
        Artifact::from_document("artifacts/wrong/artifact.md", &first.to_markdown().unwrap())
            .is_err()
    );
    assert_eq!(
        Artifact::from_document(&first.path().unwrap(), &first.to_markdown().unwrap()).unwrap(),
        first
    );
}
