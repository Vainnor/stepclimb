// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deleting a video upload's stored parts (docs/specs/m1/08-uploaded-video.md B5, B6, D7, D13).
//!
//! An upload is first marked terminating in a transaction that holds its row's lock
//! (`stepclimb_core::videos::mark_terminating`). From that commit on no chunk is stored for it:
//! a chunk takes the same lock, sees the mark and answers 404 before it copies anything. So
//! [`finish`] lists and deletes the parts holding no lock at all, then deletes the rows in a short
//! transaction. A delete that fails leaves the upload marked, and `videos.cleanup` finishes it.

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

/// Delete every stored part of the video `video_id`'s upload, [`DELETES_AT_ONCE`] at a time. A
/// part already gone counts as deleted. The number deleted.
pub async fn delete_parts(
    storage: &Storage,
    scope: OrgScope,
    video_id: Uuid,
) -> Result<usize, Error> {
    let prefix = Path::from_iter(
        storage
            .video_prefix(scope, video_id)
            .parts()
            .chain(std::iter::once("upload".into())),
    );
    let listed = storage.store().list_with_delimiter(Some(&prefix)).await?;
    let count = listed.objects.len();
    let mut running = JoinSet::new();
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
    while let Some(done) = running.join_next().await {
        done??;
    }
    Ok(count)
}

/// Finish the terminating upload `upload_id`: delete its parts, then, under its row's lock, its
/// row or its video as the mark says. Whether it was finished here; an upload that is gone or not
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
    if upload.terminating.is_none() {
        return Ok(false);
    }
    delete_parts(storage, scope, upload.video_id).await?;
    let mut tx = pool.begin().await?;
    let Some(locked) = videos::lock_upload(&mut tx, scope, upload_id).await? else {
        return Ok(false);
    };
    let finished = videos::finish_termination(&mut tx, scope, &locked).await?;
    tx.commit().await?;
    Ok(finished)
}
