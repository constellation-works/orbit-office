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

/// Tool names the manifest declares: uncommented `- name:` entries under
/// `tools:`, up to the next top-level `spec` key.
fn declared_tools(manifest: &str) -> Vec<String> {
    let mut in_tools = false;
    let mut names = Vec::new();
    for line in manifest.lines() {
        let trimmed = line.trim_start();
        if line.starts_with("  ") && !line.starts_with("   ") && !trimmed.starts_with('#') {
            in_tools = trimmed == "tools:";
            continue;
        }
        if in_tools && let Some(name) = trimmed.strip_prefix("- name: ") {
            names.push(name.trim().to_string());
        }
    }
    names
}

#[test]
fn the_manifest_declares_exactly_the_built_tools() {
    let manifest = manifest();
    let declared = declared_tools(&manifest);
    for tool in office_tools::tools() {
        let name = tool.spec().name;
        if tool.is_built() {
            assert!(
                declared.iter().any(|d| d == name),
                "built tool {name} is declared"
            );
        } else {
            // STD-01 R36: a planned tool is never advertised, but keeps its
            // reserved, commented slot so building it edits one hunk.
            assert!(
                !declared.iter().any(|d| d == name),
                "planned tool {name} is not declared"
            );
            let slot = format!("# - name: {name}");
            assert!(
                manifest.lines().any(|line| line.trim() == slot),
                "planned tool {name} keeps its commented slot in plugin.yaml"
            );
        }
    }
    let known: Vec<&str> = office_tools::tools()
        .iter()
        .map(|t| t.spec().name)
        .collect();
    for name in &declared {
        assert!(
            known.contains(&name.as_str()),
            "declared tool {name} is in the registry"
        );
    }
}

#[test]
fn each_manifest_block_matches_its_tool_spec() {
    // Built or reserved, a tool's block carries the kind and scope its
    // ToolSpec declares, so uncommenting a reserved block is all it takes.
    let manifest = manifest();
    let lines: Vec<&str> = manifest.lines().collect();
    let uncomment = |line: &str| line.trim().trim_start_matches('#').trim().to_string();
    for tool in office_tools::tools() {
        let spec = tool.spec();
        let header = format!("- name: {}", spec.name);
        let start = lines
            .iter()
            .position(|line| uncomment(line) == header)
            .unwrap_or_else(|| panic!("no manifest block for {}", spec.name));
        let block: Vec<String> = lines[start + 1..]
            .iter()
            .take_while(|line| !line.trim().is_empty() && !uncomment(line).starts_with("- name:"))
            .map(|line| uncomment(line))
            .collect();
        let kind = format!("execution_kind: {}", spec.execution_kind.as_str());
        let scope = format!("mcp_scope: {}", spec.mcp_scope.as_str());
        assert!(block.contains(&kind), "{}: {kind}", spec.name);
        assert!(block.contains(&scope), "{}: {scope}", spec.name);
    }
}

#[test]
fn each_built_tool_has_a_closed_schema_matching_its_keys() {
    for tool in office_tools::tools().iter().filter(|tool| tool.is_built()) {
        let verb = tool.spec().name;
        let accepted = tool.accepts();
        let request = schema(&format!("schemas/{verb}.request.json"));
        assert_eq!(request["additionalProperties"], serde_json::json!(false));
        let properties: Vec<&str> = request["properties"]
            .as_object()
            .map(|props| props.keys().map(String::as_str).collect())
            .unwrap_or_default();
        assert_eq!(properties.len(), accepted.len(), "tool {verb} input keys");
        for key in accepted {
            assert!(properties.contains(key), "tool {verb} accepts {key}");
        }
        schema(&format!("schemas/{verb}.response.json"));
    }
}

#[test]
fn goldens_cover_every_built_tool() {
    let dir = plugin_root().join("tests/conformance");
    let mut text = String::new();
    for entry in fs::read_dir(dir).expect("conformance dir") {
        let path = entry.expect("entry").path();
        if path.extension().is_some_and(|ext| ext == "yaml") {
            text.push_str(&fs::read_to_string(path).expect("golden is readable"));
        }
    }
    assert!(!text.is_empty());
    for tool in office_tools::tools().iter().filter(|tool| tool.is_built()) {
        let verb = tool.spec().name;
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
