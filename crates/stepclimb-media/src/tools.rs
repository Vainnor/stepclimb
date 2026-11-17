// SPDX-License-Identifier: AGPL-3.0-or-later
//! Finding ffmpeg and ffprobe and checking that they run (docs/specs/m1/08-uploaded-video.md B8).
//!
//! A process that serves the `transcode` queue resolves each program once at startup, from its
//! setting (`ffmpeg_path`, `ffprobe_path`) or else from `PATH`, to an absolute path, and runs it
//! with `-version`: it must exit successfully within [`VERSION_TIMEOUT`] and print a first line
//! starting with its own name and ` version`. Every later run uses the absolute path, so a
//! changed `PATH` cannot swap the program, and the child's environment can stay empty.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::run::{self, Invocation, Limits, RunError};

/// How long `-version` may take.
pub const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

/// ffmpeg and ffprobe, resolved and checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// Which of the two programs a check is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Ffmpeg,
    Ffprobe,
}

impl Program {
    /// The program's name on `PATH` and in its version line.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "ffmpeg",
            Self::Ffprobe => "ffprobe",
        }
    }
}

/// Why a program cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("{} is not on PATH", .0.name())]
    NotFound(Program),
    #[error("{} at {} did not run: {source}", program.name(), path.display())]
    DidNotRun {
        program: Program,
        path: PathBuf,
        source: RunError,
    },
    #[error("{} at {} did not report a version", program.name(), path.display())]
    NoVersion { program: Program, path: PathBuf },
}

impl ToolError {
    /// The program the error is about.
    #[must_use]
    pub fn program(&self) -> Program {
        match self {
            Self::NotFound(program)
            | Self::DidNotRun { program, .. }
            | Self::NoVersion { program, .. } => *program,
        }
    }
}

/// The first executable file named `name` in the process's `PATH`, or `None`.
#[must_use]
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// `program` at `configured`, or found on `PATH` when that is `None`, run with `-version`: its
/// absolute path and its version line.
pub async fn check(
    program: Program,
    configured: Option<&Path>,
    work: &Path,
) -> Result<(PathBuf, String), ToolError> {
    let path = match configured {
        Some(path) => path.to_path_buf(),
        None => find_on_path(program.name()).ok_or(ToolError::NotFound(program))?,
    };
    let args = [OsString::from("-hide_banner"), OsString::from("-version")];
    let invocation = Invocation {
        program: &path,
        args: &args,
        dir: work,
        watch: work,
        capture_stdout: true,
    };
    let limits = Limits {
        deadline: tokio::time::Instant::now() + VERSION_TIMEOUT,
        max_output_bytes: 0,
        max_memory_bytes: u64::MAX,
    };
    let ended = run::run(invocation, limits)
        .await
        .map_err(|source| ToolError::DidNotRun {
            program,
            path: path.clone(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&ended.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    if !first.starts_with(&format!("{} version ", program.name())) {
        return Err(ToolError::NoVersion { program, path });
    }
    Ok((path, first.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workdir::WorkDir;

    fn work() -> WorkDir {
        WorkDir::create(&std::env::temp_dir()).unwrap()
    }

    #[tokio::test]
    async fn the_dev_shells_ffmpeg_and_ffprobe_report_their_versions() {
        let dir = work();
        for program in [Program::Ffmpeg, Program::Ffprobe] {
            let (path, version) = check(program, None, dir.path()).await.unwrap();
            assert!(path.is_absolute(), "{}", path.display());
            assert!(version.starts_with(&format!("{} version ", program.name())));
        }
    }

    #[tokio::test]
    async fn a_missing_or_wrong_program_is_refused_naming_it() {
        let dir = work();
        let missing = check(
            Program::Ffmpeg,
            Some(Path::new("/nonexistent/ffmpeg")),
            dir.path(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(missing, ToolError::DidNotRun { .. }),
            "{missing:?}"
        );
        assert_eq!(missing.program(), Program::Ffmpeg);
        // ffmpeg is not ffprobe: its version line names ffmpeg.
        let ffmpeg = find_on_path("ffmpeg").unwrap();
        let wrong = check(Program::Ffprobe, Some(&ffmpeg), dir.path())
            .await
            .unwrap_err();
        assert!(matches!(wrong, ToolError::NoVersion { .. }), "{wrong:?}");
        // `true` runs and prints nothing.
        let silent = find_on_path("true").unwrap();
        let quiet = check(Program::Ffmpeg, Some(&silent), dir.path())
            .await
            .unwrap_err();
        assert!(matches!(quiet, ToolError::NoVersion { .. }), "{quiet:?}");
    }

    #[test]
    fn a_name_not_on_path_is_not_found() {
        assert_eq!(find_on_path("stepclimb-no-such-program"), None);
    }
}
