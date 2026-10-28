//! Shared library sync rules through the pure public API.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::domain::sharing::sync::{
    PathState, SharedPath, SyncState, classify, layout_valid, localized_view, page_path,
    remote_host, same_content,
};
use office_core::domain::sharing::{self, SharedSettings};

fn settings(remote: &str, branch: &str, member: &str) -> SharedSettings {
    SharedSettings {
        remote: remote.into(),
        branch: branch.into(),
        member: member.into(),
    }
}

#[test]
fn layout_marker_is_exactly_format_one() {
    assert!(layout_valid(b"format: 1\n"));
    for bad in [
        &b"format: 2\n"[..],
        b"format: 1\nextra: true\n",
        b"",
        b"- format: 1\n",
        b"\xff\xfe",
    ] {
        assert!(!layout_valid(bad), "{bad:?}");
    }
}

#[test]
fn classify_accepts_only_the_documented_layout() {
    assert_eq!(classify(".office-shared.yaml"), SharedPath::Layout);
    assert_eq!(
        classify("knowledge/domain/dom-topic.md"),
        SharedPath::Page {
            id: "dom-topic".into()
        }
    );
    assert_eq!(
        classify("knowledge/runbooks/rb-deploy.md"),
        SharedPath::Page {
            id: "rb-deploy".into()
        }
    );
    let dir = "members/alice/sources/2026/10/src-20261028-call";
    assert_eq!(
        classify(&format!("{dir}/source.md")),
        SharedPath::Document {
            member: "alice".into(),
            dir: dir.into()
        }
    );
    for sidecar in [
        "frames.md",
        "chapters.md",
        "digest.json",
        "summary.md",
        "v2.md",
    ] {
        assert!(
            matches!(
                classify(&format!("{dir}/{sidecar}")),
                SharedPath::Companion { source: true, .. }
            ),
            "{sidecar}"
        );
    }
    let art = "members/alice/artifacts/2026/10/art-deck";
    assert!(matches!(
        classify(&format!("{art}/deck.pptx")),
        SharedPath::Companion { source: false, .. }
    ));
    for outside in [
        "knowledge/domain/rb-wrong.md",
        "knowledge/runbooks/dom-wrong.md",
        "knowledge/domain/Upper.md",
        "knowledge/elsewhere.md",
        "README.md",
        "members/Alice/sources/2026/10/src-x/source.md",
        "members/alice/sources/2026/10/art-x/source.md",
        "members/alice/sources/26/10/src-x/source.md",
        &format!("{dir}/original.mp4"),
        &format!("{dir}/notes.txt"),
        &format!("{dir}/.hidden"),
        &format!("{art}/original.pdf"),
        &format!("{dir}/nested/source.md"),
    ] {
        assert_eq!(classify(outside), SharedPath::Outside, "{outside}");
    }
    assert_eq!(
        page_path("dom-topic").as_deref(),
        Some("knowledge/domain/dom-topic.md")
    );
    assert_eq!(page_path("src-x"), None);
    assert_eq!(page_path("dom-../x"), None);
}

#[test]
fn state_belongs_only_to_the_same_remote_branch_and_member() {
    let base = settings("/srv/team.git", "main", "alice");
    let mut state = SyncState::fresh(&base);
    state.paths.insert(
        "knowledge/domain/dom-a.md".into(),
        PathState {
            seen: Some("b1".into()),
            base: Some("b1".into()),
            own: Some("d1".into()),
        },
    );
    assert!(state.belongs_to(&base));
    for other in [
        settings("/srv/other.git", "main", "alice"),
        settings("/srv/team.git", "next", "alice"),
        settings("/srv/team.git", "main", "bob"),
    ] {
        assert!(!state.belongs_to(&other));
    }
    let encoded = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<SyncState>(&encoded).unwrap(), state);
    assert!(serde_json::from_str::<SyncState>(&encoded.replacen('{', "{\"token\":1,", 1)).is_err());
}

#[test]
fn remote_host_never_carries_users_or_paths() {
    assert_eq!(remote_host("/srv/team.git"), "local");
    assert_eq!(remote_host("file:///srv/team.git"), "local");
    assert_eq!(
        remote_host("https://git.example.invalid/team.git"),
        "git.example.invalid"
    );
    assert_eq!(
        remote_host("ssh://git@git.example.invalid:2222/team.git"),
        "git.example.invalid"
    );
    assert_eq!(
        remote_host("git@git.example.invalid:team.git"),
        "git.example.invalid"
    );
}

#[test]
fn localized_view_round_trips_through_the_shared_view() {
    let own = "---\nid: dom-person-jane\ntitle: Jane\ntemplate: person\nsensitivity: normal\nshareable: true\ncites: [src-one#L1]\nrefs:\n  - {label: Email, contact: jane@example.invalid}\n  - {label: Deck, artifact: art-one}\n---\nJane owns this [src-one#L1].\n";
    let mine = sharing::shared_view(own, "me", |_| true, |_| false).unwrap();
    // A colleague edits the body and adds a citation of their own source.
    let theirs = mine
        .replace("shared_by: \"me\"", "shared_by: \"bob\"")
        .replace("Jane owns this", "Jane leads this")
        .replace("[me/src-one#L1]", "[me/src-one#L1; bob/src-two#L3]");
    assert!(!same_content(&theirs, &mine));
    let localized = localized_view(&theirs, own, "me").unwrap();
    assert!(
        localized.contains("contact: \"jane@example.invalid\""),
        "{localized}"
    );
    assert!(!localized.contains("shared_by"));
    assert!(localized.contains("shareable: true"));
    assert!(localized.ends_with("Jane leads this [src-one#L1; bob/src-two#L3].\n"));
    let reshared = sharing::shared_view(&localized, "me", |_| true, |_| false).unwrap();
    assert!(same_content(&reshared, &theirs), "{reshared}\n{theirs}");
    assert!(!same_content(&reshared, "not a page"));
}
