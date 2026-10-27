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

/// Supervised local video inspection/extraction; no credentials and no inference.
#[derive(Clone, Debug)]
pub struct VideoProcessor {
    /// ffprobe executable (injectable for fixture tests).
    pub ffprobe: PathBuf,
    /// ffmpeg executable (injectable for fixture tests).
    pub ffmpeg: PathBuf,
}
impl Default for VideoProcessor {
    fn default() -> Self {
        Self {
            ffprobe: "ffprobe".into(),
            ffmpeg: "ffmpeg".into(),
        }
    }
}
impl VideoProcessor {
    /// Decode at most ten minutes from a media container into mono 16-kHz WAV.
    pub fn audio(
        &self,
        bytes: &[u8],
        start_seconds: u64,
        length_seconds: u64,
    ) -> office_core::Result<Vec<u8>> {
        if length_seconds == 0 || length_seconds > 600 || start_seconds > 604_800 {
            return Err(video_error(
                "audio window must be positive, at most ten minutes and within seven days",
            ));
        }
        let dir = Self::stage(bytes)?;
        let audio = self.run_bounded(
            &self.ffmpeg,
            &[
                "-nostdin".into(),
                "-v".into(),
                "error".into(),
                "-ss".into(),
                start_seconds.to_string(),
                "-i".into(),
                "input".into(),
                "-t".into(),
                length_seconds.to_string(),
                "-vn".into(),
                "-ac".into(),
                "1".into(),
                "-ar".into(),
                "16000".into(),
                "-c:a".into(),
                "pcm_s16le".into(),
                "-f".into(),
                "wav".into(),
                "pipe:1".into(),
            ],
            dir.path(),
            20 * 1024 * 1024,
        )?;
        if !audio.starts_with(b"RIFF") || audio.get(8..12) != Some(b"WAVE") {
            return Err(video_error("ffmpeg returned no WAV audio"));
        }
        Ok(audio)
    }
    fn run(&self, program: &Path, args: &[String], dir: &Path) -> office_core::Result<Vec<u8>> {
        self.run_bounded(program, args, dir, 8 * 1024 * 1024)
    }
    fn run_bounded(
        &self,
        program: &Path,
        args: &[String],
        dir: &Path,
        limit: u64,
    ) -> office_core::Result<Vec<u8>> {
        let mut command = Command::new(program);
        command.env_clear();
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        command
            .current_dir(dir)
            .env("HOME", dir)
            .env("TMPDIR", dir)
            .args(args);
        let output =
            office_common::process::run_captured_with_stdout_limit(&mut command, dir, limit)
                .map_err(|error| video_error(&format!("media process failed: {}", error.code())))?;
        if !output.status.success() {
            return Err(video_error("media process exited unsuccessfully"));
        }
        Ok(output.stdout)
    }
    fn stage(bytes: &[u8]) -> office_core::Result<tempfile::TempDir> {
        let dir =
            tempfile::tempdir().map_err(|_| video_error("cannot create private media staging"))?;
        office_common::fs::atomic::write_atomic(&dir.path().join("input"), bytes)
            .map_err(|_| video_error("cannot stage video"))?;
        Ok(dir)
    }
    /// Probe duration and audio streams, then decode scene thumbnails and metadata.
    pub fn analyze(
        &self,
        bytes: &[u8],
        threshold: &serde_json::Number,
    ) -> office_core::Result<office_core::domain::source::video::VideoAnalysis> {
        use office_core::domain::source::video::{VideoAnalysis, VideoFrame};
        let dir = Self::stage(bytes)?;
        let output = self.run(
            &self.ffprobe,
            &[
                "-v".into(),
                "error".into(),
                "-show_entries".into(),
                "format=duration:stream=codec_type".into(),
                "-of".into(),
                "json".into(),
                "input".into(),
            ],
            dir.path(),
        )?;
        let probe: serde_json::Value = serde_json::from_slice(&output)
            .map_err(|_| video_error("ffprobe returned invalid metadata"))?;
        let duration = probe["format"]["duration"]
            .as_str()
            .ok_or_else(|| video_error("ffprobe duration missing"))?;
        let duration_ms = decimal_scaled(duration, 1000, true)?;
        if duration_ms == 0 || duration_ms > 604_800_000 {
            return Err(video_error(
                "video duration must be positive and at most seven days",
            ));
        }
        let streams = probe["streams"]
            .as_array()
            .ok_or_else(|| video_error("ffprobe streams missing"))?;
        let has_audio = streams.iter().any(|stream| stream["codec_type"] == "audio");
        let has_video = streams.iter().any(|stream| stream["codec_type"] == "video");
        if !has_video {
            return Ok(VideoAnalysis {
                duration_ms,
                has_audio,
                candidates: Vec::new(),
            });
        }
        // prev_selected_t is local to this select filter. Spacing bounds both
        // decoder output and metadata volume before the pure selection repeats it.
        let filter = format!(
            "select='gt(scene,{threshold})*(isnan(prev_selected_t)+gte(t-prev_selected_t,10))',metadata=print:file=scenes.txt,scale=9:8,format=gray"
        );
        let raw = self.run(
            &self.ffmpeg,
            &[
                "-nostdin".into(),
                "-v".into(),
                "error".into(),
                "-i".into(),
                "input".into(),
                "-an".into(),
                "-vf".into(),
                filter,
                "-fps_mode".into(),
                "vfr".into(),
                "-f".into(),
                "rawvideo".into(),
                "-pix_fmt".into(),
                "gray".into(),
                "pipe:1".into(),
            ],
            dir.path(),
        )?;
        use std::io::Read as _;
        let mut metadata = String::new();
        std::fs::File::open(dir.path().join("scenes.txt"))
            .and_then(|file| {
                file.take(16 * 1024 * 1024 + 1)
                    .read_to_string(&mut metadata)
            })
            .map_err(|_| video_error("cannot read scene metadata"))?;
        if metadata.len() > 16 * 1024 * 1024 {
            return Err(video_error("scene metadata exceeds its bound"));
        }
        let mut scenes = Vec::new();
        let mut timestamp = None;
        for line in metadata.lines() {
            if let Some(raw) = line.split("pts_time:").nth(1) {
                if timestamp.is_some() {
                    return Err(video_error("scene score missing"));
                }
                timestamp = Some(decimal_scaled(
                    raw.split_whitespace().next().unwrap_or(""),
                    1000,
                    false,
                )?);
            } else if let Some(raw) = line.strip_prefix("lavfi.scene_score=") {
                let time_ms = timestamp
                    .take()
                    .ok_or_else(|| video_error("scene timestamp missing"))?;
                let score = u32::try_from(decimal_scaled(raw, 1_000_000, false)?)
                    .map_err(|_| video_error("scene score overflow"))?;
                if score > 1_000_000 {
                    return Err(video_error("scene score out of range"));
                }
                scenes.push((time_ms, score));
            }
        }
        if timestamp.is_some() || raw.len() != scenes.len() * 72 {
            return Err(video_error("scene thumbnails and metadata disagree"));
        }
        let candidates = scenes
            .into_iter()
            .zip(raw.chunks_exact(72))
            .map(|((time_ms, score), thumbnail)| {
                let mut dhash = 0_u64;
                for row in thumbnail.chunks_exact(9) {
                    for pair in row.windows(2) {
                        dhash = (dhash << 1) | u64::from(pair[0] > pair[1]);
                    }
                }
                VideoFrame {
                    time_ms,
                    score,
                    dhash,
                }
            })
            .collect();
        Ok(VideoAnalysis {
            duration_ms,
            has_audio,
            candidates,
        })
    }
    /// Decode a single PNG, width 640, preserving aspect ratio.
    pub fn frame(&self, bytes: &[u8], time_ms: u64) -> office_core::Result<Vec<u8>> {
        let dir = Self::stage(bytes)?;
        let image = self.run(
            &self.ffmpeg,
            &[
                "-nostdin".into(),
                "-v".into(),
                "error".into(),
                "-ss".into(),
                format!("{}.{:03}", time_ms / 1000, time_ms % 1000),
                "-i".into(),
                "input".into(),
                "-frames:v".into(),
                "1".into(),
                "-vf".into(),
                "scale=640:-1".into(),
                "-f".into(),
                "image2pipe".into(),
                "-vcodec".into(),
                "png".into(),
                "pipe:1".into(),
            ],
            dir.path(),
        )?;
        if !image.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(video_error("ffmpeg returned no PNG frame"));
        }
        Ok(image)
    }
}
fn video_error(message: &str) -> Error {
    Error::Model {
        code: "media_processing_failed",
        message: message.into(),
        retryable: false,
    }
}
/// Decimal conversion without floating point, overflow or narrowing casts.
fn decimal_scaled(raw: &str, scale: u64, ceil: bool) -> office_core::Result<u64> {
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || !fraction.bytes().all(|c| c.is_ascii_digit())
        || fraction.len() > 12
    {
        return Err(video_error("invalid decimal media metadata"));
    }
    let whole = whole
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(scale))
        .ok_or_else(|| video_error("media metadata overflow"))?;
    let denominator =
        10_u64.pow(u32::try_from(fraction.len()).map_err(|_| video_error("decimal overflow"))?);
    let numerator = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<u64>()
            .map_err(|_| video_error("invalid decimal"))?
    };
    let scaled = numerator
        .checked_mul(scale)
        .ok_or_else(|| video_error("media metadata overflow"))?;
    whole
        .checked_add(if ceil {
            scaled.div_ceil(denominator)
        } else {
            scaled / denominator
        })
        .ok_or_else(|| video_error("media metadata overflow"))
}
