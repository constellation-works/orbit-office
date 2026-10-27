//! Plugin-state script runner. Approved bytes run with plugin grants, not a sandbox.

use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime};

use office_common::fs::{atomic::write_atomic, hash::sha256_hex};
use office_core::Result;
use office_core::domain::script::{
    ScriptBounds, ScriptError, ScriptExecution, ScriptFile, ScriptPreparation, ScriptResult,
};
use office_core::ports::ScriptRunner;
use serde_json::json;

// Must track install.sh's PYTHON_VERSION; PATH includes the install's python3.
const PYTHON: &str = "3.12";
const WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Filesystem and process adapter; constructing it performs no I/O.
pub struct ScriptStore {
    library: PathBuf,
    state: PathBuf,
    staging: PathBuf,
    environment: Vec<(OsString, OsString)>,
}
impl ScriptStore {
    /// Roots and ambient allowlist supplied by composition, never by tool input.
    #[must_use]
    pub fn new(
        library: PathBuf,
        state: PathBuf,
        staging: PathBuf,
        environment: Vec<(OsString, OsString)>,
    ) -> Self {
        Self {
            library,
            state,
            staging,
            environment,
        }
    }

    fn script_bytes(&self, script: &ScriptPreparation) -> Result<Vec<u8>> {
        let root = fs::canonicalize(&self.library).map_err(|_| adapter("resolve Library"))?;
        let path = confined(&root, &script.path)?;
        let bytes = read_bounded(&path, 1_048_576)?;
        if sha256_hex(&bytes) != script.sha256 {
            return Err(ScriptError::DigestMismatch.into());
        }
        Ok(bytes)
    }

    fn command(&self, cwd: &Path) -> Command {
        let mut command = Command::new("uv");
        command.env_clear().current_dir(cwd);
        for (key, value) in &self.environment {
            if ["PATH", "LANG", "TZ"].iter().any(|allowed| key == allowed) {
                command.env(key, value);
            }
        }
        command
            .env("HOME", cwd.join("home"))
            .env("TMPDIR", cwd.join("tmp"))
            .env("UV_CACHE_DIR", self.state.join("uv-cache"))
            .env("UV_PYTHON_DOWNLOADS", "never");
        command
    }

    fn prepared(&self, digest: &str) -> Result<PathBuf> {
        checked_path(&self.state, &Path::new("uv-cache/prepared").join(digest))
    }

    fn prune(&self) -> Result<()> {
        let runs = checked_path(&self.state, Path::new("runs"))?;
        let entries = match fs::read_dir(&runs) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(adapter("list old runs")),
        };
        for entry in entries {
            let entry = entry.map_err(|_| adapter("read old run"))?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !run_name(name) {
                continue;
            }
            let meta =
                fs::symlink_metadata(entry.path()).map_err(|_| adapter("inspect old run"))?;
            if !meta.is_dir() || meta.file_type().is_symlink() {
                continue;
            }
            let modified = meta.modified().map_err(|_| adapter("date old run"))?;
            if SystemTime::now()
                .duration_since(modified)
                .is_ok_and(|age| age > WEEK)
            {
                make_writable(&entry.path())?;
                fs::remove_dir_all(entry.path()).map_err(|_| adapter("prune owned run"))?;
            }
        }
        Ok(())
    }
}

impl ScriptRunner for ScriptStore {
    fn prepare(&self, script: &ScriptPreparation) -> Result<()> {
        let bytes = self.script_bytes(script)?;
        let prepared = self.prepared(&script.sha256)?;
        if prepared.join("script.py.lock").is_file() {
            return Ok(());
        }
        let parent = checked_path(&self.state, Path::new("uv-cache/prepared"))?;
        private_dir(&parent)?;
        let temp = tempfile::Builder::new()
            .prefix("prepare-")
            .tempdir_in(&parent)
            .map_err(|_| adapter("create preparation"))?;
        private_dir(&temp.path().join("home"))?;
        private_dir(&temp.path().join("tmp"))?;
        write_atomic(&temp.path().join("script.py"), &bytes)
            .map_err(|_| adapter("snapshot approved script"))?;
        for verb in ["lock", "sync"] {
            let mut command = self.command(temp.path());
            command.args([
                verb,
                "--no-config",
                "--python",
                PYTHON,
                "--script",
                "script.py",
            ]);
            if verb == "sync" {
                command.arg("--locked");
            }
            let output = office_common::process::run_logged(
                &mut command,
                temp.path(),
                Duration::from_secs(300),
                1_048_576,
            )
            .map_err(|_| adapter("prepare dependencies"))?;
            if output.timed_out || output.truncated || !output.status.is_some_and(|s| s.success()) {
                return Err(ScriptError::Prepare {
                    exit_code: output.status.and_then(|s| s.code()),
                }
                .into());
            }
        }
        if !temp.path().join("script.py.lock").is_file() {
            return Err(adapter("verify dependency lock"));
        }
        match fs::rename(temp.path(), &prepared) {
            Ok(()) => {}
            Err(_) if prepared.join("script.py.lock").is_file() => {}
            Err(_) => return Err(adapter("publish prepared dependencies")),
        }
        Ok(())
    }

