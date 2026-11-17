// SPDX-License-Identifier: AGPL-3.0-or-later
//! The course files repository (docs/specs/m1/06-files.md B9 to B12, Data).
//!
//! A course file places one of the organization's stored files ([`crate::files`]) in a folder of
//! a course under a name. A folder is a path, `/` or 1 to 8 segments, and exists while a file is
//! in it (Q4). [`Folder::parse`] and [`Name::parse`] hold B9's rules and store both
//! NFC-normalized; `(course_id, folder, name)` is unique and compared exactly.
//!
//! Every write here first locks the course row `FOR NO KEY UPDATE` and refuses an archived course
//! (spec 02 B5), as module and item writes do, so a delete and an item that would reference the
//! course file take turns. Deleting a course file stamps its file's `unreferenced_since`, which
//! `files.cleanup` reads (B21). Every query filters by `org_id = scope.org_id()`.

use std::time::SystemTime;

use sqlx::{PgConnection, PgExecutor};
use unicode_normalization::UnicodeNormalization as _;
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    files::{self, Sha256Digest},
    id::new_id,
    users::is_format_char,
};

/// The most segments a folder has (B9).
pub const MAX_FOLDER_DEPTH: usize = 8;

/// The longest folder segment, in characters (B9).
pub const MAX_SEGMENT_CHARS: usize = 100;

/// The longest name, in characters (B9).
pub const MAX_NAME_CHARS: usize = 255;

/// The unique constraint on a name in a folder of a course (PostgreSQL's default name for the
/// migration's `UNIQUE (course_id, folder, name)`).
const NAME_CONSTRAINT: &str = "course_files_course_id_folder_name_key";

/// The check on `folder`, which a folder rename breaks by passing [`MAX_FOLDER_DEPTH`].
const FOLDER_CHECK: &str = "course_files_folder_check";

/// Why a folder or a name breaks B9's rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// An empty segment or name, `.` or `..`, a `/`, `\`, NUL or other control character, a
    /// Unicode format character, or a folder that is not `/` and does not start with `/` or ends with it.
    #[error("not a valid folder or name")]
    Invalid,
    /// A segment over [`MAX_SEGMENT_CHARS`] or a name over [`MAX_NAME_CHARS`] characters.
    #[error("a segment or name is too long")]
    TooLong,
    /// A folder of more than [`MAX_FOLDER_DEPTH`] segments.
    #[error("a folder is more than {MAX_FOLDER_DEPTH} levels deep")]
    TooDeep,
}

/// One segment of B9: `max` characters at most once NFC-normalized, not empty, `.` or `..`,
/// with no `/`, `\`, control character (C0, DEL, C1; NUL is one) or Unicode format character
/// (general category Cf: bidirectional overrides and isolates, zero-width characters, the byte
/// order mark), which would let one name show as another.
fn segment(value: &str, max: usize) -> Result<(), PathError> {
    let chars = value.chars().count();
    if chars == 0 || value == "." || value == ".." {
        return Err(PathError::Invalid);
    }
    if value
        .chars()
        .any(|c| c == '/' || c == '\\' || c.is_control() || is_format_char(c))
    {
        return Err(PathError::Invalid);
    }
    if chars > max {
        return Err(PathError::TooLong);
    }
    Ok(())
}

/// A folder of a course: `/` or `/<segment>` repeated 1 to [`MAX_FOLDER_DEPTH`] times,
/// NFC-normalized.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Folder(String);

