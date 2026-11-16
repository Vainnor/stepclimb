// SPDX-License-Identifier: AGPL-3.0-or-later
//! `files.cleanup` (docs/specs/m1/06-files.md B8, B21): every hour, in each organization,
//!
//! 1. delete the temporary upload objects (`orgs/<org_id>/uploads/`) last written 24 hours ago
//!    or more, which only a crashed upload leaves;
//! 2. delete up to `files::UNREFERENCED_BATCH` `files` rows nothing has referenced for 24 hours,
//!    each with its object after the row's commit;
//! 3. delete the file objects (`orgs/<org_id>/files/`) no row names: an object whose delete in
//!    step 2 failed on an earlier run, or the copy of a new hash whose upload failed in its
//!    commit.
//!
//! Steps 2 and 3 hold the hash's advisory lock (`files::lock_hash_session`) from before the row
//! is checked until the object is gone, and an upload holds the same lock from before it looks
//! the hash up until it commits its row ([`crate::upload`]). So an object is never deleted
//! between an upload's copy and its commit, and a committed row always has its object.
//!
//! Without storage (B1a) the job deletes rows only, logging once per run that it deleted no
//! object; step 3 deletes their objects once storage is configured. A failure in one
//! organization is logged and the others still run; the job retries if any failed. Safe to run
//! twice: a second run finds nothing old enough left (spec m1/01 B12).

use std::time::{Duration, SystemTime};

use object_store::{ObjectStoreExt as _, path::Path};
use sqlx::{Acquire as _, PgConnection, PgPool};
use stepclimb_core::{
    OrgScope,
    files::{self, Sha256Digest},
    id::Uuid,
    organizations,
};
use stepclimb_jobs::{JobContext, Kind, Outcome, Registry, RegistryError};

use crate::storage::Storage;

/// The kind.
pub const CLEANUP: Kind = Kind::new("files.cleanup", "default");

/// How often it runs (B8).
pub const INTERVAL: Duration = Duration::from_secs(60 * 60);

/// How old a temporary upload object, and how long unreferenced a `files` row, must be before
/// the job deletes it (B8, B21).
pub const RETENTION: Duration = Duration::from_secs(24 * 60 * 60);

/// Register `files.cleanup` in `registry`, with the process's `storage`.
pub fn register(registry: &mut Registry, storage: Option<Storage>) -> Result<(), RegistryError> {
    registry.register_periodic(CLEANUP, INTERVAL, move |ctx: JobContext| {
        let storage = storage.clone();
        async move { cleanup(ctx, storage).await }
    })
}

async fn cleanup(ctx: JobContext, storage: Option<Storage>) -> Outcome {
    match run(ctx.pool(), storage.as_ref(), ctx.clock().now()).await {
        Ok(report) if report.failed > 0 => Outcome::retry(format_args!(
            "could not clean up files in {} places",
            report.failed
        )),
        Ok(_) => Outcome::Done,
        Err(error) => Outcome::retry(format_args!("could not list organizations: {error}")),
    }
}

/// What one run did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub uploads_deleted: u64,
    pub files_deleted: u64,
    pub orphans_deleted: u64,
    /// Steps that failed in some organization, each logged.
    pub failed: usize,
}

/// One run of the job at `now`, in every organization. Only failing to list the organizations
/// stops it.
pub async fn run(
    pool: &PgPool,
    storage: Option<&Storage>,
    now: SystemTime,
) -> Result<Report, organizations::Error> {
    let mut report = Report::default();
    let Some(cutoff) = now.checked_sub(RETENTION) else {
        return Ok(report);
    };
    if storage.is_none() {
        tracing::warn!(
            "files.cleanup runs without object storage: it deletes unreferenced rows and no object"
        );
    }
    for org_id in organizations::ids(pool).await? {
        let scope = job_scope(org_id);
        let cleaner = Cleaner {
            pool,
            storage,
            scope,
            cutoff,
        };
        cleaner.run(&mut report).await;
    }
    if report.files_deleted + report.uploads_deleted + report.orphans_deleted > 0 {
        tracing::info!(?report, "cleaned up files");
    }
    Ok(report)
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a job; files.cleanup acts in each organization in turn (spec m1/01 B10)"
)]
fn job_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Why a step failed.
#[derive(Debug, thiserror::Error)]
enum Error {
    #[error(transparent)]
    Files(#[from] files::Error),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Store(#[from] object_store::Error),
}

/// One organization's run.
struct Cleaner<'a> {
    pool: &'a PgPool,
    storage: Option<&'a Storage>,
    scope: OrgScope,
    cutoff: SystemTime,
}

