//! Shared temporary Library harness and fixtures for the `library` target.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(super) const BIN: &str = env!("CARGO_BIN_EXE_office");

pub(super) struct Env {
    pub(super) _dir: tempfile::TempDir,
    pub(super) workspace: PathBuf,
    pub(super) state: PathBuf,
}

impl Env {
    pub(super) fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let state = dir.path().join("plugin_state");
        Self {
            _dir: dir,
            workspace,
            state,
        }
    }

    /// A fresh Library for scenarios whose first step is a successful init.
    #[track_caller]
    pub(super) fn with_initialized_library() -> Self {
        let env = Self::new();
        assert_eq!(env.tool("office.init", serde_json::json!({}))["ok"], true);
        env
    }

    pub(super) fn library(&self) -> PathBuf {
        self.workspace.join("library")
    }

    /// Run the binary with one envelope; `path` replaces the inherited PATH.
    pub(super) fn tool_with_path(
        &self,
        tool: &str,
        input: serde_json::Value,
        path: &str,
    ) -> serde_json::Value {
        let context = serde_json::json!({
            "workspace_root": self.workspace,
            "plugin_state": self.state,
        });
        self.run_envelope(tool, input, context, path)
    }

    /// Run the binary with one envelope and exactly this context.
    pub(super) fn run_envelope(
        &self,
        tool: &str,
        input: serde_json::Value,
        context: serde_json::Value,
        path: &str,
    ) -> serde_json::Value {
        let request = serde_json::json!({
            "schema_version": 1,
            "tool": tool,
            "input": input,
            "context": context,
        })
        .to_string();
        let mut child = Command::new(BIN)
            .args(["orbit-tool"])
            .env_clear()
            .env("HOME", self.workspace.join("home"))
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(request.as_bytes())
            .expect("write request");
        let out = child.wait_with_output().expect("wait");
        assert_eq!(
            out.status.code(),
            Some(0),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).expect("one JSON document")
    }

    /// `library_dir` under `context.config`, the plugin config the adapter reads.
    pub(super) fn tool_with_config_library_dir(
        &self,
        tool: &str,
        input: serde_json::Value,
        library_dir: &str,
    ) -> serde_json::Value {
        let path = std::env::var("PATH").expect("PATH");
        self.tool_in(
            tool,
            input,
            &path,
            serde_json::json!({ "config": { "library_dir": library_dir } }),
        )
    }

    /// Top-level `context.library_dir`, the adapter's fallback when config omits it.
    pub(super) fn tool_with_context_library_dir(
        &self,
        tool: &str,
        input: serde_json::Value,
        library_dir: &str,
    ) -> serde_json::Value {
        let path = std::env::var("PATH").expect("PATH");
        self.tool_in(
            tool,
            input,
            &path,
            serde_json::json!({ "library_dir": library_dir }),
        )
    }

    /// Run the binary merging `extra_context` into the default context.
    fn tool_in(
        &self,
        tool: &str,
        input: serde_json::Value,
        path: &str,
        extra_context: serde_json::Value,
    ) -> serde_json::Value {
        let mut context = serde_json::json!({
            "workspace_root": self.workspace,
            "plugin_state": self.state,
        });
        if let (Some(context), Some(extra)) = (context.as_object_mut(), extra_context.as_object()) {
            for (key, value) in extra {
                context.insert(key.clone(), value.clone());
            }
        }
        self.run_envelope(tool, input, context, path)
    }

    pub(super) fn tool(&self, tool: &str, input: serde_json::Value) -> serde_json::Value {
        let path = std::env::var("PATH").expect("PATH");
        self.tool_with_path(tool, input, &path)
    }
}

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("HOME", root)
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("git utf8")
}

pub(super) const SOURCE: &str = "---\nid: src-20261028-acme-qbr\nkind: meeting\ntitle: Acme QBR\nsummary: Renewal pricing.\n---\n\n[00:12:31] Jane: searchable quixotic term here.\n";
pub(super) const DOMAIN: &str = "---\nid: dom-acme\ntitle: Acme\n---\n\nAcme quixotic page.\n";

pub(super) const RENEWAL_PAGE: &str = "---\nid: dom-project-acme-renewal\ntype: domain\ntemplate: project\ntitle: Acme renewal\naliases: [the renewal]\ncites:\n  - src-20261028-acme-qbr#t=00:12:31\nrefs:\n  - { label: Renewal tickets, account: \"jira:ACME\" }\n  - { label: Jane Park, contact: jane@acme.example }\n---\n## Status\nRenewal is due in March [src-20261028-acme-qbr#t=00:10:00].\n\n## Background\nPrice sensitivity is high [src-20261028-acme-qbr#t=00:12:31].\n";

pub(super) const BUNDLED_TEMPLATES: &[&str] = &[
    "client.md",
    "glossary.md",
    "goals.md",
    "person.md",
    "project.md",
    "rhythm.md",
    "runbook.md",
    "standing-directions.md",
    "systems.md",
    "topic.md",
    "user.md",
];
