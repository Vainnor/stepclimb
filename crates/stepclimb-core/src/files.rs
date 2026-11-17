// SPDX-License-Identifier: AGPL-3.0-or-later
//! The stored files repository (docs/specs/m1/06-files.md B2, B7, B22, Data).
//!
//! A `files` row is one distinct file of one organization: the same bytes are stored once per
//! organization (`UNIQUE (org_id, sha256)`, Q9), so an upload whose hash the organization
//! already has adds a `course_files` row and no bytes. The row's object key is derived here from
//! the scope and the hash ([`storage_key`]), never from anything a client sent (B2). Every query
//! filters by `org_id = scope.org_id()`.
//!
//! The hash and the size are the server's own (B5): a [`NewFile`] can only be built from a
//! [`Hashed`], which only a [`Sha256Hasher`] makes, over the bytes it was fed. Nothing a client
//! sends can name a hash. The upload route (spec 06 I2) feeds the hasher every chunk of the body
//! as it streams it to storage, and builds the [`NewFile`] from what the hasher returns.
//!
//! ```compile_fail,E0423
//! // A digest cannot be written from bytes a caller chose.
//! let digest = stepclimb_core::files::Sha256Digest([0; 32]);
//! ```
//!
//! ```compile_fail,E0063
//! // Nor can a hash and size be written outside the hasher.
//! let hashed = stepclimb_core::files::Hashed { size: 0 };
//! ```
//!
//! This module holds rows only; the bytes are the server's storage layer's. Which upload may
//! add a row, and the order of object copy, insert and commit (B7), are the upload route's; the
//! hash lock ([`lock_hash`]) is how uploads and `files.cleanup` take turns on one hash.

use std::fmt;
use std::time::SystemTime;

use sha2::Digest as _;
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

/// The SHA-256 digest of a file's bytes, computed by the server. Outside this crate the only ways
/// to get one are [`Sha256Hasher::finish`], reading a stored row, and [`digest_of_key`] on the
/// key of an object the server stored. None of them makes a [`Hashed`], so no caller can record
/// a file under a hash of its choosing.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    /// The digest's 32 bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The digest a row stores, or `None` when it is not 32 bytes.
    pub(crate) fn from_stored(bytes: &[u8]) -> Option<Self> {
        <[u8; 32]>::try_from(bytes).ok().map(Self)
    }

    /// The digest as 64 lowercase hexadecimal digits, as object keys and `ETag`s write it.
    #[must_use]
    pub fn hex(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

impl fmt::Debug for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sha256Digest({self})")
    }
}

/// SHA-256 and the byte count over bytes fed to it in order, such as the chunks of an upload
/// body as they are streamed to storage (B5).
#[derive(Clone, Debug, Default)]
pub struct Sha256Hasher {
    digest: sha2::Sha256,
    bytes: u64,
}

impl Sha256Hasher {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed the next `chunk` of the file.
    pub fn update(&mut self, chunk: &[u8]) {
        self.digest.update(chunk);
        // A usize always fits a u64 on the targets the server builds for; saturating keeps the
        // count from wrapping, and no stored file nears 2^64 bytes.
        self.bytes = self
            .bytes
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
    }

    /// How many bytes have been fed so far.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// The digest and size of everything fed.
    #[must_use]
    pub fn finish(self) -> Hashed {
        Hashed {
            sha256: Sha256Digest(self.digest.finalize().into()),
            size: self.bytes,
        }
    }
}

/// What a [`Sha256Hasher`] computed over a file's bytes: its digest and its length. Only the
/// hasher makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hashed {
    sha256: Sha256Digest,
    size: u64,
}

impl Hashed {
    #[must_use]
    pub fn sha256(&self) -> Sha256Digest {
        self.sha256
    }

    /// The length in bytes.
    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }
}

/// The object key of the scope's organization's file with `sha256`, relative to the store's
/// prefix: `orgs/<org_id>/files/<sha256 hex>` (B2). Built from the scope and the hash only, so
/// no client string reaches a storage path.
#[must_use]
pub fn storage_key(scope: OrgScope, sha256: Sha256Digest) -> String {
    format!("orgs/{}/files/{sha256}", scope.org_id())
}

