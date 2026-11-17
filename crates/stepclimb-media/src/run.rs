// SPDX-License-Identifier: AGPL-3.0-or-later
//! One bounded child process (docs/specs/m1/08-uploaded-video.md B10).
//!
//! [`run`] starts the program directly, never through a shell, with an empty environment (no
//! `FFREPORT`, `AV_LOG_FORCE_*` or proxy variable reaches it), stdin closed and the work
//! directory as its working directory. While it runs, a watch every [`WATCH_INTERVAL`] kills it
//! when the files under the watched directory pass the output cap or, on Linux, its resident
//! memory passes the memory cap; the deadline kills it too. A killed or failed run keeps the
//! last [`STDERR_TAIL_BYTES`] of its stderr for the server's log, never for a learner.
//!
//! The memory watch reads `/proc/<pid>/status` between ticks, so an allocation burst shorter
//! than a tick can pass it; ffmpeg's `-max_alloc` ([`crate::plan`]) bounds each single
//! allocation, and the container's own memory limit stays the operator's backstop.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::Instant;

/// How much of a child's stderr is kept: its last 2 KiB (B10).
pub const STDERR_TAIL_BYTES: usize = 2 * 1024;

/// The most stdout [`run`] keeps when it captures it: 1 MiB, far more than ffprobe's JSON for one
/// file needs.
pub const MAX_STDOUT_BYTES: usize = 1024 * 1024;

/// How often the output size and memory are checked.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(250);

/// What one run may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// When the run is killed if it has not ended.
    pub deadline: Instant,
    /// The most bytes the files under the watched directory may hold.
    pub max_output_bytes: u64,
    /// The most resident memory the child may hold, in bytes (Linux only).
    pub max_memory_bytes: u64,
}

/// One program to run.
#[derive(Clone, Copy, Debug)]
pub struct Invocation<'a> {
    pub program: &'a Path,
    pub args: &'a [OsString],
    /// The working directory, where relative output names land.
    pub dir: &'a Path,
    /// The directory whose files count toward [`Limits::max_output_bytes`].
    pub watch: &'a Path,
    /// Whether stdout is kept (ffprobe's JSON) or discarded.
    pub capture_stdout: bool,
}

/// Why a run did not succeed.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("could not start {}: {source}", program.display())]
    Spawn {
        program: PathBuf,
        source: std::io::Error,
    },
    #[error("the run passed its deadline and was killed")]
    TimedOut,
    #[error("the run wrote more than {max} bytes and was killed")]
    OutputTooLarge { max: u64 },
    #[error("the run held more than {max} bytes of memory and was killed")]
    MemoryExceeded { max: u64 },
    #[error("the run wrote more than {MAX_STDOUT_BYTES} bytes to stdout")]
    StdoutTooLarge,
    /// The program ended unsuccessfully; `stderr` is the tail of what it wrote there, for the
    /// server's log only.
    #[error("the run exited with {status}")]
    Failed { status: ExitStatus, stderr: String },
    #[error("could not watch the run: {0}")]
    Io(#[from] std::io::Error),
}

impl RunError {
    /// Whether the input is the likely cause: the program ran and failed, or broke a limit. A
    /// spawn or watch failure is the host's.
    #[must_use]
    pub fn is_input_fault(&self) -> bool {
        !matches!(self, Self::Spawn { .. } | Self::Io(_))
    }
}