    fn run(
        &self,
        execution: &ScriptExecution,
        bounds: &ScriptBounds,
    ) -> Result<(ScriptResult, Vec<ScriptFile>)> {
        bounds.validate()?;
        execution.request.validate()?;
        let script = self.script_bytes(&execution.script)?;
        let root = fs::canonicalize(&self.library).map_err(|_| adapter("resolve Library"))?;
        // Walk all descendants before creating state, so nested escapes also refuse cleanly.
        let mut inputs = Vec::new();
        for (index, input) in execution.inputs.iter().enumerate() {
            let path = confined(&root, input)?;
            let name = path.file_name().ok_or_else(|| outside(input))?;
            collect_inputs(
                &root,
                &path,
                &PathBuf::from(index.to_string()).join(name),
                &mut inputs,
            )?;
        }
        let lock = read_bounded(
            &self
                .prepared(&execution.script.sha256)?
                .join("script.py.lock"),
            1_048_576,
        )?;
        self.prune()?;
        private_dir(&self.state.join("runs"))?;
        // Always an exclusively allocated suffix: repeated/overlapping routine ids never collide.
        let prefix = execution
            .run_id
            .as_deref()
            .filter(|id| run_name(id))
            .unwrap_or("run-script");
        let temp = tempfile::Builder::new()
            .prefix(&format!("{prefix}-"))
            .rand_bytes(12)
            .tempdir_in(self.state.join("runs"))
            .map_err(|_| adapter("create run"))?;
        let dir = temp.keep(); // retained on every exit for seven days
        for name in ["in", "out", "tmp", "home"] {
            private_dir(&dir.join(name))?;
        }
        write_atomic(&dir.join("script.py"), &script)
            .map_err(|_| adapter("write script snapshot"))?;
        write_atomic(&dir.join("script.py.lock"), &lock)
            .map_err(|_| adapter("write dependency lock"))?;
        for (source, relative) in inputs {
            let destination = dir.join("in").join(relative);
            if let Some(parent) = destination.parent() {
                private_dir(parent)?;
            }
            if source.is_dir() {
                private_dir(&destination)?;
                continue;
            }
            let bytes = fs::read(&source).map_err(|_| adapter("read Library input"))?;
            write_atomic(&destination, &bytes).map_err(|_| adapter("copy Library input"))?;
        }
        readonly(&dir.join("in"))?;
        write_atomic(&dir.join("run.log"), &[]).map_err(|_| adapter("create run log"))?;
        let start = Instant::now();
        let mut command = self.command(&dir);
        command
            .args([
                "run",
                "--offline",
                "--no-config",
                "--no-project",
                "--locked",
                "--python",
                PYTHON,
                "--script",
                "script.py",
            ])
            .args(&execution.request.args);
        let capture = office_common::process::run_logged(
            &mut command,
            &dir,
            Duration::from_secs(bounds.timeout_seconds),
            usize::try_from(bounds.log_bytes).map_err(|_| adapter("convert log bound"))?,
        )
        .map_err(|_| adapter("execute uv"))?;
        write_atomic(&dir.join("run.log"), &capture.bytes)
            .map_err(|_| adapter("retain run log"))?;
        if capture.timed_out {
            return Err(ScriptError::Bound {
                bound: "timeout_seconds",
                limit: bounds.timeout_seconds,
            }
            .into());
        }
        if capture.truncated {
            return Err(ScriptError::Bound {
                bound: "log_bytes",
                limit: bounds.log_bytes,
            }
            .into());
        }
        let tail = String::from_utf8_lossy(&capture.bytes)
            .lines()
            .rev()
            .take(50)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        let exit_code = capture.status.and_then(|s| s.code());
        if exit_code != Some(0) {
            return Err(ScriptError::Exit {
                exit_code,
                log_tail: tail,
            }
            .into());
        }
        let mut files = Vec::new();
        outputs(&dir.join("out"), &dir.join("out"), bounds, &mut files)?;
        files.sort();
        if files.iter().any(|name| {
            ["artifact-manifest.json", "script-run.json"].contains(&name.as_str())
                || name.to_ascii_lowercase().starts_with("review-")
        }) {
            return Err(adapter("refuse reserved output artifact path"));
        }
        let mut result = ScriptResult {
            exit_code,
            duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            log_tail: tail,
            run_dir: dir.to_string_lossy().into(),
            output_task: None,
            files: files.clone(),
        };
        let mut attached = Vec::new();
        if !files.is_empty() {
            // The callback confines sources to the workspace; composition supplies its granted attachment staging root.
            if let Ok(relative) = self.staging.strip_prefix(&self.library) {
                checked_path(&self.library, relative)?;
            }
            private_dir(&self.staging)?;
            // Stage under the physical path: the Library port accepts a staged
            // file only under its canonical root, and an aliased ancestor
            // (macOS `/var` -> `/private/var`) would otherwise fail that check.
            let staging =
                fs::canonicalize(&self.staging).map_err(|_| adapter("resolve output staging"))?;
            let stage_guard = tempfile::Builder::new()
                .prefix("script-output-")
                .tempdir_in(&staging)
                .map_err(|_| adapter("stage output"))?;
            let stage = stage_guard.path();
            for name in &files {
                let bytes = read_bounded(&dir.join("out").join(name), bounds.output_file_bytes)?;
                let path = stage.join(name);
                if let Some(parent) = path.parent() {
                    private_dir(parent)?;
                }
                write_atomic(&path, &bytes).map_err(|_| adapter("stage output file"))?;
                attached.push(ScriptFile {
                    path: name.clone(),
                    source_path: path.to_string_lossy().into(),
                });
            }
            let manifest = execution.request.manifest(files);
            manifest.validate()?;
            let provenance = json!({"artifact":execution.request.artifact,"sha256":execution.request.sha256,"inputs":execution.request.inputs,"run_id":execution.run_id});
            for (name, value) in [
                ("script-run.json", provenance),
                ("artifact-manifest.json", json!(manifest)),
            ] {
                let path = stage.join(name);
                let bytes = serde_json::to_vec_pretty(&value)
                    .map_err(|_| adapter("encode output manifest"))?;
                if bytes.len() > 1_048_576 {
                    return Err(ScriptError::Bound {
                        bound: "manifest_bytes",
                        limit: 1_048_576,
                    }
                    .into());
                }
                write_atomic(&dir.join(name), &bytes)
                    .map_err(|_| adapter("retain output manifest"))?;
                write_atomic(&path, &bytes).map_err(|_| adapter("stage output manifest"))?;
                attached.push(ScriptFile {
                    path: name.into(),
                    source_path: path.to_string_lossy().into(),
                });
            }
            let _ = stage_guard.keep();
        }
        result.output_task = None;
        Ok((result, attached))
    }
}

