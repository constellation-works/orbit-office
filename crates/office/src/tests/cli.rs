//! The parse surface: the one subcommand parses, everything else is a
//! usage error, and every documented command line parses (STD-04 R14).

use crate::cli::{Cli, Command};
use std::ffi::OsString;

fn args(words: &[&str]) -> Vec<OsString> {
    std::iter::once(OsString::from("office"))
        .chain(words.iter().map(OsString::from))
        .collect()
}

#[test]
fn orbit_tool_parses() {
    let cli = Cli::parse_args(&args(&["orbit-tool"])).expect("orbit-tool parses");
    assert!(matches!(cli.command, Command::OrbitTool));
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let error = Cli::parse_args(&args(&["note", "list"])).expect_err("notes are not a command");
    assert_eq!(error.exit_code(), 2);
}

#[test]
fn every_documented_command_line_parses() {
    // Examples in help and in the README are run through the real parser,
    // so an example naming a removed command fails here.
    let mut documented: Vec<String> = Vec::new();
    let help = {
        let mut cmd = <Cli as clap::CommandFactory>::command();
        cmd.render_long_help().to_string()
    };
    if let Some(examples) = help.split("Examples:").nth(1) {
        documented.extend(examples.lines().map(str::to_owned));
    }
    let readme = concat!(env!("CARGO_MANIFEST_DIR"), "/../../README.md");
    let readme = std::fs::read_to_string(readme).expect("README.md next to the workspace");
    documented.extend(readme.lines().map(str::to_owned));

    let mut parsed = 0;
    for line in &documented {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("office ") else {
            continue;
        };
        let command = rest.split(" | ").next().unwrap_or(rest);
        let command = command.split("  #").next().unwrap_or(command);
        let words: Vec<OsString> = std::iter::once(OsString::from("office"))
            .chain(
                command
                    .split_whitespace()
                    .take_while(|word| {
                        !matches!(*word, "<" | ">" | "request.json" | "response.json")
                    })
                    .map(OsString::from),
            )
            .collect();
        if words.len() < 2 {
            continue;
        }
        match Cli::parse_args(&words) {
            Ok(_) => {}
            // `--help`/`--version` parse fine and exit by rendering: clap
            // reports them as errors with a display kind.
            Err(error)
                if matches!(
                    error.kind(),
                    clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
                ) => {}
            Err(error) => panic!("documented line `{line}` does not parse:\n{error}"),
        }
        parsed += 1;
    }
    assert!(parsed >= 2, "only {parsed} documented command lines found");
}