/// What a run that ended left: its stdout when captured, and the tail of its stderr.
#[derive(Debug)]
pub struct Ended {
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// Run `invocation` under `limits`. The child is killed when the returned future is dropped.
pub async fn run(invocation: Invocation<'_>, limits: Limits) -> Result<Ended, RunError> {
    let stdout = if invocation.capture_stdout {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = Command::new(invocation.program)
        .args(invocation.args)
        .current_dir(invocation.dir)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|source| RunError::Spawn {
            program: invocation.program.to_path_buf(),
            source,
        })?;
    let stderr = child.stderr.take().map(|s| tokio::spawn(tail(s)));
    let stdout = child.stdout.take().map(|s| tokio::spawn(capped(s)));
    let watched = watch(&mut child, invocation.watch, limits).await;
    if watched.is_err() {
        // Already ended or killed: either way the wait below reaps it.
        let _ = child.start_kill();
        let _ = child.wait().await;
    }
    let stderr = match stderr {
        Some(task) => task.await.unwrap_or_default(),
        None => String::new(),
    };
    let status = watched?;
    let (stdout, overflowed) = match stdout {
        Some(task) => task.await.unwrap_or_default(),
        None => (Vec::new(), false),
    };
    if !status.success() {
        return Err(RunError::Failed { status, stderr });
    }
    if overflowed {
        return Err(RunError::StdoutTooLarge);
    }
    Ok(Ended { stdout, stderr })
}

/// Wait for `child`, checking the limits every [`WATCH_INTERVAL`].
async fn watch(
    child: &mut tokio::process::Child,
    dir: &Path,
    limits: Limits,
) -> Result<ExitStatus, RunError> {
    let pid = child.id();
    let mut tick = tokio::time::interval(WATCH_INTERVAL);
    loop {
        tokio::select! {
            status = child.wait() => return Ok(status?),
            () = tokio::time::sleep_until(limits.deadline) => return Err(RunError::TimedOut),
            _ = tick.tick() => {
                let dir = dir.to_path_buf();
                let size = tokio::task::spawn_blocking(move || tree_size(&dir))
                    .await
                    .map_err(std::io::Error::other)??;
                if size > limits.max_output_bytes {
                    return Err(RunError::OutputTooLarge { max: limits.max_output_bytes });
                }
                if pid.and_then(resident_bytes).is_some_and(|rss| rss > limits.max_memory_bytes) {
                    return Err(RunError::MemoryExceeded { max: limits.max_memory_bytes });
                }
            }
        }
    }
}

/// The bytes of every file under `dir`; a file removed while it is walked counts as nothing.
pub fn tree_size(dir: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        let entries = match std::fs::read_dir(&at) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(total)
}

/// The resident memory of process `pid` in bytes, from `/proc/<pid>/status`; `None` when it
/// cannot be read (the process ended, or the OS is not Linux).
#[must_use]
pub fn resident_bytes(pid: u32) -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// The last [`STDERR_TAIL_BYTES`] of `stream`, lossily as UTF-8.
async fn tail(mut stream: impl AsyncRead + Unpin) -> String {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 4096];
    while let Ok(read) = stream.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        kept.extend_from_slice(&buffer[..read]);
        if kept.len() > STDERR_TAIL_BYTES {
            kept.drain(..kept.len() - STDERR_TAIL_BYTES);
        }
    }
    String::from_utf8_lossy(&kept).into_owned()
}

