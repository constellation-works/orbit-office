//! Audio splitting with `ffmpeg` for long recordings.
//!
//! This is the only file in `office-models` allowed a subprocess
//! (`scripts/check-dependency-direction.sh` enforces exactly that), and
//! the child runs under the bounded supervisor in `office-common`
//! (STD-03 R11–R15: own process group, deadline, capped output, group
//! swept and reaped on every exit path). `ffmpeg` never receives a
//! secret: its arguments are only the input path, offsets and output
//! paths, and the supervisor discards its stderr.
//! The child inherits only PATH for executable lookup; HOME and TMPDIR point
//! into the staging directory. Provider credentials and Office/Orbit authority
//! variables are never inherited.

use std::path::{Path, PathBuf};
use std::process::Command;

use office_core::Error;

/// Recordings longer than this many seconds are split before transcription.
pub const DEFAULT_SPLIT_THRESHOLD_SECONDS: u64 = 600;

/// One planned segment: start offset and length, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    /// Start offset into the recording.
    pub start_seconds: u64,
    /// Segment length (the last segment may be shorter).
    pub length_seconds: u64,
}

/// Whether a recording of `duration_seconds` crosses `threshold_seconds`.
#[must_use]
pub fn needs_split(duration_seconds: u64, threshold_seconds: u64) -> bool {
    duration_seconds > threshold_seconds
}

/// The segments covering `duration_seconds` in chunks of at most
/// `max_segment_seconds`. An empty recording plans no segments.
#[must_use]
pub fn segment_plan(duration_seconds: u64, max_segment_seconds: u64) -> Vec<Segment> {
    if duration_seconds == 0 || max_segment_seconds == 0 {
        return Vec::new();
    }
    let mut plan = Vec::new();
    let mut start = 0;
    while start < duration_seconds {
        let length = max_segment_seconds.min(duration_seconds - start);
        plan.push(Segment {
            start_seconds: start,
            length_seconds: length,
        });
        start += length;
    }
    plan
}

/// Splits long recordings with `ffmpeg`; short ones pass through unsplit.
#[derive(Clone, Debug)]
pub struct AudioSplitter {
    /// The `ffmpeg` executable to run.
    pub ffmpeg_path: PathBuf,
    /// Split only above this duration, in seconds.
    pub threshold_seconds: u64,
    /// Maximum length of one segment, in seconds.
    pub max_segment_seconds: u64,
}

impl Default for AudioSplitter {
    fn default() -> Self {
        Self {
            ffmpeg_path: PathBuf::from("ffmpeg"),
            threshold_seconds: DEFAULT_SPLIT_THRESHOLD_SECONDS,
            max_segment_seconds: DEFAULT_SPLIT_THRESHOLD_SECONDS,
        }
    }
}

impl AudioSplitter {
    /// Split `input` into `output_dir` when its duration crosses the
    /// configured threshold.
    ///
    /// At or below the threshold this runs no subprocess and returns the
    /// input as the single segment. Above it, one supervised `ffmpeg`
    /// call per planned segment writes `segment-<index>` files keeping
    /// the input's extension.
    ///
    /// # Errors
    ///
    /// [`Error::Model`] with code `media_split_failed` when `ffmpeg`
    /// cannot start, times out, or exits unsuccessfully.
    pub fn split_if_needed(
        &self,
        input: &Path,
        output_dir: &Path,
        duration_seconds: u64,
    ) -> Result<Vec<PathBuf>, Error> {
        if !needs_split(duration_seconds, self.threshold_seconds) {
            return Ok(vec![input.to_path_buf()]);
        }
        if self.max_segment_seconds == 0 {
            return Err(Error::Model {
                code: "media_split_failed",
                message: "audio_splitter.max_segment_seconds must be greater than zero".into(),
                retryable: false,
            });
        }
        let extension = input
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("wav");
        let mut outputs = Vec::new();
        for (index, segment) in segment_plan(duration_seconds, self.max_segment_seconds)
            .into_iter()
            .enumerate()
        {
            let output = output_dir.join(format!("segment-{index:03}.{extension}"));
            let mut command = Command::new(&self.ffmpeg_path);
            command.env_clear();
            if let Some(path) = std::env::var_os("PATH") {
                command.env("PATH", path);
            }
            command.env("HOME", output_dir).env("TMPDIR", output_dir);
            command
                .arg("-y")
                .arg("-ss")
                .arg(segment.start_seconds.to_string())
                .arg("-t")
                .arg(segment.length_seconds.to_string())
                .arg("-i")
                .arg(input)
                .arg("-c")
                .arg("copy")
                .arg(&output);
            let (status, _) =
                office_common::process::run(&mut command, output_dir).map_err(|error| {
                    Error::Model {
                        code: "media_split_failed",
                        message: format!("ffmpeg split failed: {}", error.code()),
                        retryable: false,
                    }
                })?;
            if !status.success() {
                return Err(Error::Model {
                    code: "media_split_failed",
                    message: "ffmpeg split exited unsuccessfully".to_string(),
                    retryable: false,
                });
            }
            outputs.push(output);
        }
        Ok(outputs)
    }
}
