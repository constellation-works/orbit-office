//! The plugin manifest, schemas, goldens and code agree, and the plugin
//! tree is installable. The tree lives at `plugin/.orbit-plugin/` beside
//! this workspace.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;

fn plugin_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugin/.orbit-plugin")
}

fn manifest() -> String {
    fs::read_to_string(plugin_root().join("plugin.yaml")).expect("plugin.yaml is readable")
}

fn schema(ref_path: &str) -> serde_json::Value {
    let text = fs::read_to_string(plugin_root().join(ref_path)).expect("schema is readable");
    serde_json::from_str(&text).expect("schema is JSON")
}

#[test]
fn identity_matches_the_package() {
    let manifest = manifest();
    assert!(manifest.contains(&format!("name: {}", office_core::NAMESPACE)));
    assert!(manifest.contains(&format!("version: {}", office_core::VERSION)));
}

#[test]
fn declared_secrets_match_the_code() {
    let manifest = manifest();
    for name in office_core::SECRET_NAMES {
        assert!(
            manifest.contains(&format!("- name: {name}")),
            "secret {name}"
        );
    }
}

#[test]
fn each_tool_is_declared_and_its_schema_is_closed() {
    let manifest = manifest();
    for (verb, accepted, _) in office_core::tool_table() {
        assert!(
            manifest.contains(&format!("- name: {verb}")),
            "tool {verb} is declared"
        );
        let request = schema(&format!("schemas/{verb}.request.json"));
        assert_eq!(request["additionalProperties"], serde_json::json!(false));
        let properties: Vec<&str> = request["properties"]
            .as_object()
            .map(|props| props.keys().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(properties.len(), accepted.len(), "tool {verb} input keys");
        for key in accepted {
            assert!(properties.contains(&key), "tool {verb} accepts {key}");
        }
    }
}

#[test]
fn goldens_cover_every_tool() {
    let dir = plugin_root().join("tests/conformance");
    let mut text = String::new();
    for entry in fs::read_dir(dir).expect("conformance dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|ext| ext == "yaml") {
            text.push_str(&fs::read_to_string(path).expect("golden is readable"));
        }
    }
    assert!(!text.is_empty());
    for (verb, _, _) in office_core::tool_table() {
        assert!(text.contains(&format!("tool: {verb}")), "golden for {verb}");
    }
}

#[test]
fn plugin_tree_has_no_symlinks_and_a_launcher() {
    fn walk(dir: &PathBuf, offenders: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("dir is readable") {
            let path = entry.expect("entry").path();
            if path
                .symlink_metadata()
                .expect("metadata")
                .file_type()
                .is_symlink()
            {
                offenders.push(path.clone());
            }
            if path.is_dir() {
                walk(&path, offenders);
            }
        }
    }
    let mut offenders = Vec::new();
    walk(&plugin_root(), &mut offenders);
    assert!(
        offenders.is_empty(),
        "symlinks in plugin tree: {offenders:?}"
    );

    let launcher = plugin_root().join("bin/office");
    let metadata = fs::metadata(&launcher).expect("launcher exists");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert!(
            metadata.permissions().mode() & 0o111 != 0,
            "launcher is executable"
        );
    }
    #[cfg(not(unix))]
    let _ = metadata;
}
