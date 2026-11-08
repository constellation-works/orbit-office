mod attach;
mod show;
mod verify;
mod write;

use std::path::PathBuf;

use super::library_support::{Env, RENEWAL_PAGE, git};

/// An initialized Library holding one meeting source, indexed.
pub(super) fn library_with_source(env: &Env) -> PathBuf {
    let init = env.tool("office.init", serde_json::json!({}));
    assert_eq!(init["ok"], serde_json::json!(true), "{init}");
    let library = env.library();
    let source = library.join("sources/2026/10/src-20261028-acme-qbr/source.md");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(
        &source,
        "---\nid: src-20261028-acme-qbr\nkind: meeting\ntitle: Acme QBR\nrelated: [dom-project-acme-renewal]\n---\n\n[00:10:00] Jane: Renewal is in March.\n\n[00:12:31] Jane: Price matters most.\n",
    )
    .unwrap();
    let reindex = env.tool("office.reindex", serde_json::json!({}));
    assert_eq!(reindex["output"]["indexed_documents"], serde_json::json!(1));
    library
}
