// SPDX-License-Identifier: AGPL-3.0-or-later
//! Moving a transcode's bytes between the store and its work directory
//! (docs/specs/m1/08-uploaded-video.md B9, B12).
//!
//! [`concatenate`] writes an upload's parts, in offset order, into one local file, checking
//! that they cover the upload's length with no gap; [`upload_outputs`] puts every file of the
//! output directory under the run's prefix `orgs/<org>/videos/<id>/v<n>/` and counts their bytes.
//! File writes and reads run on blocking threads.

use std::io::{Read as _, Write as _};
use std::path::{Path as FsPath, PathBuf};

use object_store::{ObjectStoreExt as _, PutPayload, WriteMultipart, path::Path};
use stepclimb_core::{OrgScope, id::Uuid, videos::Upload};

use crate::{storage::Storage, video_parts};

/// Files up to this size are put in one request; larger ones in parts of this size.
const PART_BYTES: usize = 16 * 1024 * 1024;

/// Why bytes could not be moved.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("a blocking file task did not finish: {0}")]
    Task(#[from] tokio::task::JoinError),
    /// The stored parts do not cover the upload's length from 0 with no gap.
    #[error("the stored parts do not make up the upload")]
    Incomplete,
    /// The output directory holds an entry that is neither a directory nor a regular file
    /// (a symbolic link, a pipe); nothing of it is uploaded.
    #[error("the output {0} is not a regular file")]
    NotAFile(String),
}

/// The parts of the video `video_id`'s upload: `(offset, location)` in offset order.
async fn parts(
    storage: &Storage,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<Vec<(u64, Path)>, Error> {
    let prefix = video_parts::parts_prefix(storage, scope, video_id);
    let listed = storage.store().list_with_delimiter(Some(&prefix)).await?;
    let mut parts: Vec<(u64, Path)> = listed
        .objects
        .into_iter()
        .filter_map(|o| Some((o.location.filename()?.parse().ok()?, o.location)))
        .collect();
    parts.sort_by_key(|(offset, _)| *offset);
    Ok(parts)
}

/// Write the parts of `upload` below its length into `into`, in offset order. A part at an
/// offset the previous parts do not reach, or a total short of the length, is
/// [`Error::Incomplete`]. A part at or past the length (a chunk whose commit failed,
/// `crate::video_uploads`) is not part of the upload and is skipped.
pub async fn concatenate(
    storage: &Storage,
    scope: OrgScope,
    upload: &Upload,
    into: &FsPath,
) -> Result<(), Error> {
    let mut file = std::fs::File::create_new(into)?;
    let mut written = 0u64;
    for (offset, location) in parts(storage, scope, upload.video_id).await? {
        if offset >= upload.length {
            continue;
        }
        if offset != written {
            return Err(Error::Incomplete);
        }
        let bytes = storage.store().get(&location).await?.bytes().await?;
        written += u64::try_from(bytes.len()).map_err(|_| Error::Incomplete)?;
        file = tokio::task::spawn_blocking(move || {
            let mut file = file;
            file.write_all(&bytes).map(|()| file)
        })
        .await??;
    }
    if written != upload.length {
        return Err(Error::Incomplete);
    }
    tokio::task::spawn_blocking(move || file.sync_all()).await??;
    Ok(())
}

/// The first `n` bytes of the file at `path`, or all of it if shorter.
pub async fn head(path: &FsPath, n: usize) -> Result<Vec<u8>, Error> {
    let path = path.to_path_buf();
    let head = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        let mut head = Vec::with_capacity(n);
        std::fs::File::open(path)?
            .take(u64::try_from(n).unwrap_or(u64::MAX))
            .read_to_end(&mut head)?;
        Ok(head)
    })
    .await??;
    Ok(head)
}

/// Every regular file under `dir`, as `(relative path with '/', absolute path, size)`, sorted.
/// Entries are read without following links, and anything that is neither a directory nor a
/// regular file is [`Error::NotAFile`], so a link ffmpeg was made to write cannot upload a file
/// from outside the work directory.
fn walk(dir: &FsPath) -> Result<Vec<(String, PathBuf, u64)>, Error> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(at) = pending.pop() {
        for entry in std::fs::read_dir(&at)? {
            let entry = entry?;
            // `DirEntry::metadata` does not follow a symbolic link.
            let meta = entry.metadata()?;
            let path = entry.path();
            if meta.is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(dir)
                .map_err(std::io::Error::other)?
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if !meta.is_file() {
                return Err(Error::NotAFile(relative));
            }
            found.push((relative, path, meta.len()));
        }
    }
    found.sort();
    Ok(found)
}

/// Put every file under `out` at `run_prefix/<relative path>`. The bytes put.
pub async fn upload_outputs(
    storage: &Storage,
    out: &FsPath,
    run_prefix: &Path,
) -> Result<u64, Error> {
    let dir = out.to_path_buf();
    let files = tokio::task::spawn_blocking(move || walk(&dir)).await??;
    let mut total = 0u64;
    for (relative, path, size) in files {
        let location = Path::from_iter(
            run_prefix
                .parts()
                .chain(relative.split('/').map(Into::into)),
        );
        put_file(storage, &path, &location).await?;
        total = total.saturating_add(size);
    }
    Ok(total)
}

/// Put the file at `path` at `location`, in one request or in parts of [`PART_BYTES`].
async fn put_file(storage: &Storage, path: &FsPath, location: &Path) -> Result<(), Error> {
    let mut file = std::fs::File::open(path)?;
    let first;
    (file, first) = read_chunk(file).await?;
    if first.len() < PART_BYTES {
        storage
            .store()
            .put(location, PutPayload::from(first))
            .await?;
        return Ok(());
    }
    let mut writer = WriteMultipart::new_with_chunk_size(
        storage.store().put_multipart(location).await?,
        PART_BYTES,
    );
    let mut chunk = first;
    while !chunk.is_empty() {
        writer.wait_for_capacity(2).await?;
        writer.write(&chunk);
        (file, chunk) = read_chunk(file).await?;
    }
    writer.finish().await?;
    Ok(())
}

/// Up to [`PART_BYTES`] more of `file`; empty at its end.
async fn read_chunk(file: std::fs::File) -> Result<(std::fs::File, Vec<u8>), Error> {
    let read = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let mut chunk = Vec::with_capacity(PART_BYTES);
        let mut file = file;
        (&mut file)
            .take(u64::try_from(PART_BYTES).unwrap_or(u64::MAX))
            .read_to_end(&mut chunk)?;
        Ok((file, chunk))
    })
    .await??;
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_output_walk_refuses_a_link_and_lists_regular_files() {
        let base = std::env::temp_dir().join(format!("stepclimb-walk-{}", Uuid::now_v7().simple()));
        std::fs::create_dir_all(base.join("360p")).unwrap();
        std::fs::write(base.join("360p/index.m3u8"), b"#EXTM3U").unwrap();
        std::fs::write(base.join("poster.jpg"), b"jpg").unwrap();
        let listed: Vec<_> = walk(&base)
            .unwrap()
            .into_iter()
            .map(|(r, _, n)| (r, n))
            .collect();
        assert_eq!(
            listed,
            [
                ("360p/index.m3u8".to_owned(), 7),
                ("poster.jpg".to_owned(), 3)
            ]
        );
        std::os::unix::fs::symlink("/etc/hostname", base.join("360p/segment_00000.ts")).unwrap();
        let refused = walk(&base);
        std::fs::remove_dir_all(&base).unwrap();
        assert!(
            matches!(refused, Err(Error::NotAFile(ref name)) if name == "360p/segment_00000.ts"),
            "{refused:?}"
        );
    }
}
