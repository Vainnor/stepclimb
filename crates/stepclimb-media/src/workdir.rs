// SPDX-License-Identifier: AGPL-3.0-or-later
//! One transcode's private temporary directory (docs/specs/m1/08-uploaded-video.md B9, B12).
//!
//! [`WorkDir::create`] makes `stepclimb-transcode-<random>` inside `transcode_dir`, readable by
//! the server's user only on Unix, and fails rather than reuse a directory that exists. Every
//! file of the transcode (the concatenated upload and ffmpeg's outputs) lives under it, and
//! dropping it removes the whole tree, whether the transcode succeeded, failed or panicked.

use std::path::{Path, PathBuf};

/// The prefix of every work directory's name.
pub const PREFIX: &str = "stepclimb-transcode-";

/// A temporary directory removed with its contents when dropped.
#[derive(Debug)]
pub struct WorkDir {
    path: PathBuf,
}

impl WorkDir {
    /// A new, empty directory inside `base`.
    pub fn create(base: &Path) -> std::io::Result<Self> {
        let path = base.join(format!("{PREFIX}{}", uuid::Uuid::now_v7().simple()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&path)?;
        Ok(Self { path })
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Remove every work directory in `base` last modified before `cutoff`: one a process killed
/// mid-transcode left behind. A process that transcodes calls it at startup with a cutoff past
/// the longest a transcode may run, so no running transcode's directory is removed. Other
/// entries are left alone. The number removed.
pub fn sweep(base: &Path, cutoff: std::time::SystemTime) -> std::io::Result<usize> {
    let mut removed = 0;
    for entry in std::fs::read_dir(base)? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with(PREFIX) {
            continue;
        }
        // Another process may remove an entry between the listing and this read.
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !meta.is_dir() || meta.modified()? >= cutoff {
            continue;
        }
        match std::fs::remove_dir_all(entry.path()) {
            Ok(()) => removed += 1,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(removed)
}

impl Drop for WorkDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(%error, path = %self.path.display(), "could not remove a transcode's work directory");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_work_directory_is_private_and_removed_with_its_files_on_drop() {
        let dir = WorkDir::create(&std::env::temp_dir()).unwrap();
        let path = dir.path().to_path_buf();
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(PREFIX)
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        std::fs::create_dir(path.join("out")).unwrap();
        std::fs::write(path.join("out/segment.ts"), b"x").unwrap();
        drop(dir);
        assert!(!path.exists());
    }

    #[test]
    fn the_sweep_removes_only_old_work_directories() {
        let base = WorkDir::create(&std::env::temp_dir()).unwrap();
        let old = WorkDir::create(base.path()).unwrap();
        let old_path = old.path().to_path_buf();
        std::fs::write(old_path.join("input"), b"x").unwrap();
        std::mem::forget(old);
        let other = base.path().join("keep-me");
        std::fs::create_dir(&other).unwrap();
        let hour = std::time::Duration::from_secs(3600);
        let past = std::time::SystemTime::now() - hour;
        assert_eq!(sweep(base.path(), past).unwrap(), 0, "too recent");
        assert!(old_path.exists());
        let future = std::time::SystemTime::now() + hour;
        assert_eq!(sweep(base.path(), future).unwrap(), 1);
        assert!(!old_path.exists());
        assert!(other.exists(), "not a work directory");
    }

    #[test]
    fn two_work_directories_never_share_a_path() {
        let base = std::env::temp_dir();
        let (a, b) = (
            WorkDir::create(&base).unwrap(),
            WorkDir::create(&base).unwrap(),
        );
        assert_ne!(a.path(), b.path());
    }
}
