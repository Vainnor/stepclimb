// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deleting a video's stored objects (docs/specs/m1/08-uploaded-video.md B5, B6, B12, D7, D13):
//! an upload's parts, every object of a video, and the outputs of transcode runs a newer run
//! replaced.
//!
//! An upload is first marked terminating in a transaction that holds its row's lock
//! (`stepclimb_core::videos::mark_terminating`). From that commit on no chunk is stored for it:
//! a chunk takes the same lock, sees the mark and answers 404 before it copies anything. So
//! [`finish`] lists and deletes the parts holding no lock at all, then deletes the rows in a short
//! transaction. A delete that fails leaves the upload marked, and `videos.cleanup` finishes it.

use std::time::SystemTime;

use object_store::{ObjectStoreExt as _, path::Path};
use sqlx::PgPool;
use stepclimb_core::{OrgScope, id::Uuid, videos};
use tokio::task::JoinSet;

use crate::storage::Storage;

/// How many part deletes run at once.
const DELETES_AT_ONCE: usize = 16;

/// Why the parts or rows of a terminating upload could not be deleted.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Store(#[from] object_store::Error),
    #[error("a part delete did not finish: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error(transparent)]
    Videos(#[from] videos::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The prefix of the video `video_id`'s upload parts, `orgs/<org>/videos/<id>/upload/`.
pub(crate) fn parts_prefix(storage: &Storage, scope: OrgScope, video_id: Uuid) -> Path {
    Path::from_iter(
        storage
            .video_prefix(scope, video_id)
            .parts()
            .chain(std::iter::once("upload".into())),
    )
}

/// Delete every stored part of the video `video_id`'s upload. The number deleted.
pub async fn delete_parts(
    storage: &Storage,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<usize, Error> {
    delete_tree(storage, &parts_prefix(storage, scope, video_id)).await
}

/// Delete every object under `prefix`, at any depth, [`DELETES_AT_ONCE`] at a time. An object
/// already gone counts as deleted. The number deleted.
pub async fn delete_tree(storage: &Storage, prefix: &Path) -> Result<usize, Error> {
    let mut count = 0;
    let mut pending = vec![prefix.clone()];
    let mut running = JoinSet::new();
    while let Some(at) = pending.pop() {
        let listed = storage.store().list_with_delimiter(Some(&at)).await?;
        pending.extend(listed.common_prefixes);
        count += listed.objects.len();
        for object in listed.objects {
            if running.len() >= DELETES_AT_ONCE
                && let Some(done) = running.join_next().await
            {
                done??;
            }
            let store = storage.store().clone();
            running.spawn(async move {
                match store.delete(&object.location).await {
                    Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
                    Err(error) => Err(error),
                }
            });
        }
    }
    while let Some(done) = running.join_next().await {
        done??;
    }
    Ok(count)
}

/// Delete the upload parts no upload row owns, in the scope's organization, last written at
/// `cutoff` or before (docs/specs/m1/08-uploaded-video.md D13, D45): a chunk whose `CopyObject`
/// landed after a termination had listed the parts. For each video directory under
/// `orgs/<org>/videos/`, a video that has an upload row keeps its parts; one without (its upload
/// ended, or the video is gone) has its older parts deleted. A chunk stores a part only under its
/// upload row's lock, and no upload row is created for an existing video, so no upload in
/// progress loses a part; `cutoff` leaves a copy still landing alone. The number deleted.
pub async fn delete_orphaned_parts(
    pool: &PgPool,
    storage: &Storage,
    scope: OrgScope,
    cutoff: SystemTime,
) -> Result<usize, Error> {
    let videos = storage
        .store()
        .list_with_delimiter(Some(&storage.videos_prefix(scope)))
        .await?;
    let mut count = 0;
    for video in videos.common_prefixes {
        let Some(video_id) = video.filename().and_then(|id| Uuid::parse_str(id).ok()) else {
            continue;
        };
        let prefix = parts_prefix(storage, scope, video_id);
        let parts = storage.store().list_with_delimiter(Some(&prefix)).await?;
        let old: Vec<Path> = parts
            .objects
            .into_iter()
            .filter(|part| SystemTime::from(part.last_modified) <= cutoff)
            .map(|part| part.location)
            .collect();
        if old.is_empty() || videos::has_upload(pool, scope, video_id).await? {
            continue;
        }
        for part in old {
            match storage.store().delete(&part).await {
                Ok(()) | Err(object_store::Error::NotFound { .. }) => count += 1,
                Err(error) => return Err(error.into()),
            }
        }
    }
    Ok(count)
}

/// Delete the outputs of every transcode run of the video `video_id` but `keep`
/// (`orgs/<org>/videos/<id>/v<n>/` for each `n` other than `keep`), after `keep` committed
/// (docs/specs/m1/08-uploaded-video.md B12). The upload's parts are left to [`finish`].
pub async fn delete_other_runs(
    storage: &Storage,
    scope: OrgScope,
    video_id: Uuid,
    keep: i32,
) -> Result<usize, Error> {
    let prefix = storage.video_prefix(scope, video_id);
    let listed = storage.store().list_with_delimiter(Some(&prefix)).await?;
    let keep = format!("v{keep}");
    let mut count = 0;
    for run in listed.common_prefixes {
        let name = run.filename().unwrap_or_default();
        let is_run = name
            .strip_prefix('v')
            .is_some_and(|n| n.parse::<u32>().is_ok());
        if is_run && name != keep {
            count += delete_tree(storage, &run).await?;
        }
    }
    Ok(count)
}

/// Finish the terminating upload `upload_id`: delete its parts ([`videos::Terminating::Parts`])
/// or every object of its video, transcode outputs included ([`videos::Terminating::Video`]),
/// then, under its row's lock, its row or its video as the mark says. Whether it was finished here; an upload that is gone or not
/// marked is left alone.
pub async fn finish(
    pool: &PgPool,
    storage: &Storage,
    scope: OrgScope,
    upload_id: Uuid,
) -> Result<bool, Error> {
    let Some(upload) = videos::get_upload(pool, scope, upload_id).await? else {
        return Ok(false);
    };
    match upload.terminating {
        None => return Ok(false),
        Some(videos::Terminating::Parts) => {
            delete_parts(storage, scope, upload.video_id).await?;
        }
        // A failed video may hold transcode outputs besides its parts.
        Some(videos::Terminating::Video) => {
            delete_tree(storage, &storage.video_prefix(scope, upload.video_id)).await?;
        }
    }
    let mut tx = pool.begin().await?;
    let Some(locked) = videos::lock_upload(&mut tx, scope, upload_id).await? else {
        return Ok(false);
    };
    let finished = videos::finish_termination(&mut tx, scope, &locked).await?;
    tx.commit().await?;
    Ok(finished)
}