impl Folder {
    /// The course's top folder, `/`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// `value` NFC-normalized, if it keeps B9's folder rules.
    pub fn parse(value: &str) -> Result<Self, PathError> {
        let value: String = value.nfc().collect();
        if value == "/" {
            return Ok(Self(value));
        }
        let rest = value.strip_prefix('/').ok_or(PathError::Invalid)?;
        let segments: Vec<&str> = rest.split('/').collect();
        for part in &segments {
            segment(part, MAX_SEGMENT_CHARS)?;
        }
        if segments.len() > MAX_FOLDER_DEPTH {
            return Err(PathError::TooDeep);
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    /// The path of the folder `name` directly inside this one.
    #[must_use]
    pub fn child(&self, name: &str) -> String {
        format!("{}{name}", self.below())
    }

    /// What every folder below this one starts with: `/` for the root, `<folder>/` otherwise.
    fn below(&self) -> String {
        if self.is_root() {
            self.0.clone()
        } else {
            format!("{}/", self.0)
        }
    }
}

/// A course file's name: one segment of at most [`MAX_NAME_CHARS`] characters, NFC-normalized.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Name(String);

impl Name {
    /// `value` NFC-normalized, if it keeps B9's name rules.
    pub fn parse(value: &str) -> Result<Self, PathError> {
        let value: String = value.nfc().collect();
        segment(&value, MAX_NAME_CHARS)?;
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A course file with what its stored file says about the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CourseFile {
    pub id: Uuid,
    pub course_id: Uuid,
    pub file_id: Uuid,
    pub folder: String,
    pub name: String,
    /// The file's length in bytes.
    pub size: u64,
    /// The sniffed type (B6).
    pub mime: String,
    pub sha256: Sha256Digest,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// One entry of a folder listing (B12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// A folder directly inside the listed one: its last segment.
    Folder {
        name: String,
    },
    File(CourseFile),
}

impl Entry {
    /// The sort key [`list_folder`] orders by and continues after.
    #[must_use]
    pub fn key(&self) -> ListKey {
        match self {
            Self::Folder { name } => (FOLDER_RANK, name.clone()),
            Self::File(file) => (FILE_RANK, file.name.clone()),
        }
    }
}

/// The sort key of a folder listing: folders (`0`) before files (`1`), then the name, compared
/// as bytes. A folder's name is unique among folders and a file's among files.
pub type ListKey = (i32, String);

const FOLDER_RANK: i32 = 0;
const FILE_RANK: i32 = 1;

/// Why a course files repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    CourseNotFound,
    /// No course file with that id in the scope's organization.
    #[error("the course file does not exist")]
    NotFound,
    /// No course file is in the folder or below it.
    #[error("the folder does not exist")]
    FolderNotFound,
    /// The course is archived and takes no content write (spec 02 B5).
    #[error("the course is archived")]
    CourseArchived,
    /// Another course file of the course has the folder and name.
    #[error("the folder already holds a file with this name")]
    NameTaken,
    /// A `file` item references the course file (B11).
    #[error("the course file is in use")]
    InUse,
    /// A folder rename would put a file more than [`MAX_FOLDER_DEPTH`] levels deep.
    #[error("the folder would be more than {MAX_FOLDER_DEPTH} levels deep")]
    TooDeep,
    /// The top folder cannot be renamed, and nothing can be renamed to it.
    #[error("the top folder cannot be renamed")]
    RootFolder,
    #[error(transparent)]
    File(#[from] files::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some(NAME_CONSTRAINT) => Self::NameTaken,
            Some(FOLDER_CHECK) => Self::TooDeep,
            _ => Self::Database(error),
        }
    }
}

/// A `course_files` row joined to its file, as the queries select it.
struct Row {
    id: Uuid,
    course_id: Uuid,
    file_id: Uuid,
    folder: String,
    name: String,
    size: i64,
    mime: String,
    sha256: Vec<u8>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for CourseFile {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let stored = |column| files::Error::Stored {
            id: row.file_id,
            column,
        };
        Ok(Self {
            size: u64::try_from(row.size).map_err(|_| stored("size"))?,
            sha256: files::Sha256Digest::from_stored(&row.sha256)
                .ok_or_else(|| stored("sha256"))?,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            id: row.id,
            course_id: row.course_id,
            file_id: row.file_id,
            folder: row.folder,
            name: row.name,
            mime: row.mime,
        })
    }
}

/// Lock the course `id` `FOR NO KEY UPDATE` in the transaction `conn` belongs to:
/// [`Error::CourseNotFound`] when the scope's organization has no such course,
/// [`Error::CourseArchived`] when it is archived.
async fn lock_course(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    let status = sqlx::query_scalar!(
        "SELECT status FROM courses WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::CourseNotFound)?;
    if status == "archived" {
        return Err(Error::CourseArchived);
    }
    Ok(())
}

/// Place the scope's organization's file `file_id` in `folder` of the course `course_id` as
/// `name`, stamped `now`. In the caller's transaction: the course is locked until it ends.
pub async fn create(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    file_id: Uuid,
    (folder, name): (&Folder, &Name),
    now: SystemTime,
) -> Result<CourseFile, Error> {
    lock_course(conn, scope, course_id).await?;
    let id = new_id();
    sqlx::query!(
        r#"INSERT INTO course_files (id, org_id, course_id, file_id, folder, name, created_at,
                                     updated_at)
           VALUES ($1, $2, $3, $4, $5, $6,
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond')"#,
        id,
        scope.org_id(),
        course_id,
        file_id,
        folder.as_str(),
        name.as_str(),
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?;
    get(conn, scope, id).await?.ok_or(Error::NotFound)
}

/// The course file `id` of the scope's organization, or `None`.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<CourseFile>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT cf.id, cf.course_id, cf.file_id, cf.folder, cf.name, f.size, f.mime, f.sha256,
                  (extract(epoch FROM cf.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM cf.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM course_files cf JOIN files f ON f.org_id = cf.org_id AND f.id = cf.file_id
           WHERE cf.org_id = $1 AND cf.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(CourseFile::try_from)
    .transpose()
}

/// The course of the course file `id` of the scope's organization, or `None`: what
/// `CourseFileTarget` checks grants at.
pub async fn course_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    let course = sqlx::query_scalar!(
        "SELECT course_id FROM course_files WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(course)
}

/// Whether `folder` of the course `course_id` already holds a file named `name` (B4).
pub async fn name_taken(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
    (folder, name): (&Folder, &Name),
) -> Result<bool, Error> {
    let taken = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM course_files
               WHERE org_id = $1 AND course_id = $2 AND folder = $3 AND name = $4) AS "taken!""#,
        scope.org_id(),
        course_id,
        folder.as_str(),
        name.as_str(),
    )
    .fetch_one(db)
    .await?;
    Ok(taken)
}

/// What [`list_folder`] reads per entry: a folder has only `kind` and `name`.
struct EntryRow {
    kind: i32,
    name: String,
    id: Option<Uuid>,
    course_id: Option<Uuid>,
    file_id: Option<Uuid>,
    folder: Option<String>,
    size: Option<i64>,
    mime: Option<String>,
    sha256: Option<Vec<u8>>,
    created_at_us: Option<i64>,
    updated_at_us: Option<i64>,
}

impl TryFrom<EntryRow> for Entry {
    type Error = Error;

    fn try_from(row: EntryRow) -> Result<Self, Error> {
        if row.kind == FOLDER_RANK {
            return Ok(Self::Folder { name: row.name });
        }
        let id = row.id.unwrap_or_default();
        let missing = || files::Error::Stored {
            id,
            column: "course file",
        };
        let file = Row {
            id,
            course_id: row.course_id.ok_or_else(missing)?,
            file_id: row.file_id.ok_or_else(missing)?,
            folder: row.folder.ok_or_else(missing)?,
            name: row.name,
            size: row.size.ok_or_else(missing)?,
            mime: row.mime.ok_or_else(missing)?,
            sha256: row.sha256.ok_or_else(missing)?,
            created_at_us: row.created_at_us.ok_or_else(missing)?,
            updated_at_us: row.updated_at_us.ok_or_else(missing)?,
        };
        Ok(Self::File(file.try_into()?))
    }
}

/// Up to `limit` entries directly in `folder` of the course `course_id` after `after`: the
/// folders below it that hold files, by name, then its files, by name, names compared as bytes
/// (B12). The caller checks the course exists.
pub async fn list_folder(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
    folder: &Folder,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Entry>, Error> {
    let (after_kind, after_name) = match after {
        Some((kind, name)) => (Some(*kind), Some(name.as_str())),
        None => (None, None),
    };
    let rows = sqlx::query_as!(
        EntryRow,
        r#"WITH entries AS (
               SELECT DISTINCT 0 AS kind,
                      split_part(substr(folder, char_length($4) + 1), '/', 1) AS name,
                      NULL::uuid AS id
               FROM course_files
               WHERE org_id = $1 AND course_id = $2 AND starts_with(folder, $4) AND folder <> $3
               UNION ALL
               SELECT 1, name, id
               FROM course_files
               WHERE org_id = $1 AND course_id = $2 AND folder = $3
           )
           SELECT e.kind AS "kind!", e.name AS "name!", e.id, cf.course_id AS "course_id?",
                  cf.file_id AS "file_id?", cf.folder AS "folder?", f.size AS "size?",
                  f.mime AS "mime?", f.sha256 AS "sha256?",
                  (extract(epoch FROM cf.created_at) * 1000000)::bigint AS "created_at_us?",
                  (extract(epoch FROM cf.updated_at) * 1000000)::bigint AS "updated_at_us?"
           FROM entries e
           LEFT JOIN course_files cf ON cf.org_id = $1 AND cf.id = e.id
           LEFT JOIN files f ON f.org_id = cf.org_id AND f.id = cf.file_id
           WHERE $5::int IS NULL
              OR e.kind > $5
              OR (e.kind = $5 AND e.name COLLATE "C" > $6::text COLLATE "C")
           ORDER BY e.kind, e.name COLLATE "C"
           LIMIT $7"#,
        scope.org_id(),
        course_id,
        folder.as_str(),
        folder.below(),
        after_kind,
        after_name,
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Entry::try_from).collect()
}

/// Move or rename the course file `id` within its course: `None` leaves the folder or the name as
/// it is, and a change to the current values changes nothing, `updated_at` included.
pub async fn update(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    (folder, name): (Option<&Folder>, Option<&Name>),
    now: SystemTime,
) -> Result<CourseFile, Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::NotFound)?;
    lock_course(conn, scope, course_id).await?;
    sqlx::query!(
        r#"UPDATE course_files
           SET folder = COALESCE($3, folder),
               name = COALESCE($4, name),
               updated_at = CASE
                   WHEN (COALESCE($3, folder), COALESCE($4, name)) IS DISTINCT FROM (folder, name)
                   THEN timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
                   ELSE updated_at END
           WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
        folder.map(Folder::as_str),
        name.map(Name::as_str),
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?;
    get(conn, scope, id).await?.ok_or(Error::NotFound)
}

/// Rename the folder `from` of the course `course_id` to `to`, with every folder below it, in one
/// statement (B10): how many course files moved. [`Error::FolderNotFound`] when no file is in
/// `from` or below it, [`Error::NameTaken`] when a moved file meets one already at its new path
/// (nothing moves), [`Error::TooDeep`] when a moved file would be more than
/// [`MAX_FOLDER_DEPTH`] levels deep.
pub async fn rename_folder(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    (from, to): (&Folder, &Folder),
    now: SystemTime,
) -> Result<u64, Error> {
    if from.is_root() || to.is_root() {
        return Err(Error::RootFolder);
    }
    lock_course(conn, scope, course_id).await?;
    let moved = sqlx::query!(
        r#"UPDATE course_files
           SET folder = $5 || substr(folder, char_length($3) + 1),
               updated_at = CASE WHEN $3 = $5 THEN updated_at
                   ELSE timestamptz 'epoch' + $6::bigint * interval '1 microsecond' END
           WHERE org_id = $1 AND course_id = $2 AND (folder = $3 OR starts_with(folder, $4))"#,
        scope.org_id(),
        course_id,
        from.as_str(),
        from.below(),
        to.as_str(),
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?
    .rows_affected();
    if moved == 0 {
        return Err(Error::FolderNotFound);
    }
    Ok(moved)
}

/// Whether a `file` item references the course file `id` (B11). The course's cover joins this
/// check with spec 06 I4.
pub async fn referenced(db: impl PgExecutor<'_>, scope: OrgScope, id: Uuid) -> Result<bool, Error> {
    let referenced = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM module_items
               WHERE org_id = $1 AND kind = 'file' AND ref_id = $2) AS "referenced!""#,
        scope.org_id(),
        id,
    )
    .fetch_one(db)
    .await?;
    Ok(referenced)
}

/// Delete the course file `id`, unless something references it ([`Error::InUse`], B11), and
/// stamp its file's `unreferenced_since` with `now` (B21). The `files` row and its object stay
/// for `files.cleanup`.
pub async fn delete(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
) -> Result<(), Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::NotFound)?;
    lock_course(conn, scope, course_id).await?;
    if referenced(&mut *conn, scope, id).await? {
        return Err(Error::InUse);
    }
    let file_id = sqlx::query_scalar!(
        "DELETE FROM course_files WHERE org_id = $1 AND id = $2 RETURNING file_id",
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::NotFound)?;
    sqlx::query!(
        r#"UPDATE files
           SET unreferenced_since = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        file_id,
        epoch_micros(now)?,
    )
    .execute(conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_keep_the_segment_rules() {
        for ok in ["/", "/a", "/handouts/week-1", "/a/b/c/d/e/f/g/h", "/ é .x"] {
            assert_eq!(Folder::parse(ok).map(|f| f.0), Ok(ok.to_owned()), "{ok}");
        }
        let long = format!("/{}", "x".repeat(MAX_SEGMENT_CHARS + 1));
        let cases = [
            ("", PathError::Invalid),
            ("a", PathError::Invalid),
            ("/a/", PathError::Invalid),
            ("//", PathError::Invalid),
            ("/a//b", PathError::Invalid),
            ("/..", PathError::Invalid),
            ("/a/./b", PathError::Invalid),
            ("/a\\b", PathError::Invalid),
            ("/a\0", PathError::Invalid),
            ("/a\u{7f}", PathError::Invalid),
            ("/a\u{85}", PathError::Invalid),
            ("/a\tb", PathError::Invalid),
            ("/rtl\u{202E}gpj.exe", PathError::Invalid),
            ("/a/zero\u{200B}width", PathError::Invalid),
            ("/a/b/c/d/e/f/g/h/i", PathError::TooDeep),
            (long.as_str(), PathError::TooLong),
        ];
        for (bad, error) in cases {
            assert_eq!(Folder::parse(bad), Err(error), "{bad:?}");
        }
        let longest = format!("/{}", "x".repeat(MAX_SEGMENT_CHARS));
        assert!(Folder::parse(&longest).is_ok());
    }

    #[test]
    fn names_are_one_segment_of_up_to_255_characters() {
        assert!(Name::parse(&"n".repeat(MAX_NAME_CHARS)).is_ok());
        assert_eq!(
            Name::parse(&"n".repeat(MAX_NAME_CHARS + 1)),
            Err(PathError::TooLong)
        );
        for bad in [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "a\nb",
            "a\u{9b}",
            "invoice\u{202E}fdp.exe",
            "a\u{2066}b\u{2069}",
            "zero\u{200B}width",
            "\u{FEFF}bom",
            "soft\u{AD}hyphen",
            "tag\u{E0041}",
        ] {
            assert_eq!(Name::parse(bad), Err(PathError::Invalid), "{bad:?}");
        }
        assert!(Name::parse("..hidden").is_ok());
    }

    #[test]
    fn both_are_stored_nfc_normalized() {
        let decomposed = "Cafe\u{301}.pdf";
        assert_eq!(Name::parse(decomposed).unwrap().as_str(), "Caf\u{e9}.pdf");
        assert_eq!(
            Folder::parse("/Re\u{301}sume\u{301}").unwrap().as_str(),
            "/R\u{e9}sum\u{e9}"
        );
        // A character whose NFC form is longer counts at its normalized length.
        let expands = "\u{0958}".repeat(MAX_NAME_CHARS);
        assert_eq!(Name::parse(&expands), Err(PathError::TooLong));
    }

    #[test]
    fn below_is_the_prefix_of_the_folders_inside() {
        assert_eq!(Folder::root().below(), "/");
        assert_eq!(Folder::parse("/a/b").unwrap().below(), "/a/b/");
        assert_eq!(Folder::root().child("a"), "/a");
        assert_eq!(Folder::parse("/a").unwrap().child("b"), "/a/b");
    }
}
