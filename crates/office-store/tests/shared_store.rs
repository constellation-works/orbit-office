//! The shared clone adapter against a real `file` remote, plus the exact git
//! child environment and transport policy.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]
use office_core::domain::sharing::SharedSettings;
use office_core::ports::{PushOutcome, PushRequest, SharedRepository};
use office_store::{SharedStore, git_command, git_environment};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

#[test]
fn git_children_get_only_the_allowlist_and_the_transport_overrides() {
    let ambient = [
        "PATH",
        "HOME",
        "LANG",
        "SSH_AUTH_SOCK",
        "GIT_DIR",
        "GIT_SSH_COMMAND",
        "GIT_CONFIG_PARAMETERS",
        "GIT_ASKPASS",
        "AWS_SECRET_ACCESS_KEY",
    ];
    let environment = git_environment(|name| {
        ambient
            .contains(&name)
            .then(|| OsString::from(format!("{name}-value")))
    });
    let names: Vec<&str> = environment.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "PATH",
            "HOME",
            "LANG",
            "SSH_AUTH_SOCK",
            "GIT_TERMINAL_PROMPT"
        ]
    );
    assert_eq!(environment[4].1, "0");
    let command = git_command(Path::new("/clone"), &environment, &["status"]);
    let envs: Vec<(String, Option<String>)> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert_eq!(envs.len(), 5, "{envs:?}");
    assert!(envs.iter().all(|(name, _)| names.contains(&name.as_str())));
    let args: Vec<String> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    let overrides: Vec<&str> = args
        .windows(2)
        .filter(|pair| pair[0] == "-c")
        .map(|pair| pair[1].as_str())
        .collect();
    for required in [
        "protocol.allow=never",
        "protocol.ssh.allow=always",
        "protocol.https.allow=always",
        "protocol.file.allow=always",
        "core.hooksPath=/dev/null",
    ] {
        assert!(overrides.contains(&required), "{args:?}");
    }
    assert_eq!(args.last().map(String::as_str), Some("status"));
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    remote: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join(".gitconfig"),
            "[user]\n\tname = Fixture Member\n\temail = member@example.invalid\n[init]\n\tdefaultBranch = main\n",
        )
        .unwrap();
        let remote = root.join("team.git");
        let fixture = Self {
            _dir: dir,
            root,
            home,
            remote,
        };
        fixture.git(&fixture.root, &["init", "--quiet", "--bare", "team.git"]);
        fixture
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(cwd)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap())
            .env("HOME", &self.home)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn store(&self, name: &str) -> SharedStore {
        let state = self.root.join(name);
        let environment = vec![
            ("PATH".to_string(), std::env::var_os("PATH").unwrap()),
            ("HOME".to_string(), self.home.clone().into_os_string()),
            ("GIT_TERMINAL_PROMPT".to_string(), OsString::from("0")),
        ];
        SharedStore::new(state, environment)
    }

    fn settings(&self) -> SharedSettings {
        SharedSettings {
            remote: self.remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            member: "me".into(),
        }
    }
}

fn request(base: Option<String>, path: &str, text: &str) -> PushRequest {
    PushRequest {
        base,
        writes: vec![(path.into(), text.as_bytes().to_vec())],
        removes: Vec::new(),
        message: "me: share 1 document, remove 0".into(),
    }
}

#[test]
fn push_fetch_and_rejection_against_a_file_remote_run_no_hooks() {
    let fixture = Fixture::new();
    let settings = fixture.settings();
    let mine = fixture.store("mine");
    let theirs = fixture.store("theirs");
    let empty = mine.fetch(&settings).unwrap();
    assert_eq!(empty.head, None);
    assert!(empty.files.is_empty());
    // A hook in the clone and one on the remote must never run.
    let marker = fixture.root.join("hook-ran");
    let hook = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    for dir in [
        fixture.root.join("mine/shared/repo/.git/hooks"),
        fixture.remote.join("hooks"),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        for name in [
            "pre-push",
            "pre-receive",
            "post-receive",
            "reference-transaction",
        ] {
            let path = dir.join(name);
            std::fs::write(&path, &hook).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    theirs.fetch(&settings).unwrap();
    let PushOutcome::Pushed(first) = mine
        .push(
            &settings,
            &request(None, ".office-shared.yaml", "format: 1\n"),
        )
        .unwrap()
    else {
        panic!("first push rejected")
    };
    assert!(first.head.is_some());
    assert_eq!(first.files.len(), 1);
    assert!(!marker.exists(), "a git hook ran");
    let log = fixture.git(&fixture.remote, &["log", "--format=%an <%ae>|%s", "main"]);
    assert_eq!(
        log,
        "Fixture Member <member@example.invalid>|me: share 1 document, remove 0\n"
    );
    // A stale base is not a fast-forward.
    assert_eq!(
        theirs
            .push(&settings, &request(None, "knowledge/domain/dom-a.md", "x"))
            .unwrap(),
        PushOutcome::Rejected
    );
    let fetched = theirs.fetch(&settings).unwrap();
    assert_eq!(fetched, first);
    assert_eq!(theirs.read(&fetched.files[0].blob).unwrap(), b"format: 1\n");
    // The working tree is untouched until checkout.
    assert!(
        !fixture
            .root
            .join("theirs/shared/repo/.office-shared.yaml")
            .exists()
    );
    theirs.checkout(fetched.head.as_deref()).unwrap();
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("theirs/shared/repo/.office-shared.yaml"))
            .unwrap(),
        "format: 1\n"
    );
}

#[test]
fn unreachable_remote_names_the_host_only_and_a_second_lock_is_busy() {
    let fixture = Fixture::new();
    let mut settings = fixture.settings();
    settings.remote = fixture
        .root
        .join("missing.git")
        .to_string_lossy()
        .into_owned();
    let store = fixture.store("state");
    let error = store.fetch(&settings).unwrap_err();
    assert_eq!(error.code(), "shared_remote_unreachable");
    let message = error.to_string();
    assert!(message.contains("local"), "{message}");
    assert!(
        !message.contains("missing.git") && !message.contains("fatal"),
        "{message}"
    );
    store.lock().unwrap();
    assert_eq!(
        fixture.store("state").lock().unwrap_err().code(),
        "library_writer_busy"
    );
}
