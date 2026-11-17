// SPDX-License-Identifier: AGPL-3.0-or-later
//! The startup check of a process that serves the `transcode` queue
//! (docs/specs/m1/08-uploaded-video.md B8).
//!
//! [`prepare`] makes and removes a work directory in `transcode_dir`, then resolves ffmpeg and
//! ffprobe and runs each with `-version`. A `transcoder` process refuses to start when any step
//! fails, naming the setting to fix. So does an `all` process whose `transcode_dir`,
//! `ffmpeg_path` or `ffprobe_path` is set and fails. An `all` process that relies on `PATH` and
//! finds no ffmpeg or ffprobe there starts without serving the queue, with one warning naming
//! the setting: its uploads stay `processing` until a process that can transcode takes them.
//! Coordinator decision (overnight, 2026-11-17): the release image gets ffmpeg in I6, and until
//! then refusing to start would stop every default install.

use std::path::{Path, PathBuf};

use stepclimb_media::{
    tools::{self, Program, ToolError, Tools},
    workdir::WorkDir,
};

use crate::config::{FFMPEG_PATH, FFPROBE_PATH, Role, Setting, TRANSCODE_DIR, TranscodeConfig};
use crate::video_transcode::Transcoder;

/// Why a process that must transcode cannot.
#[derive(Debug, thiserror::Error)]
pub enum TranscodeStartupError {
    #[error(
        "{} ({}) cannot hold transcode work directories: {source}",
        TRANSCODE_DIR.key, dir.display()
    )]
    Dir {
        dir: PathBuf,
        source: std::io::Error,
    },
    #[error("{source}; set {} ({}) to the program's absolute path", setting.key, setting.var)]
    Tool {
        setting: &'static Setting,
        source: ToolError,
    },
}

fn setting_of(program: Program) -> &'static Setting {
    match program {
        Program::Ffmpeg => &FFMPEG_PATH,
        Program::Ffprobe => &FFPROBE_PATH,
    }
}

/// The transcoder `role` runs with `config`, or `None` when it does not serve the queue
/// (module documentation).
pub async fn prepare(
    role: Role,
    config: &TranscodeConfig,
) -> Result<Option<Transcoder>, TranscodeStartupError> {
    if !role.runs_transcodes() {
        return Ok(None);
    }
    let work = WorkDir::create(&config.dir).map_err(|source| TranscodeStartupError::Dir {
        dir: config.dir.clone(),
        source,
    })?;
    let Some(tools) = settle(role, check_tools(config, work.path()).await)? else {
        return Ok(None);
    };
    Ok(Some(Transcoder {
        tools,
        threads: config.threads,
        dir: config.dir.clone(),
        max_video_seconds: config.max_video_seconds.get(),
    }))
}

/// What `role` does with the result of the program check: an `all` process whose program is not
/// on `PATH` goes on without transcoding, after a warning; any other failure stops the start.
fn settle(
    role: Role,
    checked: Result<Tools, ToolError>,
) -> Result<Option<Tools>, TranscodeStartupError> {
    match checked {
        Ok(tools) => Ok(Some(tools)),
        Err(ToolError::NotFound(program)) if role == Role::All => {
            tracing::warn!(
                "{} is not on PATH: this process does not transcode videos, which stay processing \
                 until a process that can takes them; install it or set {}",
                program.name(),
                setting_of(program).var
            );
            Ok(None)
        }
        Err(source) => Err(TranscodeStartupError::Tool {
            setting: setting_of(source.program()),
            source,
        }),
    }
}

/// Both programs, resolved and checked; the first failure.
async fn check_tools(config: &TranscodeConfig, work: &Path) -> Result<Tools, ToolError> {
    let (ffmpeg, ffmpeg_version) =
        tools::check(Program::Ffmpeg, config.ffmpeg.as_deref(), work).await?;
    let (ffprobe, ffprobe_version) =
        tools::check(Program::Ffprobe, config.ffprobe.as_deref(), work).await?;
    tracing::info!(
        ffmpeg = %ffmpeg.display(),
        ffmpeg_version,
        ffprobe = %ffprobe.display(),
        ffprobe_version,
        "ffmpeg and ffprobe passed the startup check"
    );
    Ok(Tools { ffmpeg, ffprobe })
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    fn config(ffmpeg: Option<&str>, dir: &Path) -> TranscodeConfig {
        TranscodeConfig {
            max_video_seconds: NonZeroU32::new(60).unwrap(),
            concurrency: NonZeroU32::MIN,
            threads: 0,
            dir: dir.to_path_buf(),
            ffmpeg: ffmpeg.map(PathBuf::from),
            ffprobe: None,
        }
    }

    #[tokio::test]
    async fn only_the_roles_that_serve_the_queue_check_and_get_a_transcoder() {
        let dir = std::env::temp_dir();
        for role in [Role::Web, Role::Worker] {
            assert!(prepare(role, &config(None, &dir)).await.unwrap().is_none());
        }
        for role in [Role::All, Role::Transcoder] {
            let transcoder = prepare(role, &config(None, &dir)).await.unwrap().unwrap();
            assert!(transcoder.tools.ffmpeg.is_absolute());
            assert!(transcoder.tools.ffprobe.is_absolute());
        }
    }

    /// T13 (B8): a missing ffmpeg stops a transcoder at startup naming `ffmpeg_path`, and an
    /// `all` process whose setting names it.
    #[tokio::test]
    async fn a_missing_ffmpeg_stops_the_start_naming_its_setting() {
        let dir = std::env::temp_dir();
        for role in [Role::Transcoder, Role::All] {
            let error = prepare(role, &config(Some("/nonexistent/ffmpeg"), &dir))
                .await
                .unwrap_err();
            let message = error.to_string();
            assert!(message.contains("ffmpeg_path"), "{message}");
            assert!(message.contains("STEPCLIMB_FFMPEG_PATH"), "{message}");
        }
    }

    #[test]
    fn a_program_not_on_path_stops_a_transcoder_but_not_an_all_process() {
        let not_found = || Err(ToolError::NotFound(Program::Ffprobe));
        assert!(settle(Role::All, not_found()).unwrap().is_none());
        let error = settle(Role::Transcoder, not_found()).unwrap_err();
        assert!(error.to_string().contains("ffprobe_path"), "{error}");
    }

    #[tokio::test]
    async fn a_transcode_dir_that_does_not_exist_stops_the_start_naming_it() {
        let missing = std::env::temp_dir().join("stepclimb-no-such-dir/below");
        let error = prepare(Role::Transcoder, &config(None, &missing))
            .await
            .unwrap_err();
        assert!(error.to_string().starts_with("transcode_dir ("), "{error}");
    }
}
