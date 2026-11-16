// SPDX-License-Identifier: AGPL-3.0-or-later
//! The stored files repository (docs/specs/m1/06-files.md B2, B7, B22, Data).
//!
//! A `files` row is one distinct file of one organization: the same bytes are stored once per
//! organization (`UNIQUE (org_id, sha256)`, Q9), so an upload whose hash the organization
//! already has adds a `course_files` row and no bytes. The row's object key is derived here from
//! the scope and the hash ([`storage_key`]), never from anything a client sent (B2). Every query
//! filters by `org_id = scope.org_id()`.
//!
//! This module holds rows only; the bytes are the server's storage layer's. Which upload may
//! add a row, and the order of object copy, insert and commit (B7), are the upload route's.

use std::fmt;
use std::time::SystemTime;

use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
};

/// The unique constraint that stores a hash once per organization (PostgreSQL's default name
/// for the migration's `UNIQUE (org_id, sha256)`).
const HASH_CONSTRAINT: &str = "files_org_id_sha256_key";

/// The shortest and longest stored MIME type, in characters (the table's check).
const MIME_CHARS: std::ops::RangeInclusive<usize> = 3..=100;

/// A file's SHA-256 digest.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256(pub [u8; 32]);

impl Sha256 {
    /// The digest as 64 lowercase hexadecimal digits, as object keys and `ETag`s write it.
    #[must_use]
    pub fn hex(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for Sha256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256({self})")
    }
}

/// The object key of the scope's organization's file with `sha256`, relative to the store's
/// prefix: `orgs/<org_id>/files/<sha256 hex>` (B2). Built from the scope and the hash only, so
/// no client string reaches a storage path.
#[must_use]
pub fn storage_key(scope: OrgScope, sha256: Sha256) -> String {
    format!("orgs/{}/files/{sha256}", scope.org_id())
}

/// A stored file as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub id: Uuid,
    pub sha256: Sha256,
    /// The file's length in bytes.
    pub size: u64,
    /// The sniffed type (B6), never the client's.
    pub mime: String,
    /// Where the bytes are, relative to the store's prefix: [`storage_key`] of the row's
    /// organization and hash.
    pub storage_key: String,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A file to record once its bytes are at [`storage_key`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewFile {
    pub sha256: Sha256,
    pub size: u64,
    /// 3 to 100 characters.
    pub mime: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The organization already has a file with this hash: another upload of the same bytes
    /// committed first. B7's loser reads the row with [`find_by_hash`] and adds nothing.
    #[error("the organization already has a file with this SHA-256")]
    HashTaken,
    /// A MIME type outside 3 to 100 characters, or one holding NUL.
    #[error("a stored MIME type has 3 to 100 characters and no NUL")]
    MimeInvalid,
    /// A size past what a `bigint` holds, which no upload under `max_file_bytes` reaches.
    #[error("a file size is larger than the database stores")]
    SizeOutOfRange,
    /// A row whose hash is not 32 bytes or whose size is negative, which the table's checks keep
    /// from happening.
    #[error("the {column} of file {id} cannot be read")]
    Stored { id: Uuid, column: &'static str },
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some(HASH_CONSTRAINT) => Self::HashTaken,
            _ => Self::Database(error),
        }
    }
}

/// A `files` row as the queries select it.
struct Row {
    id: Uuid,
    sha256: Vec<u8>,
    size: i64,
    mime: String,
    storage_key: String,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for File {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let stored = |column| Error::Stored { id: row.id, column };
        let sha256 = <[u8; 32]>::try_from(row.sha256.as_slice()).map_err(|_| stored("sha256"))?;
        Ok(Self {
            id: row.id,
            sha256: Sha256(sha256),
            size: u64::try_from(row.size).map_err(|_| stored("size"))?,
            mime: row.mime,
            storage_key: row.storage_key,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The scope's organization's file with `sha256`, or `None` when it has none, read `FOR KEY
/// SHARE`: in the caller's transaction the row cannot be deleted until it ends, so an upload
/// that reuses the file keeps it from cleanup (B7, B21). Outside a transaction the lock ends
/// with the statement.
pub async fn find_by_hash(
    conn: &mut PgConnection,
    scope: OrgScope,
    sha256: Sha256,
) -> Result<Option<File>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, sha256, size, mime, storage_key,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM files
           WHERE org_id = $1 AND sha256 = $2
           FOR KEY SHARE"#,
        scope.org_id(),
        &sha256.0[..],
    )
    .fetch_optional(conn)
    .await?
    .map(File::try_from)
    .transpose()
}

/// Record the scope's organization's file `new`, whose bytes the caller has put at
/// [`storage_key`], stamped `now`. [`Error::HashTaken`] when the organization already has the
/// hash; in a transaction that error aborts it, so B7's caller rolls back and retries.
pub async fn insert(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    new: &NewFile,
    now: SystemTime,
) -> Result<File, Error> {
    if !MIME_CHARS.contains(&new.mime.chars().count()) || new.mime.contains('\0') {
        return Err(Error::MimeInvalid);
    }
    let size = i64::try_from(new.size).map_err(|_| Error::SizeOutOfRange)?;
    let key = storage_key(scope, new.sha256);
    sqlx::query_as!(
        Row,
        r#"INSERT INTO files (id, org_id, sha256, size, mime, storage_key, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6,
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond')
           RETURNING id, sha256, size, mime, storage_key,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        new_id(),
        scope.org_id(),
        &new.sha256.0[..],
        size,
        new.mime,
        key,
        epoch_micros(now)?,
    )
    .fetch_one(db)
    .await?
    .try_into()
}

/// How many bytes count against the organization's `storage` limit: the sum of `size` over its
/// `files` rows (B22). A file several courses share counts once, since it has one row.
pub async fn count_for_limit(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<i64, Error> {
    let bytes = sqlx::query_scalar!(
        r#"SELECT COALESCE(sum(size), 0)::bigint AS "bytes!" FROM files WHERE org_id = $1"#,
        scope.org_id(),
    )
    .fetch_one(db)
    .await?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_is_written_as_lowercase_hex() {
        let mut bytes = [0_u8; 32];
        bytes[0] = 0xab;
        bytes[31] = 0x0f;
        let hex = Sha256(bytes).hex();
        assert_eq!(hex.len(), 64);
        assert!(hex.starts_with("ab00"), "{hex}");
        assert!(hex.ends_with("000f"), "{hex}");
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a unit test stands in for a credential resolver"
    )]
    fn the_key_is_the_organization_and_the_hash_only() {
        let org = Uuid::nil();
        let key = storage_key(OrgScope::instance(org), Sha256([0x11; 32]));
        assert_eq!(
            key,
            format!(
                "orgs/00000000-0000-0000-0000-000000000000/files/{}",
                "11".repeat(32)
            )
        );
    }
}
