// SPDX-License-Identifier: AGPL-3.0-or-later
//! The transcoder's settings (docs/specs/m1/08-uploaded-video.md B8, B9, API -> Configuration
//! added): `max_video_seconds`, `transcode_concurrency`, `transcode_threads`, `transcode_dir`,
//! `ffmpeg_path` and `ffprobe_path`.
//!
//! Loading reads no file system: `transcode_dir` and the two program paths must be absolute
//! here, and `stepclimb serve` checks that the directory exists and that both programs run
//! when its role serves the `transcode` queue (`crate::startup`).

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use super::{ConfigError, Origin, Setting, Sources};

pub const MAX_VIDEO_SECONDS: Setting = Setting {
    var: "STEPCLIMB_MAX_VIDEO_SECONDS",
    key: "max_video_seconds",
    secret: false,
    example: "14400 (the longest video a transcode accepts, in seconds: 1 to 86400)",
};
pub const TRANSCODE_CONCURRENCY: Setting = Setting {
    var: "STEPCLIMB_TRANSCODE_CONCURRENCY",
    key: "transcode_concurrency",
    secret: false,
    example: "1 (transcodes one process runs at once, 1 to 16)",
};
pub const TRANSCODE_THREADS: Setting = Setting {
    var: "STEPCLIMB_TRANSCODE_THREADS",
    key: "transcode_threads",
    secret: false,
    example: "0 (ffmpeg's threads per transcode, 0 to 64; 0 leaves the choice to ffmpeg)",
};
pub const TRANSCODE_DIR: Setting = Setting {
    var: "STEPCLIMB_TRANSCODE_DIR",
    key: "transcode_dir",
    secret: false,
    example: "/var/tmp/stepclimb (an absolute path to an existing directory)",
};
pub const FFMPEG_PATH: Setting = Setting {
    var: "STEPCLIMB_FFMPEG_PATH",
    key: "ffmpeg_path",
    secret: false,
    example: "/usr/local/bin/ffmpeg (an absolute path; unset, ffmpeg is found on PATH)",
};
pub const FFPROBE_PATH: Setting = Setting {
    var: "STEPCLIMB_FFPROBE_PATH",
    key: "ffprobe_path",
    secret: false,
    example: "/usr/local/bin/ffprobe (an absolute path; unset, ffprobe is found on PATH)",
};

/// `max_video_seconds` when unset: 4 hours (B9).
pub const DEFAULT_MAX_VIDEO_SECONDS: u32 = 4 * 60 * 60;

/// The largest `max_video_seconds`: 24 hours.
pub const MAX_MAX_VIDEO_SECONDS: u32 = 24 * 60 * 60;

/// The largest `transcode_concurrency` (B8).
pub const MAX_TRANSCODE_CONCURRENCY: u32 = 16;

/// The largest `transcode_threads`.
pub const MAX_TRANSCODE_THREADS: u32 = 64;

/// The transcoder's settings, validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscodeConfig {
    /// The longest video a transcode accepts, in seconds.
    pub max_video_seconds: NonZeroU32,
    /// The most transcodes one process runs at once.
    pub concurrency: NonZeroU32,
    /// ffmpeg's `-threads`; 0 leaves the choice to ffmpeg.
    pub threads: u32,
    /// Where each transcode's work directory is made; the OS temporary directory when unset.
    pub dir: PathBuf,
    /// ffmpeg's path, or `None` to find it on `PATH`.
    pub ffmpeg: Option<PathBuf>,
    /// ffprobe's path, or `None` to find it on `PATH`.
    pub ffprobe: Option<PathBuf>,
}

impl TranscodeConfig {
    /// Each setting's key and value as `check-config` prints them.
    pub fn report(&self) -> Vec<(&'static str, String)> {
        let path = |p: &Option<PathBuf>| match p {
            Some(path) => path.display().to_string(),
            None => "(PATH)".to_owned(),
        };
        vec![
            (MAX_VIDEO_SECONDS.key, self.max_video_seconds.to_string()),
            (TRANSCODE_CONCURRENCY.key, self.concurrency.to_string()),
            (TRANSCODE_THREADS.key, self.threads.to_string()),
            (TRANSCODE_DIR.key, self.dir.display().to_string()),
            (FFMPEG_PATH.key, path(&self.ffmpeg)),
            (FFPROBE_PATH.key, path(&self.ffprobe)),
        ]
    }
}

/// Read and check the six settings.
pub(super) fn load(sources: &Sources<'_>) -> Result<TranscodeConfig, ConfigError> {
    let max_video_seconds = bounded(sources, &MAX_VIDEO_SECONDS, 1, MAX_MAX_VIDEO_SECONDS)?
        .unwrap_or(DEFAULT_MAX_VIDEO_SECONDS);
    let concurrency = bounded(
        sources,
        &TRANSCODE_CONCURRENCY,
        1,
        MAX_TRANSCODE_CONCURRENCY,
    )?
    .unwrap_or(1);
    let threads = bounded(sources, &TRANSCODE_THREADS, 0, MAX_TRANSCODE_THREADS)?.unwrap_or(0);
    let dir = absolute(sources, &TRANSCODE_DIR)?.unwrap_or_else(std::env::temp_dir);
    Ok(TranscodeConfig {
        // Both are at least 1: their lower bound above.
        max_video_seconds: NonZeroU32::new(max_video_seconds).unwrap_or(NonZeroU32::MIN),
        concurrency: NonZeroU32::new(concurrency).unwrap_or(NonZeroU32::MIN),
        threads,
        dir,
        ffmpeg: absolute(sources, &FFMPEG_PATH)?,
        ffprobe: absolute(sources, &FFPROBE_PATH)?,
    })
}

/// A whole number of digits from `min` to `max`, or `None` when unset.
fn bounded(
    sources: &Sources<'_>,
    setting: &Setting,
    min: u32,
    max: u32,
) -> Result<Option<u32>, ConfigError> {
    let Some((value, origin)) = sources.plain(setting)? else {
        return Ok(None);
    };
    let digits = value.bytes().all(|b| b.is_ascii_digit());
    match value.parse::<u32>() {
        Ok(n) if digits && (min..=max).contains(&n) => Ok(Some(n)),
        _ => Err(invalid(
            origin,
            setting,
            &format!("{value:?} is not a whole number from {min} to {max}"),
        )),
    }
}

/// An absolute path, or `None` when unset.
fn absolute(sources: &Sources<'_>, setting: &Setting) -> Result<Option<PathBuf>, ConfigError> {
    let Some((value, origin)) = sources.plain(setting)? else {
        return Ok(None);
    };
    if Path::new(&value).is_absolute() {
        Ok(Some(PathBuf::from(value)))
    } else {
        Err(invalid(
            origin,
            setting,
            &format!("{value:?} is not an absolute path"),
        ))
    }
}

fn invalid(origin: Origin, setting: &Setting, problem: &str) -> ConfigError {
    ConfigError::Invalid {
        origin,
        problem: format!("{problem}; for example {}", setting.example),
    }
}