/// Up to [`MAX_STDOUT_BYTES`] of `stream`, read to its end, and whether it held more.
async fn capped(mut stream: impl AsyncRead + Unpin) -> (Vec<u8>, bool) {
    let mut kept = Vec::new();
    let mut overflowed = false;
    let mut buffer = [0u8; 8192];
    while let Ok(read) = stream.read(&mut buffer).await {
        if read == 0 {
            break;
        }
        let room = MAX_STDOUT_BYTES - kept.len();
        overflowed |= read > room;
        kept.extend_from_slice(&buffer[..read.min(room)]);
    }
    (kept, overflowed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::find_on_path;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn limits(deadline: Duration) -> Limits {
        Limits {
            deadline: Instant::now() + deadline,
            max_output_bytes: GIB,
            max_memory_bytes: 4 * GIB,
        }
    }

    fn program(name: &str) -> PathBuf {
        find_on_path(name).unwrap_or_else(|| panic!("{name} on PATH (the dev shell has it)"))
    }

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    fn scratch() -> crate::workdir::WorkDir {
        crate::workdir::WorkDir::create(&std::env::temp_dir()).expect("a work directory")
    }

    async fn go(name: &str, list: &[&str], dir: &Path, limits: Limits) -> Result<Ended, RunError> {
        let program = program(name);
        let args = args(list);
        let invocation = Invocation {
            program: &program,
            args: &args,
            dir,
            watch: dir,
            capture_stdout: true,
        };
        run(invocation, limits).await
    }

    #[tokio::test]
    async fn the_child_gets_an_empty_environment_and_the_work_directory() {
        let dir = scratch();
        let ended = go("env", &[], dir.path(), limits(Duration::from_secs(30)))
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&ended.stdout), "", "no variable");
        let ended = go("pwd", &[], dir.path(), limits(Duration::from_secs(30)))
            .await
            .unwrap();
        let printed = String::from_utf8_lossy(&ended.stdout);
        assert_eq!(Path::new(printed.trim()), dir.path());
    }

    #[tokio::test]
    async fn a_run_past_its_deadline_is_killed() {
        let dir = scratch();
        let started = std::time::Instant::now();
        let error = go(
            "sleep",
            &["30"],
            dir.path(),
            limits(Duration::from_millis(300)),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, RunError::TimedOut), "{error:?}");
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(error.is_input_fault());
    }

    #[tokio::test]
    async fn a_run_that_writes_past_the_output_cap_is_killed() {
        let dir = scratch();
        let mut limits = limits(Duration::from_secs(60));
        limits.max_output_bytes = 1024 * 1024;
        // 6.5 GB of zeros: the cap stops it, not the deadline.
        let error = go(
            "dd",
            &["if=/dev/zero", "of=big", "bs=65536", "count=100000"],
            dir.path(),
            limits,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, RunError::OutputTooLarge { .. }),
            "{error:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_run_that_holds_more_memory_than_its_cap_is_killed() {
        let dir = scratch();
        let mut limits = limits(Duration::from_secs(60));
        limits.max_memory_bytes = 64 * 1024 * 1024;
        let error = go(
            "python3",
            &[
                "-c",
                "import time; x = bytearray(512 * 1024 * 1024); time.sleep(30)",
            ],
            dir.path(),
            limits,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, RunError::MemoryExceeded { .. }),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_run_keeps_only_the_tail_of_its_stderr() {
        let dir = scratch();
        let long = format!("{}END", "x".repeat(10_000));
        let error = go("ls", &[&long], dir.path(), limits(Duration::from_secs(30)))
            .await
            .unwrap_err();
        let RunError::Failed { stderr, .. } = &error else {
            panic!("{error:?}")
        };
        assert!(stderr.len() <= STDERR_TAIL_BYTES, "{}", stderr.len());
        assert!(stderr.contains("END"), "{stderr}");
        assert!(
            !error.to_string().contains("END"),
            "Display holds no stderr"
        );
    }

    #[tokio::test]
    async fn a_missing_program_is_a_spawn_error_not_an_input_fault() {
        let dir = scratch();
        let invocation = Invocation {
            program: Path::new("/nonexistent/ffmpeg"),
            args: &[],
            dir: dir.path(),
            watch: dir.path(),
            capture_stdout: false,
        };
        let error = run(invocation, limits(Duration::from_secs(5)))
            .await
            .unwrap_err();
        assert!(matches!(error, RunError::Spawn { .. }), "{error:?}");
        assert!(!error.is_input_fault());
    }

    #[test]
    fn the_tree_size_counts_nested_files() {
        let dir = scratch();
        std::fs::create_dir(dir.path().join("a")).unwrap();
        std::fs::write(dir.path().join("a/b"), [0u8; 10]).unwrap();
        std::fs::write(dir.path().join("c"), [0u8; 5]).unwrap();
        assert_eq!(tree_size(dir.path()).unwrap(), 15);
        assert_eq!(tree_size(&dir.path().join("missing")).unwrap(), 0);
    }
}