/// The digest in `key`, the key of the scope's organization's file object as [`storage_key`]
/// writes it (`orgs/<org_id>/files/<64 lowercase hex digits>`), or `None` for any other key. For
/// `files.cleanup`, which lists the objects in storage and looks for those without a row (B21).
#[must_use]
pub fn digest_of_key(scope: OrgScope, key: &str) -> Option<Sha256Digest> {
    let hex = key.strip_prefix(&format!("orgs/{}/files/", scope.org_id()))?;
    let lower_hex = |b: &u8| b.is_ascii_digit() || (b'a'..=b'f').contains(b);
    if hex.len() != 64 || !hex.as_bytes().iter().all(lower_hex) {
        return None;
    }
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(hex.as_bytes().chunks(2)) {
        // Two lowercase hexadecimal digits, checked above.
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(Sha256Digest(digest))
}

/// The first key of the advisory lock on one hash of one organization's files: the ASCII of
/// `file`. PostgreSQL keeps two-`int4` keys apart from single-`bigint` ones, and this class keeps
/// it apart from the account tree's lock.
const HASH_LOCK_CLASS: i32 = 0x6669_6c65;

/// The second key of the hash lock: the first four bytes of SHA-256 over the organization's id
/// and the digest. Two hashes that share it only wait for each other.
fn hash_lock_key(scope: OrgScope, sha256: Sha256Digest) -> i32 {
    let mut key = sha2::Sha256::new();
    key.update(scope.org_id().as_bytes());
    key.update(sha256.0);
    let key = key.finalize();
    i32::from_be_bytes([key[0], key[1], key[2], key[3]])
}

/// Take the transaction-scoped advisory lock on the scope's organization's `sha256`, held until
/// the transaction `conn` is in ends. An upload holds it from before it looks the hash up until
/// it commits the row (B7), and `files.cleanup` holds [`lock_hash_session`] while it deletes the
/// row and then the object (B21), so an object is never deleted while an upload of the same bytes
/// is between copying it and committing its row.
pub async fn lock_hash(
    conn: &mut PgConnection,
    scope: OrgScope,
    sha256: Sha256Digest,
) -> Result<(), Error> {
    sqlx::query!(
        "SELECT pg_advisory_xact_lock($1, $2)",
        HASH_LOCK_CLASS,
        hash_lock_key(scope, sha256),
    )
    .fetch_one(conn)
    .await?;
    Ok(())
}

/// [`lock_hash`]'s lock, held by the session `conn` until [`unlock_hash_session`], so it spans a
/// commit and the object delete after it. A connection that drops releases it.
pub async fn lock_hash_session(
    conn: &mut PgConnection,
    scope: OrgScope,
    sha256: Sha256Digest,
) -> Result<(), Error> {
    sqlx::query!(
        "SELECT pg_advisory_lock($1, $2)",
        HASH_LOCK_CLASS,
        hash_lock_key(scope, sha256),
    )
    .fetch_one(conn)
    .await?;
    Ok(())
}

/// Release [`lock_hash_session`]'s lock. `false` when the session did not hold it.
pub async fn unlock_hash_session(
    conn: &mut PgConnection,
    scope: OrgScope,
    sha256: Sha256Digest,
) -> Result<bool, Error> {
    let released = sqlx::query_scalar!(
        r#"SELECT pg_advisory_unlock($1, $2) AS "released!""#,
        HASH_LOCK_CLASS,
        hash_lock_key(scope, sha256),
    )
    .fetch_one(conn)
    .await?;
    Ok(released)
}

/// A stored file as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File {
    pub id: Uuid,
    pub sha256: Sha256Digest,
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

/// A file to record once its bytes are at [`storage_key`]. Its hash and size are a [`Hashed`]
/// the server computed over those bytes (B5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewFile {
    pub hashed: Hashed,
    /// 3 to 100 characters.
    pub mime: String,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The organization already has a file with this hash. An upload holds [`lock_hash`] from
    /// before its [`find_by_hash`] to its commit, so it finds the row instead of meeting this.
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
            sha256: Sha256Digest(sha256),
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
    sha256: Sha256Digest,
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
/// hash, which aborts the caller's transaction.
pub async fn insert(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    new: &NewFile,
    now: SystemTime,
) -> Result<File, Error> {
    if !MIME_CHARS.contains(&new.mime.chars().count()) || new.mime.contains('\0') {
        return Err(Error::MimeInvalid);
    }
    let size = i64::try_from(new.hashed.size).map_err(|_| Error::SizeOutOfRange)?;
    let key = storage_key(scope, new.hashed.sha256);
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
        &new.hashed.sha256.0[..],
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

/// Whether the scope's organization has a file of exactly `size` bytes: an upload of that length
/// may be one of its files again, which adds nothing to the `storage` limit (B4, B7).
pub async fn any_of_size(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    size: u64,
) -> Result<bool, Error> {
    let Ok(size) = i64::try_from(size) else {
        return Ok(false);
    };
    let found = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM files WHERE org_id = $1 AND size = $2) AS "found!""#,
        scope.org_id(),
        size,
    )
    .fetch_one(db)
    .await?;
    Ok(found)
}

/// Whether the scope's organization has a file with `sha256`. Read under [`lock_hash`] or
/// [`lock_hash_session`], the answer holds until the lock is released.
pub async fn exists(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    sha256: Sha256Digest,
) -> Result<bool, Error> {
    let found = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM files WHERE org_id = $1 AND sha256 = $2) AS "found!""#,
        scope.org_id(),
        &sha256.0[..],
    )
    .fetch_one(db)
    .await?;
    Ok(found)
}

/// The most rows [`unreferenced`] returns at once.
pub const UNREFERENCED_BATCH: i64 = 1000;

