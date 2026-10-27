//! ScriptRunner public API with a finite, offline uv stand-in. No ambient mutation.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use office_core::{domain::script::*, ports::ScriptRunner};
use office_store::ScriptStore;
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

struct Fixture {
    _root: tempfile::TempDir,
    library: PathBuf,
    state: PathBuf,
    store: ScriptStore,
    execution: ScriptExecution,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let library = root.path().join("library");
        let state = root.path().join("state");
        let bin = root.path().join("bin");
        for dir in [&library, &bin] {
            fs::create_dir(dir).unwrap();
        }
        let script = b"# /// script\n# dependencies = []\n# ///\nprint('approved')\n";
        fs::write(library.join("script.py"), script).unwrap();
        fs::write(bin.join("uv"), STUB).unwrap();
        fs::set_permissions(bin.join("uv"), fs::Permissions::from_mode(0o700)).unwrap();
        let environment = vec![
            ("PATH".into(), bin.as_os_str().into()),
            ("LANG".into(), "C.UTF-8".into()),
            ("TZ".into(), "UTC".into()),
            ("META_API_KEY".into(), "never-pass-this".into()),
            ("AMBIENT".into(), "also-forbidden".into()),
        ];
        let store = ScriptStore::new(
            library.clone(),
            state.clone(),
            root.path().join(".orbit/tmp"),
            environment,
        );
        let hash = office_common::fs::hash::sha256_hex(script);
        let request:ScriptRequest=serde_json::from_value(json!({"artifact":"art-19700101-script","sha256":hash,"inputs":[],"args":[],"output":{"title":"Results","kind":"data","series":"results"}})).unwrap();
        let execution = ScriptExecution {
            script: ScriptPreparation {
                path: "script.py".into(),
                sha256: hash,
            },
            request,
            inputs: vec![],
            run_id: Some("jrun-20261027-0517-c18".into()),
        };
        Self {
            _root: root,
            library,
            state,
            store,
            execution,
        }
    }
    fn prepare(&self) {
        self.store.prepare(&self.execution.script).unwrap();
    }
    fn runs(&self) -> Vec<PathBuf> {
        fs::read_dir(self.state.join("runs"))
            .map(|e| e.map(|e| e.unwrap().path()).collect())
            .unwrap_or_default()
    }
    fn run(&self) -> office_core::Result<(ScriptResult, Vec<ScriptFile>)> {
        self.store.run(&self.execution, &ScriptBounds::default())
    }
}
const STUB: &str = r#"#!/usr/bin/python3 -S
import os,sys,json,pathlib,stat,subprocess,signal,time
p=pathlib.Path
if sys.argv[1]=='lock':
    p('script.py.lock').write_text('locked dependencies')
    sys.exit(0)
if sys.argv[1]=='sync':
    if b'bad-deps' in p('script.py').read_bytes(): sys.exit(9)
    p(os.environ['UV_CACHE_DIR'],'downloaded').write_text('once')
    sys.exit(0)
assert sys.argv[1]=='run' and '--offline' in sys.argv and '--locked' in sys.argv
assert '--python' in sys.argv and sys.argv[sys.argv.index('--python')+1]=='3.12'
args=sys.argv[sys.argv.index('script.py')+1:]
p('tmp/record.json').write_text(json.dumps({'env':dict(os.environ),'cwd':os.getcwd(),'args':args,'readonly':all(not x.stat().st_mode & 0o222 for x in p('in').rglob('*')) and not p('in').stat().st_mode & 0o222}))
mode=args[0] if args else 'empty'
if mode=='outputs':
    p('out/report.csv').write_text('a,b\n1,2\n')
    p('out/bytes.bin').write_bytes(bytes([0,255,128]))
if mode=='exit':
    for i in range(60): print('line-'+str(i))
    sys.exit(7)
if mode=='log': os.write(1,b'x'*(1048576+1))
if mode=='stderr': os.write(2,b'e'*(1048576+1))
if mode=='count':
    for i in range(9): p('out/'+str(i)+'.txt').write_text('x')
