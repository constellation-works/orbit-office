//! Shared fixtures and helpers for the `library_store` integration tests.
//! Included by each sibling test file as a `#[path]` module; also compiles
//! standalone as its own test target with no tests.
#![allow(clippy::expect_used, clippy::unwrap_used, dead_code)]

use office_store::{DocumentKind, LibraryStore, SearchFilter, SearchHit};
use std::path::Path;

pub(crate) const MEETING: &str = "\
---
id: src-20261028-acme-qbr
kind: meeting
title: Acme QBR
summary: Renewal pricing discussion.
tags: [client:acme]
sensitivity: normal
---

[00:12:31] Jane Park: We cannot go above three percent this year.
";

pub(crate) const DOMAIN: &str = "\
---
id: dom-acme-renewal
type: domain
title: Acme renewal
summary: Acme's contract renews in March; Jane Park decides.
tags: [client:acme]
cites:
  - src-20261028-acme-qbr#t=00:12:31
related: [rb-client-qbr-prep]
---

Acme's contract renews in March [src-20261028-acme-qbr#t=00:12:31].
";

pub(crate) fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write fixture");
}

pub(crate) fn git(root: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.test");
    let (status, output) = office_common::process::run(&mut command, root).expect("git fixture");
    assert!(status.success(), "git fixture {args:?} exited {status}");
    output
}

pub(crate) fn initialize(root: &Path) {
    std::fs::create_dir_all(root.join(".office")).expect("config dir");
    std::fs::write(root.join(".office/config.yaml"), "").expect("config");
    git(root, &["init", "--quiet"]);
}

pub(crate) fn upload_request(root: &Path) -> office_core::ports::CaptureSourceRequest {
    let upload = root.join("upload.bin");
    std::fs::write(&upload, [0, 255, 1, 128, 2]).expect("binary upload");
    office_core::ports::CaptureSourceRequest {
        captured_at: "2026-10-27T10:00:00+00:00".into(),
        title: Some("Upload".into()),
        upload_path: Some(upload.to_string_lossy().into()),
        ..Default::default()
    }
}

pub(crate) const UPLOAD_DIR: &str = "sources/note/2026/10/src-20261027-upload";

pub(crate) fn search_sorted(store: &LibraryStore, query: &str) -> Vec<SearchHit> {
    let mut hits = store
        .search(query, &SearchFilter::default())
        .expect("search");
    hits.sort_by(|left, right| left.id.cmp(&right.id));
    hits
}

pub(crate) fn search_sorted_kind(
    store: &LibraryStore,
    query: &str,
    kind: DocumentKind,
) -> Vec<SearchHit> {
    let mut hits = store
        .search(
            query,
            &SearchFilter {
                kind: Some(kind),
                ..SearchFilter::default()
            },
        )
        .expect("search");
    hits.sort_by(|left, right| left.id.cmp(&right.id));
    hits
}

pub(crate) fn token_hit(store: &LibraryStore, id: &str, token: &str) -> SearchHit {
    let hits = search_sorted(store, token);
    assert_eq!(hits.len(), 1, "{id}");
    assert_eq!(hits[0].id, id);
    hits.into_iter().next().expect("one hit")
}

pub(crate) const SOURCE_ONE: &str = "\
---
id: src-1
kind: note
title: Source one
---

sourcetoken keptafterreindex
";

pub(crate) const DOMAIN_ONE: &str = "\
---
id: dom-a
type: domain
title: Domain one
summary: A page that must survive reindex.
---

domaintoken keptafterreindex
";

pub(crate) const RUNBOOK_ONE: &str = "\
---
id: rb-a
type: runbook
title: Runbook one
---

runbooktoken keptafterreindex
";

pub(crate) const ARTIFACT_ONE: &str = "\
---
id: art-1
kind: report
title: Artifact one
series: renewal-research
---

artifacttoken keptafterreindex
";

#[cfg(unix)]
pub(crate) fn library_with_links_outside(
    link_knowledge_file: bool,
) -> (tempfile::TempDir, LibraryStore) {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("tempdir");
    let library = dir.path().join("library");
    let state = dir.path().join("state");
    initialize(&library);
    write(
        &library,
        "knowledge/domain/inside.md",
        "---\nid: dom-inside\ntype: domain\ntitle: Inside page\n---\n\ninside library page\n",
    );
    write(
        &library,
        "sources/2026/10/src-inside/source.md",
        "---\nid: src-inside\nkind: note\ntitle: Inside source\n---\n\ninside library source\n",
    );
    let outside = dir.path().join("outside");
    write(
        &outside,
        "secret.md",
        "---\nid: dom-outside-file\ntype: domain\ntitle: Outside file\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "tree/page.md",
        "---\nid: dom-outside-dir\ntype: domain\ntitle: Outside dir\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "source.md",
        "---\nid: src-outside-file\nkind: note\ntitle: Outside source file\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "source-tree/source.md",
        "---\nid: src-outside-dir\nkind: note\ntitle: Outside source dir\n---\n\nOUTSIDE library secret\n",
    );
    write(
        &outside,
        "artifact-tree/artifact.md",
        "---\nid: art-outside\nkind: doc\ntitle: Outside artifact\n---\n\nOUTSIDE library secret\n",
    );
    let store = LibraryStore::open(&library, &state).expect("open");
    if link_knowledge_file {
        symlink(
            outside.join("secret.md"),
            library.join("knowledge/domain/linked.md"),
        )
        .expect("knowledge file symlink");
    }
    symlink(outside.join("tree"), library.join("knowledge/outside"))
        .expect("knowledge dir symlink");
    std::fs::create_dir_all(library.join("sources/2026/10/src-linked")).expect("linked source dir");
    symlink(
        outside.join("source.md"),
        library.join("sources/2026/10/src-linked/source.md"),
    )
    .expect("source file symlink");
    symlink(outside.join("source-tree"), library.join("sources/linked"))
        .expect("sources dir symlink");
    let artifacts = library.join("artifacts");
    std::fs::remove_dir_all(&artifacts).expect("remove real artifacts layer");
    symlink(outside.join("artifact-tree"), &artifacts).expect("artifacts layer symlink");
    (dir, store)
}

pub(crate) fn markdown_under(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                found.push(
                    path.strip_prefix(root)
                        .expect("relative")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    found.sort();
    found
}

pub(crate) fn artifact_candidate() -> office_core::domain::artifact::Artifact {
    use office_core::domain::artifact::{Approval, Approver, Artifact, ArtifactManifest, Producer};
    Artifact::approved(
        serde_json::from_value::<ArtifactManifest>(serde_json::json!({
            "kind":"report","title":"Promoted output","files":["report.md","data/results.bin"],
            "series":"fixture-series","summary":"Approved fixture", "cites":[]
        }))
        .unwrap(),
        "<id>".into(),
        Producer {
            crew: Some("fixture-crew".into()),
            runbook: Some("rb-research".into()),
            at: Some("2026-10-27T10:00:00Z".into()),
        },
        Approval {
            by: Approver::User,
            at: "2026-10-27T11:00:00Z".into(),
            edited: true,
        },
    )
    .unwrap()
}

pub(crate) fn artifact_files() -> Vec<office_core::domain::artifact::ArtifactFile> {
    use office_core::domain::artifact::ArtifactFile;
    vec![
        ArtifactFile {
            path: "report.md".into(),
            bytes: b"Approved report\n".to_vec(),
        },
        ArtifactFile {
            path: "data/results.bin".into(),
            bytes: vec![0, 255, 128, 3],
        },
    ]
}