impl Cleaner<'_> {
    async fn run(&self, report: &mut Report) {
        let org_id = self.scope.org_id();
        if let Some(storage) = self.storage {
            match self.stale_uploads(storage).await {
                Ok(count) => report.uploads_deleted += count,
                Err(error) => {
                    report.failed += 1;
                    tracing::error!(%org_id, %error, "could not delete stale upload objects");
                }
            }
        }
        match self.unreferenced().await {
            Ok(count) => report.files_deleted += count,
            Err(error) => {
                report.failed += 1;
                tracing::error!(%org_id, %error, "could not delete unreferenced files");
            }
        }
        if let Some(storage) = self.storage {
            match self.orphans(storage).await {
                Ok(count) => report.orphans_deleted += count,
                Err(error) => {
                    report.failed += 1;
                    tracing::error!(%org_id, %error, "could not delete file objects without a row");
                }
            }
        }
    }

    /// Step 1: the temporary objects last written at or before the cutoff.
    async fn stale_uploads(&self, storage: &Storage) -> Result<u64, Error> {
        let listed = storage
            .store()
            .list_with_delimiter(Some(&storage.uploads_prefix(self.scope)))
            .await?;
        let mut deleted = 0;
        for object in listed.objects {
            if SystemTime::from(object.last_modified) <= self.cutoff {
                delete_object(storage, &object.location).await?;
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    /// Step 2: the rows nothing has referenced since the cutoff, each under its hash lock.
    async fn unreferenced(&self) -> Result<u64, Error> {
        let mut deleted = 0;
        for (id, sha256) in files::unreferenced(self.pool, self.scope, self.cutoff).await? {
            let (scope, cutoff, storage) = (self.scope, self.cutoff, self.storage);
            let done = self
                .under_hash_lock(sha256, async |conn| {
                    let mut tx = conn.begin().await?;
                    let row = files::delete(&mut tx, scope, id, cutoff).await?;
                    tx.commit().await?;
                    let Some(row) = row else { return Ok(false) };
                    if let Some(storage) = storage
                        && let Err(error) =
                            delete_object(storage, &storage.path(&row.storage_key)).await
                    {
                        // Step 3 finds the object again on the next run: no row names it now.
                        tracing::warn!(%error, key = %row.storage_key, "could not delete a file object");
                    }
                    Ok(true)
                })
                .await?;
            deleted += u64::from(done);
        }
        Ok(deleted)
    }

    /// Step 3: the file objects no row names, each deleted only after its hash lock is taken
    /// and the row is still missing.
    async fn orphans(&self, storage: &Storage) -> Result<u64, Error> {
        let listed = storage
            .store()
            .list_with_delimiter(Some(&storage.files_prefix(self.scope)))
            .await?;
        let mut deleted = 0;
        for object in listed.objects {
            let key = storage.key_of(&object.location);
            let Some(sha256) = key.and_then(|key| files::digest_of_key(self.scope, &key)) else {
                tracing::warn!(key = %object.location, "an object under files/ that no file names");
                continue;
            };
            if files::exists(self.pool, self.scope, sha256).await? {
                continue;
            }
            let scope = self.scope;
            let removed = self
                .under_hash_lock(sha256, async |conn| {
                    if files::exists(&mut *conn, scope, sha256).await? {
                        return Ok(false);
                    }
                    delete_object(storage, &object.location).await?;
                    Ok(true)
                })
                .await?;
            deleted += u64::from(removed);
        }
        Ok(deleted)
    }

    /// `work` on a connection of its own holding `sha256`'s hash lock, released after it. A
    /// connection whose unlock fails is closed, which releases the lock with the session.
    async fn under_hash_lock<T>(
        &self,
        sha256: Sha256Digest,
        work: impl AsyncFnOnce(&mut PgConnection) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut conn = self.pool.acquire().await?;
        files::lock_hash_session(&mut conn, self.scope, sha256).await?;
        let result = work(&mut conn).await;
        match files::unlock_hash_session(&mut conn, self.scope, sha256).await {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                tracing::warn!("could not release a file hash lock; closing its connection");
                drop(conn.detach());
            }
        }
        result
    }
}

/// Delete the object at `location`; one already gone counts as deleted.
async fn delete_object(storage: &Storage, location: &Path) -> Result<(), object_store::Error> {
    match storage.store().delete(location).await {
        Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
        Err(error) => Err(error),
    }
}