fn adapter(operation: &'static str) -> office_core::Error {
    ScriptError::Adapter { operation }.into()
}
fn outside(path: &str) -> office_core::Error {
    ScriptError::InputOutsideLibrary { path: path.into() }.into()
}

fn confined(root: &Path, input: &str) -> Result<PathBuf> {
    let path = Path::new(input);
    if path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(outside(input));
    }
    let canonical = fs::canonicalize(root.join(path)).map_err(|_| outside(input))?;
    if !canonical.starts_with(root) || canonical == root {
        return Err(outside(input));
    }
    Ok(canonical)
}

fn collect_inputs(
    root: &Path,
    path: &Path,
    relative: &Path,
    files: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| outside(&path.to_string_lossy()))?;
    // Reject nested links (including cycles) and special entries; top-level links are canonicalized.
    if meta.file_type().is_symlink()
        || (!meta.is_file() && !meta.is_dir())
        || !path.starts_with(root)
    {
        return Err(outside(&path.to_string_lossy()));
    }
    if meta.is_file() {
        files.push((path.into(), relative.into()));
    } else {
        files.push((path.into(), relative.into()));
        for entry in fs::read_dir(path).map_err(|_| adapter("list Library input"))? {
            let entry = entry.map_err(|_| adapter("read Library input entry"))?;
            collect_inputs(
                root,
                &entry.path(),
                &relative.join(entry.file_name()),
                files,
            )?;
        }
    }
    Ok(())
}