if mode=='size': p('out/big.bin').write_bytes(b'x'*(1048576+1))
if mode=='link': os.symlink('../script.py','out/link')
if mode=='out-link':
    p('out').rmdir(); os.symlink('tmp','out')
if mode=='timeout':
    signal.signal(signal.SIGTERM,signal.SIG_IGN)
    child=subprocess.Popen(['/usr/bin/python3','-S','-c','import time,signal; signal.signal(signal.SIGTERM,signal.SIG_IGN); time.sleep(30)'])
    p('tmp/child.pid').write_text(str(child.pid))
    time.sleep(30)
"#;

#[test]
fn offline_run_has_only_allowlisted_environment_literal_argv_and_readonly_inputs() {
    let mut f = Fixture::new();
    f.prepare();
    fs::create_dir(f.library.join("data")).unwrap();
    fs::write(f.library.join("data/input.csv"), "input").unwrap();
    f.execution.inputs = vec!["data".into()];
    f.execution.request.inputs = vec!["data".into()];
    f.execution.request.args = vec![
        "empty".into(),
        "$(touch injected); spaces".into(),
        "--flag=value".into(),
    ];
    let (result, files) = f.run().unwrap();
    assert_eq!(result.exit_code, Some(0));
    assert!(files.is_empty());
    let dir = Path::new(&result.run_dir);
    let record: Value =
        serde_json::from_slice(&fs::read(dir.join("tmp/record.json")).unwrap()).unwrap();
    assert_eq!(record["cwd"], json!(dir.canonicalize().unwrap()));
    assert_eq!(record["args"], json!(f.execution.request.args));
    assert_eq!(record["readonly"], true);
    let env = record["env"].as_object().unwrap();
    // The fake `uv` runs under `/usr/bin/python3`, which on macOS is the
    // Xcode shim: it injects these keys after the runner's cleared env, so
    // they are not something the runner passed.
    let shim: &[&str] = if cfg!(target_os = "macos") {
        &[
            "CPATH",
            "LC_CTYPE",
            "LIBRARY_PATH",
            "MANPATH",
            "SDKROOT",
            "__CF_USER_TEXT_ENCODING",
        ]
    } else {
        &[]
    };
    let mut keys: Vec<_> = env
        .keys()
        .map(String::as_str)
        .filter(|key| !shim.contains(key))
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        vec![
            "HOME",
            "LANG",
            "PATH",
            "TMPDIR",
            "TZ",
            "UV_CACHE_DIR",
            "UV_PYTHON_DOWNLOADS"
        ]
    );
    assert_eq!(env["HOME"], json!(dir.join("home")));
    assert_eq!(env["TMPDIR"], json!(dir.join("tmp")));
    assert_eq!(env["UV_CACHE_DIR"], json!(f.state.join("uv-cache")));
    assert_eq!(env["UV_PYTHON_DOWNLOADS"], "never");
    assert_eq!(
        fs::read_to_string(dir.join("in/0/data/input.csv")).unwrap(),
        "input"
    );
    assert!(!dir.join("injected").exists());
}
#[test]
fn digest_and_all_input_escapes_refuse_without_creating_runs() {
    let mut f = Fixture::new();
    f.prepare();
    f.execution.script.sha256 = "0".repeat(64);
    assert_eq!(f.run().unwrap_err().code(), "script_digest_mismatch");
    assert!(f.runs().is_empty());
    f.execution.script.sha256 = f.execution.request.sha256.clone();
    let outside = f._root.path().join("outside");
    fs::write(&outside, "private").unwrap();
    symlink(&outside, f.library.join("escape")).unwrap();
    fs::create_dir(f.library.join("nested")).unwrap();
    symlink(&outside, f.library.join("nested/escape")).unwrap();
    for input in ["../outside", "escape", "nested", outside.to_str().unwrap()] {
        f.execution.inputs = vec![input.into()];
        assert_eq!(
            f.run().unwrap_err().code(),
            "input_outside_library",
            "{input}"
        );
        assert!(f.runs().is_empty());
    }
}
#[test]
fn capped_stdout_and_stderr_are_retained_and_fail_naming_the_log_bound() {
    for mode in ["log", "stderr"] {
        let mut f = Fixture::new();
        f.prepare();
        f.execution.request.args = vec![mode.into()];
        let error = f.run().unwrap_err();
        assert_eq!(error.code(), "script_bound_exceeded");
        assert!(error.to_string().contains("log_bytes"));
        assert_eq!(
            fs::metadata(f.runs()[0].join("run.log")).unwrap().len(),
            1_048_576
        );
    }
}
#[test]
fn output_count_size_and_links_are_refused_before_filing() {
    for (mode, bound) in [
        ("count", "output_files"),
        ("size", "output_file_bytes"),
        ("link", "output link"),
        ("out-link", "output link"),
    ] {
        let mut f = Fixture::new();
        f.prepare();
        f.execution.request.args = vec![mode.into()];
        let error = f.run().unwrap_err();
        assert!(error.to_string().contains(bound), "{error}");
    }
}
#[test]
fn failed_exit_preserves_actual_code_and_exact_last_fifty_lines() {
    let mut f = Fixture::new();
    f.prepare();
    f.execution.request.args = vec!["exit".into()];
    let error = f.run().unwrap_err();
    let office_core::Error::Script(ScriptError::Exit {
        exit_code,
        log_tail,
    }) = error
    else {
        panic!("unexpected {error}");
    };
    assert_eq!(exit_code, Some(7));
    assert_eq!(log_tail.lines().count(), 50);
    assert_eq!(log_tail.lines().next(), Some("line-10"));
    assert_eq!(log_tail.lines().last(), Some("line-59"));
}
#[test]
fn timeout_kills_descendants_holding_log_pipes_and_names_the_bound() {
    let mut f = Fixture::new();
    f.prepare();
    f.execution.request.args = vec!["timeout".into()];
    let bounds = ScriptBounds {
        timeout_seconds: 3,
        ..Default::default()
    };
    let start = std::time::Instant::now();
    let error = f.store.run(&f.execution, &bounds).unwrap_err();
    assert_eq!(error.code(), "script_bound_exceeded");
    assert!(error.to_string().contains("timeout_seconds"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(bounds.timeout_seconds + 3));
    let pid = fs::read_to_string(f.runs()[0].join("tmp/child.pid")).unwrap();
    // On Linux an orphan can remain a zombie until init reaps it; it is no longer executing.
    #[cfg(target_os = "linux")]
    if let Ok(status) = fs::read_to_string(format!("/proc/{pid}/stat")) {
        assert_eq!(
            status.split(')').nth(1).unwrap().split_whitespace().next(),
            Some("Z")
        );
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
    } // Bounded pipe EOF above proves inherited descriptors closed on macOS too.
}
#[test]
fn outputs_are_staged_as_binary_files_and_promotion_manifest_with_provenance() {
    let mut f = Fixture::new();
    f.prepare();
    f.execution.request.args = vec!["outputs".into()];
    fs::write(f.library.join("private.csv"), "private input").unwrap();
    f.execution.inputs = vec!["private.csv".into()];
    f.execution.request.inputs = vec!["private.csv".into()];
    let (result, files) = f.run().unwrap();
    assert_eq!(result.files, vec!["bytes.bin", "report.csv"]);
    assert_eq!(files.len(), 4);
    let binary = files.iter().find(|f| f.path == "bytes.bin").unwrap();
    assert_eq!(fs::read(&binary.source_path).unwrap(), vec![0, 255, 128]);
    let manifest = files
        .iter()
        .find(|f| f.path == "artifact-manifest.json")
        .unwrap();
    let manifest: office_core::domain::artifact::ArtifactManifest =
        serde_json::from_slice(&fs::read(&manifest.source_path).unwrap()).unwrap();
    manifest.validate().unwrap();
    assert_eq!(manifest.sensitivity, office_common::Sensitivity::Private);
    assert_eq!(manifest.files, result.files);
    let provenance = files.iter().find(|f| f.path == "script-run.json").unwrap();
    let provenance: Value =
        serde_json::from_slice(&fs::read(&provenance.source_path).unwrap()).unwrap();
    assert_eq!(provenance["sha256"], f.execution.request.sha256);
    assert_eq!(provenance["run_id"], json!(f.execution.run_id));
}
#[test]
fn preparation_fetches_and_pins_dependencies_without_running_the_script() {
    let mut f = Fixture::new();
    f.prepare();
    assert!(f.state.join("uv-cache/downloaded").exists());
    assert!(f.runs().is_empty());
    // A repeated acceptance reuses the immutable prepared snapshot.
    f.prepare();
    fs::write(f.library.join("script.py"), "bad-deps").unwrap();
    let hash = office_common::fs::hash::sha256_hex(b"bad-deps");
    f.execution.script.sha256 = hash;
    assert!(matches!(
        f.store.prepare(&f.execution.script),
        Err(office_core::Error::Script(ScriptError::Prepare {
            exit_code: Some(9)
        }))
    ));
}
#[test]
fn pruning_removes_only_owned_old_directories_and_preserves_other_entries() {
    let f = Fixture::new();
    f.prepare();
    let runs = f.state.join("runs");
    fs::create_dir(&runs).unwrap();
    let old = SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60);
    for name in [
        "jrun-20261001-1200-abc",
        "run-script-Abcdef123456",
        "run-not-owned",
        "notes",
    ] {
        let dir = runs.join(name);
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("keep"), "x").unwrap();
        fs::File::open(dir)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
    fs::write(runs.join("jrun-20261001-1200-xyz"), "file").unwrap();
    symlink(
        f._root.path().join("outside"),
        runs.join("jrun-20261001-1200-link"),
    )
    .unwrap();
    f.run().unwrap();
    assert!(!runs.join("jrun-20261001-1200-abc").exists());
    assert!(!runs.join("run-script-Abcdef123456").exists());
    assert!(runs.join("run-not-owned/keep").exists());
    assert!(runs.join("notes/keep").exists());
    assert!(runs.join("jrun-20261001-1200-xyz").is_file());
    assert!(
        fs::symlink_metadata(runs.join("jrun-20261001-1200-link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn run_state_symlinks_cannot_redirect_pruning_or_cache_access() {
    let f = Fixture::new();
    f.prepare();
    let outside = f._root.path().join("unrelated");
    fs::create_dir(&outside).unwrap();
    let old = outside.join("jrun-20261001-1200-abc");
    fs::create_dir(&old).unwrap();
    fs::write(old.join("keep"), "unrelated").unwrap();
    fs::File::open(&old)
        .unwrap()
        .set_times(
            fs::FileTimes::new()
                .set_modified(SystemTime::now() - Duration::from_secs(8 * 24 * 60 * 60)),
        )
        .unwrap();
    symlink(&outside, f.state.join("runs")).unwrap();
    assert!(
        f.run().is_err(),
        "an internal state symlink must refuse execution"
    );
    assert!(
        old.join("keep").exists(),
        "pruning must not follow a state symlink"
    );
}

#[test]
fn dependency_cache_symlink_is_refused_without_following_it() {
    let f = Fixture::new();
    f.prepare();
    let outside = f._root.path().join("moved-cache");
    fs::rename(f.state.join("uv-cache"), &outside).unwrap();
    symlink(&outside, f.state.join("uv-cache")).unwrap();
    assert!(f.store.prepare(&f.execution.script).is_err());
    assert!(f.run().is_err());
    assert!(f.runs().is_empty());
}
