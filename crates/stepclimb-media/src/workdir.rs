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
    fn two_work_directories_never_share_a_path() {
        let base = std::env::temp_dir();
        let (a, b) = (
            WorkDir::create(&base).unwrap(),
            WorkDir::create(&base).unwrap(),
        );
        assert_ne!(a.path(), b.path());
    }
}
