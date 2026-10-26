//! Golden tests of the public surface, captured from the built binary as
//! it ships (STD-01 R24): the `--help` pages and the `office.status`
//! response envelope.
//!
//! A mismatch fails. After an intentional surface change, regenerate with
//! `make goldens UPDATE=1` (which sets `UPDATE_GOLDENS=1`) and review the
//! diff as a contract change.
#![allow(clippy::expect_used, clippy::unwrap_used)]

mod support;

use std::path::PathBuf;
use support::{Fixture, text};

const HELP: &[(&str, &[&str])] = &[
    ("root", &["--help"]),
    ("orbit-tool", &["orbit-tool", "--help"]),
];

const STATUS_REQUEST: &str =
    r#"{"schema_version":1,"tool":"office.status","input":{},"context":{}}"#;

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

fn updating() -> bool {
    std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1")
}

/// Compare `actual` to the golden at `rel`, or rewrite it when updating.
fn check(rel: &str, actual: &str) -> Option<String> {
    let path = golden_dir().join(rel);
    if updating() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, actual).unwrap();
        return None;
    }
    let expected = std::fs::read_to_string(&path).unwrap_or_default();
    (expected != actual).then(|| {
        format!(
            "golden {rel} differs\n--- expected ({})\n{expected}\n--- actual\n{actual}",
            path.display()
        )
    })
}

fn report(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{}\n\nIf the change is intended: make goldens UPDATE=1, then review the diff.",
        failures.join("\n\n")
    );
}

#[test]
fn help_matches_goldens() {
    let fx = Fixture::new();
    let failures: Vec<String> = HELP
        .iter()
        .filter_map(|(name, args)| {
            let out = fx.ok(args, "");
            check(&format!("help/{name}.txt"), &text(&out.stdout))
        })
        .collect();
    report(&failures);
}

#[test]
fn output_matches_goldens() {
    let fx = Fixture::new();
    let out = fx.ok(&["orbit-tool"], STATUS_REQUEST);
    let failures: Vec<String> = check("output/status.json", &text(&out.stdout))
        .into_iter()
        .collect();
    report(&failures);
}
