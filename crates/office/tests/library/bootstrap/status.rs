use super::super::library_support::{Env, git};

#[cfg(unix)]
#[test]
fn bootstrap_status_exec_reads_digest_and_proposal_callback_without_writing() {
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    assert_eq!(env.tool("office.init", json!({}))["ok"], true);
    let source = env.tool(
        "office.capture",
        json!({"kind":"doc","title":"Design","text":"Context.","tags":["bootstrap:docs"]}),
    );
    assert_eq!(source["ok"], true, "{source}");
    let path = source["output"]["path"].as_str().unwrap();
    let digest_path = env.library().join(path).with_file_name("digest.json");
    std::fs::write(
        digest_path,
        json!({"summary":"Design","subjects":[],"claims":[],"procedures":[],"terms":[],"refs":[]})
            .to_string(),
    )
    .unwrap();
    let bin = env.workspace.join("callback-bin");
    std::fs::create_dir(&bin).unwrap();
    let callback = bin.join("orbit");
    std::fs::write(&callback,"#!/bin/sh\n[ \"$3\" = orbit.task.list ] || exit 1\nprintf '%s' '{\"tasks\":[{\"id\":\"fixture\",\"status\":\"done\",\"tags\":[\"proposal\",\"proposal:knowledge_add\",\"bootstrap:docs\"]}],\"total\":1,\"truncated\":false}'\n").unwrap();
    std::fs::set_permissions(&callback, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let before = git(&env.library(), &["status", "--porcelain"]);
    // A report must not recreate a missing index or plugin-state directory.
    std::fs::rename(&env.state, env.workspace.join("saved-plugin-state")).unwrap();
    let response = env.tool_with_path("office.bootstrap-status", json!({}), &path);
    assert_eq!(response["ok"], true, "{response}");
    assert_eq!(response["output"]["phases"][0]["captured"], 1);
    assert_eq!(response["output"]["phases"][0]["digested"], 1);
    assert_eq!(response["output"]["phases"][0]["accepted"], 1);
    assert_eq!(git(&env.library(), &["status", "--porcelain"]), before);
    assert!(!env.library().join(".office/bootstrap.json").exists());
    assert!(!env.state.exists());
    std::fs::rename(env.workspace.join("saved-plugin-state"), &env.state).unwrap();
    let journal = env.state.join("library.pending-write.json");
    std::fs::write(&journal, "pending mutation must stay untouched").unwrap();
    let refused = env.tool_with_path("office.bootstrap-status", json!({}), &path);
    assert_eq!(
        refused["error"]["code"], "library_write_pending",
        "{refused}"
    );
    assert_eq!(
        std::fs::read_to_string(journal).unwrap(),
        "pending mutation must stay untouched"
    );
}