fn outputs(root: &Path, path: &Path, bounds: &ScriptBounds, files: &mut Vec<String>) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|_| adapter("inspect output directory"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(adapter("refuse output link or special file"));
    }
    for entry in fs::read_dir(path).map_err(|_| adapter("list output"))? {
        let entry = entry.map_err(|_| adapter("read output entry"))?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| adapter("inspect output"))?;
        if meta.file_type().is_symlink() || (!meta.is_file() && !meta.is_dir()) {
            return Err(adapter("refuse output link or special file"));
        }
        if meta.is_dir() {
            outputs(root, &entry.path(), bounds, files)?;
        } else {
            if meta.len() > bounds.output_file_bytes {
                return Err(ScriptError::Bound {
                    bound: "output_file_bytes",
                    limit: bounds.output_file_bytes,
                }
                .into());
            }
            files.push(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| adapter("resolve output"))?
                    .to_string_lossy()
                    .into(),
            );
            if files.len() > bounds.output_files {
                return Err(ScriptError::Bound {
                    bound: "output_files",
                    limit: u64::try_from(bounds.output_files).unwrap_or(u64::MAX),
                }
                .into());
            }
        }
    }
    Ok(())
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| adapter("inspect script state"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(adapter("refuse script state link or special file"));
    }
    let file = fs::File::open(path).map_err(|_| adapter("read script state"))?;
    if !file
        .metadata()
        .map_err(|_| adapter("inspect script state"))?
        .is_file()
    {
        return Err(adapter("refuse special file"));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| adapter("read bounded file"))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(ScriptError::Bound {
            bound: "file_bytes",
            limit,
        }
        .into());
    }
    Ok(bytes)
}

fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    let mut path = root.to_path_buf();
    for part in std::iter::once(None).chain(relative.components().map(Some)) {
        if let Some(part) = part {
            let Component::Normal(name) = part else {
                return Err(adapter("refuse nonrelative state path"));
            };
            path.push(name);
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(adapter("refuse state symlink"));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(adapter("inspect state path")),
        }
    }
    Ok(path)
}

fn private_dir(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(adapter("refuse directory symlink"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
            .map_err(|_| adapter("create private directory"))?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(path).map_err(|_| adapter("create private directory"))?;
    Ok(())
}
fn readonly(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| adapter("inspect input copy"))?;
    if meta.is_dir() {
        for entry in fs::read_dir(path).map_err(|_| adapter("list input copy"))? {
            readonly(&entry.map_err(|_| adapter("read input copy"))?.path())?;
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(if meta.is_dir() { 0o500 } else { 0o400 }),
        )
        .map_err(|_| adapter("protect input copy"))?;
    }
    Ok(())
}
fn make_writable(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| adapter("inspect owned run"))?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .map_err(|_| adapter("unlock owned run"))?;
        }
        for entry in fs::read_dir(path).map_err(|_| adapter("list owned run"))? {
            make_writable(&entry.map_err(|_| adapter("read owned entry"))?.path())?;
        }
    }
    Ok(())
}
fn run_name(name: &str) -> bool {
    let alphanumeric = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
    if let Some(suffix) = name.strip_prefix("run-script-") {
        return suffix.len() == 12 && alphanumeric(suffix);
    }
    let Some(rest) = name
        .strip_prefix("jrun-")
        .or_else(|| name.strip_prefix("run-"))
    else {
        return false;
    };
    let parts: Vec<_> = rest.split('-').collect();
    (parts.len() == 3 || parts.len() == 4)
        && parts[0].len() == 8
        && parts[0].bytes().all(|b| b.is_ascii_digit())
        && parts[1].len() == 4
        && parts[1].bytes().all(|b| b.is_ascii_digit())
        && (3..=16).contains(&parts[2].len())
        && alphanumeric(parts[2])
        && (parts.len() == 3 || (parts[3].len() == 12 && alphanumeric(parts[3])))
}