/// The ids and hashes of up to [`UNREFERENCED_BATCH`] of the scope's organization's files that
/// no course file references and whose last reference went at or before `cutoff` (B21): stamped
/// `unreferenced_since`, or, never having lost one, created by then. Spec 08's videos and
/// captions add their references to this query.
pub async fn unreferenced(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    cutoff: SystemTime,
) -> Result<Vec<(Uuid, Sha256Digest)>, Error> {
    let rows = sqlx::query!(
        r#"SELECT f.id, f.sha256
           FROM files f
           WHERE f.org_id = $1
             AND COALESCE(f.unreferenced_since, f.created_at)
                 <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'
             AND NOT EXISTS (
                 SELECT 1 FROM course_files cf WHERE cf.org_id = f.org_id AND cf.file_id = f.id)
           ORDER BY f.id
           LIMIT $3"#,
        scope.org_id(),
        epoch_micros(cutoff)?,
        UNREFERENCED_BATCH,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|row| {
            let sha256 =
                <[u8; 32]>::try_from(row.sha256.as_slice()).map_err(|_| Error::Stored {
                    id: row.id,
                    column: "sha256",
                })?;
            Ok((row.id, Sha256Digest(sha256)))
        })
        .collect()
}

/// Delete the scope's organization's file `id` if it is still unreferenced as [`unreferenced`]
/// reads it at `cutoff`, checked again with the row locked `FOR UPDATE`: an upload or a copy
/// that is adding a reference holds `FOR KEY SHARE` on it, so this waits for it and then sees
/// its course file. Returns the deleted row, whose object the caller deletes after the commit,
/// or `None` when the row is gone or referenced again.
pub async fn delete(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    cutoff: SystemTime,
) -> Result<Option<File>, Error> {
    let locked = sqlx::query_scalar!(
        "SELECT id FROM files WHERE org_id = $1 AND id = $2 FOR UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *conn)
    .await?;
    if locked.is_none() {
        return Ok(None);
    }
    sqlx::query_as!(
        Row,
        r#"DELETE FROM files f
           WHERE f.org_id = $1 AND f.id = $2
             AND COALESCE(f.unreferenced_since, f.created_at)
                 <= timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
             AND NOT EXISTS (
                 SELECT 1 FROM course_files cf WHERE cf.org_id = f.org_id AND cf.file_id = f.id)
           RETURNING id, sha256, size, mime, storage_key,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        id,
        epoch_micros(cutoff)?,
    )
    .fetch_optional(conn)
    .await?
    .map(File::try_from)
    .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SHA-256 of "abc" (FIPS 180-2, appendix B.1).
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn hashed(chunks: &[&[u8]]) -> Hashed {
        let mut hasher = Sha256Hasher::new();
        for chunk in chunks {
            hasher.update(chunk);
        }
        hasher.finish()
    }

    #[test]
    fn the_hasher_digests_and_counts_the_bytes_fed_in_chunks() {
        let whole = hashed(&[b"abc"]);
        assert_eq!(whole.sha256().hex(), ABC);
        assert_eq!(whole.size(), 3);
        assert_eq!(hashed(&[b"a", b"", b"bc"]), whole);
        let empty = hashed(&[]);
        assert_eq!(
            empty.sha256().to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(empty.size(), 0);
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a unit test stands in for a credential resolver"
    )]
    fn a_key_gives_back_its_digest_only_in_its_own_organization() {
        let (org, other) = (Uuid::nil(), Uuid::max());
        let sha256 = hashed(&[b"abc"]).sha256();
        let key = storage_key(OrgScope::instance(org), sha256);
        assert_eq!(digest_of_key(OrgScope::instance(org), &key), Some(sha256));
        assert_eq!(digest_of_key(OrgScope::instance(other), &key), None);
        let prefix = format!("orgs/{org}/files/");
        for bad in [
            format!("{prefix}{}", &ABC[..63]),
            format!("{prefix}{ABC}0"),
            format!("{prefix}{}", ABC.to_uppercase()),
            format!("{prefix}{}g", &ABC[..63]),
            format!("orgs/{org}/uploads/{ABC}"),
        ] {
            assert_eq!(digest_of_key(OrgScope::instance(org), &bad), None, "{bad}");
        }
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a unit test stands in for a credential resolver"
    )]
    fn the_hash_lock_key_depends_on_the_organization_and_the_hash() {
        let (a, b) = (hashed(&[b"a"]).sha256(), hashed(&[b"b"]).sha256());
        let org = OrgScope::instance(Uuid::nil());
        let other = OrgScope::instance(Uuid::max());
        assert_eq!(hash_lock_key(org, a), hash_lock_key(org, a));
        assert_ne!(hash_lock_key(org, a), hash_lock_key(org, b));
        assert_ne!(hash_lock_key(org, a), hash_lock_key(other, a));
    }

    #[test]
    #[expect(
        clippy::disallowed_methods,
        reason = "a unit test stands in for a credential resolver"
    )]
    fn the_key_is_the_organization_and_the_hash_only() {
        let org = Uuid::nil();
        let key = storage_key(OrgScope::instance(org), hashed(&[b"abc"]).sha256());
        assert_eq!(
            key,
            format!("orgs/00000000-0000-0000-0000-000000000000/files/{ABC}")
        );
    }
}
