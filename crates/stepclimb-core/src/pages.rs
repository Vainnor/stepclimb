// SPDX-License-Identifier: AGPL-3.0-or-later
//! The pages repository (docs/specs/m1/07-pages-and-the-editor.md B1 to B3, B5, B6, B10, B10a,
//! Data).
//!
//! A page belongs to one course and always has a current version. Every save writes a new,
//! immutable version (Q1): [`save`] and [`restore`] take the base version the author started
//! from and refuse with [`Error::VersionConflict`] when another save made a newer one current,
//! so no save silently overwrites another author's. A body equal to the current one writes
//! nothing.
//!
//! Every write first locks the course row `FOR NO KEY UPDATE` and refuses an archived course
//! (spec 02 B5), as module, item and course file writes do, then locks the page row; so two saves
//! of one page take turns, and a course file cannot be deleted between the image check and the
//! version that names it. A version's images are checked against the course's files (B10, B10a)
//! and listed in `page_version_files`. Every query filters by `org_id = scope.org_id()`.

use std::{
    collections::{HashMap, HashSet},
    time::SystemTime,
};

use serde_json::Value;
use sqlx::{Acquire as _, PgConnection, PgExecutor};
use unicode_normalization::UnicodeNormalization as _;
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name,
    page_doc::{self, IMAGE_TYPES, PageDoc},
};

/// The longest slug, in characters (B1).
pub const MAX_SLUG_CHARS: usize = 100;

/// The unique constraint on a slug in a course (PostgreSQL's default name for the migration's
/// `UNIQUE (course_id, slug)`).
const SLUG_CONSTRAINT: &str = "pages_course_id_slug_key";

/// A page's slug: lowercase ASCII letters and digits in runs joined by single hyphens, 1 to
/// [`MAX_SLUG_CHARS`] characters (B1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slug(String);

impl Slug {
    /// `value` if it is a slug as given; nothing is normalized.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let shaped = !value.is_empty()
            && value.len() <= MAX_SLUG_CHARS
            && value.split('-').all(|run| {
                !run.is_empty()
                    && run
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            });
        shaped.then(|| Self(value.to_owned()))
    }

    /// The slug of `title`: its compatibility decomposition (so `é` gives `e`) lowercased, ASCII
    /// letters and digits kept, other characters outside ASCII dropped, and every run of other
    /// ASCII characters made one hyphen; cut to [`MAX_SLUG_CHARS`]. `None` when nothing is left,
    /// as for a title in a script without ASCII letters.
    #[must_use]
    pub fn derive(title: &str) -> Option<Self> {
        let mut slug = String::new();
        let mut gap = false;
        for c in title.nfkd() {
            if c.is_ascii_alphanumeric() {
                if gap && !slug.is_empty() {
                    slug.push('-');
                }
                gap = false;
                slug.push(c.to_ascii_lowercase());
            } else if c.is_ascii() {
                gap = true;
            }
        }
        slug.truncate(MAX_SLUG_CHARS);
        Self::parse(slug.trim_end_matches('-'))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A version without its body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionSummary {
    pub id: Uuid,
    /// 1, 2, ... per page.
    pub number: i32,
    pub author_id: Uuid,
    pub created_at: SystemTime,
}

/// A version with its body, as stored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub summary: VersionSummary,
    pub page_id: Uuid,
    pub body: Value,
}

/// A page with its current version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Page {
    pub id: Uuid,
    pub course_id: Uuid,
    pub title: String,
    pub slug: String,
    pub current: VersionSummary,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A page to create.
#[derive(Clone, Debug, PartialEq)]
pub struct NewPage {
    /// Trimmed; 1 to 200 characters.
    pub title: String,
    /// `None` derives the slug from the title.
    pub slug: Option<Slug>,
    /// Version 1's body.
    pub body: PageDoc,
}

/// A change to a page; `None` leaves a member as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PagePatch {
    pub title: Option<String>,
    pub slug: Option<Slug>,
}

/// Why a pages repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    CourseNotFound,
    /// No page with that id in the scope's organization.
    #[error("the page does not exist")]
    NotFound,
    /// No version with that id of the page.
    #[error("the page version does not exist")]
    VersionNotFound,
    /// The course is archived and takes no content write (spec 02 B5).
    #[error("the course is archived")]
    CourseArchived,
    /// Another page of the course has the slug.
    #[error("the course already has a page with this slug")]
    SlugTaken,
    /// No slug was given and none can be derived from the title.
    #[error("no slug can be derived from the title")]
    SlugUnderivable,
    #[error("the title is empty")]
    TitleEmpty,
    #[error("the title is longer than 200 characters")]
    TitleTooLong,
    #[error("the title contains NUL")]
    TitleContainsNul,
    /// An item references the page (B6).
    #[error("the page is in use")]
    InUse,
    /// The save's base version is not the current one (B3); nothing was written.
    #[error("the page has a newer version")]
    VersionConflict(VersionSummary),
    /// An image names no course file of the page's course (B10).
    #[error("an image names no course file of the course")]
    FileNotFound,
    /// An image names a course file whose stored type is not an image type (B10a); `path` is
    /// the image's `course_file_id` attribute in the body.
    #[error("an image names a file that is not an image")]
    ImageType { path: Vec<String> },
    /// A stored body no longer parses.
    #[error(transparent)]
    Body(#[from] page_doc::Invalid),
    /// A stored value the repository cannot read.
    #[error("the stored {column} of page {id} is not valid")]
    Stored { id: Uuid, column: &'static str },
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        match error.as_database_error().and_then(|e| e.constraint()) {
            Some(SLUG_CONSTRAINT) => Self::SlugTaken,
            _ => Self::Database(error),
        }
    }
}

/// `title` trimmed, or why it is refused.
fn title(value: &str) -> Result<&str, Error> {
    name::valid(value).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::TitleEmpty,
        name::Invalid::TooLong => Error::TitleTooLong,
        name::Invalid::ContainsNul => Error::TitleContainsNul,
    })
}

/// A page row joined to its current version, as the queries select it.
struct Row {
    id: Uuid,
    course_id: Uuid,
    title: String,
    slug: String,
    version_id: Uuid,
    number: i32,
    author_id: Uuid,
    version_created_us: i64,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Page {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            current: VersionSummary {
                id: row.version_id,
                number: row.number,
                author_id: row.author_id,
                created_at: from_epoch_micros(row.version_created_us)?,
            },
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            id: row.id,
            course_id: row.course_id,
            title: row.title,
            slug: row.slug,
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

/// Lock the page `id`'s course as [`lock_course`] does, then the page row `FOR UPDATE`: the
/// page's course and current version id.
async fn lock_page(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<(Uuid, Uuid), Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::NotFound)?;
    lock_course(conn, scope, course_id).await?;
    let current = sqlx::query_scalar!(
        "SELECT current_version_id FROM pages WHERE org_id = $1 AND id = $2 FOR UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::NotFound)?;
    Ok((course_id, current))
}

/// Check every image of `body` against the course's files, in document order: the first that
/// is no course file of the course is [`Error::FileNotFound`], the first whose stored type is
/// not in [`IMAGE_TYPES`] is [`Error::ImageType`]. Returns the distinct course file ids.
async fn check_images(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    body: &PageDoc,
) -> Result<Vec<Uuid>, Error> {
    let images = body.images();
    if images.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = images.iter().map(|(id, _)| *id).collect();
    let found = sqlx::query!(
        r#"SELECT cf.id, f.mime
           FROM course_files cf JOIN files f ON f.org_id = cf.org_id AND f.id = cf.file_id
           WHERE cf.org_id = $1 AND cf.course_id = $2 AND cf.id = ANY($3)"#,
        scope.org_id(),
        course_id,
        &ids[..],
    )
    .fetch_all(conn)
    .await?;
    let mimes: HashMap<Uuid, String> = found.into_iter().map(|row| (row.id, row.mime)).collect();
    let mut distinct = Vec::new();
    let mut seen = HashSet::new();
    for (id, path) in images {
        let mime = mimes.get(&id).ok_or(Error::FileNotFound)?;
        if !IMAGE_TYPES.contains(&mime.as_str()) {
            return Err(Error::ImageType { path });
        }
        if seen.insert(id) {
            distinct.push(id);
        }
    }
    Ok(distinct)
}

/// Where a new version goes: its page and the page's course, its id and its number.
#[derive(Clone, Copy)]
struct VersionSlot {
    course_id: Uuid,
    page_id: Uuid,
    id: Uuid,
    number: i32,
}

/// Insert the version `slot` with `body`, by `author_id` at `now`, and list its images.
async fn insert_version(
    conn: &mut PgConnection,
    scope: OrgScope,
    slot: VersionSlot,
    body: &PageDoc,
    (images, author_id, now): (&[Uuid], Uuid, SystemTime),
) -> Result<(), Error> {
    let VersionSlot {
        course_id,
        page_id,
        id,
        number,
    } = slot;
    sqlx::query!(
        r#"INSERT INTO page_versions (id, org_id, course_id, page_id, number, body, author_id,
                                      created_at, updated_at)
           VALUES ($1, $2, $8, $3, $4, $5::text::jsonb, $6,
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond')"#,
        id,
        scope.org_id(),
        page_id,
        number,
        body.to_value().to_string(),
        author_id,
        epoch_micros(now)?,
        course_id,
    )
    .execute(&mut *conn)
    .await?;
    sqlx::query!(
        r#"INSERT INTO page_version_files (org_id, course_id, version_id, course_file_id,
                                           created_at, updated_at)
           SELECT $1, $5, $2, file_id,
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
           FROM unnest($3::uuid[]) AS file_id"#,
        scope.org_id(),
        id,
        images,
        epoch_micros(now)?,
        course_id,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// Create a page of the course `course_id` with version 1 of `new.body`, by `author_id` at
/// `now` (B1). The page and its version are written in one transaction (a savepoint inside the
/// caller's), which the deferred `pages_current_version` key needs; the course stays locked
/// until the caller's transaction ends.
pub async fn create(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    new: &NewPage,
    author_id: Uuid,
    now: SystemTime,
) -> Result<Page, Error> {
    let mut tx = conn.begin().await?;
    let page = insert_page(&mut tx, scope, course_id, new, (author_id, now)).await?;
    tx.commit().await?;
    Ok(page)
}

async fn insert_page(
    conn: &mut PgConnection,
    scope: OrgScope,
    course_id: Uuid,
    new: &NewPage,
    (author_id, now): (Uuid, SystemTime),
) -> Result<Page, Error> {
    let title = title(&new.title)?;
    let slug = match &new.slug {
        Some(slug) => slug.clone(),
        None => Slug::derive(title).ok_or(Error::SlugUnderivable)?,
    };
    lock_course(conn, scope, course_id).await?;
    let images = check_images(conn, scope, course_id, &new.body).await?;
    let (id, version_id) = (new_id(), new_id());
    sqlx::query!(
        r#"INSERT INTO pages (id, org_id, course_id, slug, title, current_version_id, created_at,
                              updated_at)
           VALUES ($1, $2, $3, $4, $5, $6,
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $7::bigint * interval '1 microsecond')"#,
        id,
        scope.org_id(),
        course_id,
        slug.as_str(),
        title,
        version_id,
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?;
    let author = (images.as_slice(), author_id, now);
    let slot = VersionSlot {
        course_id,
        page_id: id,
        id: version_id,
        number: 1,
    };
    insert_version(conn, scope, slot, &new.body, author).await?;
    get(conn, scope, id).await?.ok_or(Error::NotFound)
}

/// The page `id` of the scope's organization with its current version, or `None`.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Page>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT p.id, p.course_id, p.title, p.slug, v.id AS version_id, v.number, v.author_id,
                  (extract(epoch FROM v.created_at) * 1000000)::bigint AS "version_created_us!",
                  (extract(epoch FROM p.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM p.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM pages p JOIN page_versions v ON v.org_id = p.org_id AND v.id = p.current_version_id
           WHERE p.org_id = $1 AND p.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Page::try_from)
    .transpose()
}

/// The course of the page `id` of the scope's organization, or `None`: what `PageTarget`
/// checks grants at.
pub async fn course_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    let course = sqlx::query_scalar!(
        "SELECT course_id FROM pages WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    Ok(course)
}

/// Up to `limit` pages of the course `course_id` after the slug `after`, by slug (compared as
/// bytes). The caller checks the course exists.
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
    after: Option<&str>,
    limit: i64,
) -> Result<Vec<Page>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT p.id, p.course_id, p.title, p.slug, v.id AS version_id, v.number, v.author_id,
                  (extract(epoch FROM v.created_at) * 1000000)::bigint AS "version_created_us!",
                  (extract(epoch FROM p.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM p.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM pages p JOIN page_versions v ON v.org_id = p.org_id AND v.id = p.current_version_id
           WHERE p.org_id = $1 AND p.course_id = $2
             AND ($3::text IS NULL OR p.slug COLLATE "C" > $3::text COLLATE "C")
           ORDER BY p.slug COLLATE "C"
           LIMIT $4"#,
        scope.org_id(),
        course_id,
        after,
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Page::try_from).collect()
}

/// Change the page `id`'s title or slug (B6). A patch naming only current values changes
/// nothing, `updated_at` included.
pub async fn update(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    patch: &PagePatch,
    now: SystemTime,
) -> Result<Page, Error> {
    let title = patch.title.as_deref().map(title).transpose()?;
    lock_page(conn, scope, id).await?;
    sqlx::query!(
        r#"UPDATE pages
           SET title = COALESCE($3, title),
               slug = COALESCE($4, slug),
               updated_at = CASE
                   WHEN (COALESCE($3, title), COALESCE($4, slug)) IS DISTINCT FROM (title, slug)
                   THEN timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
                   ELSE updated_at END
           WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
        title,
        patch.slug.as_ref().map(Slug::as_str),
        epoch_micros(now)?,
    )
    .execute(&mut *conn)
    .await?;
    get(conn, scope, id).await?.ok_or(Error::NotFound)
}

/// What a save writes: a body, from the version the author started at.
#[derive(Clone, Copy, Debug)]
pub struct Save<'a> {
    pub body: &'a PageDoc,
    /// The version the author's body is based on; it must be the current one (B3).
    pub base_version_id: Uuid,
    pub author_id: Uuid,
}

/// Save `save.body` as the page `id`'s new current version at `now` (B3): refused with
/// [`Error::VersionConflict`], writing nothing, when `save.base_version_id` is not the current
/// version; a body equal to the current one writes nothing either. The version, its images and
/// the page's new current version are written in one transaction (a savepoint inside the
/// caller's).
pub async fn save(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    save: Save<'_>,
    now: SystemTime,
) -> Result<Page, Error> {
    let mut tx = conn.begin().await?;
    let page = save_in(&mut tx, scope, id, save, now).await?;
    tx.commit().await?;
    Ok(page)
}

async fn save_in(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    save: Save<'_>,
    now: SystemTime,
) -> Result<Page, Error> {
    let (course_id, current) = lock_page(conn, scope, id).await?;
    if save.base_version_id != current {
        let page = get(&mut *conn, scope, id).await?.ok_or(Error::NotFound)?;
        return Err(Error::VersionConflict(page.current));
    }
    let unchanged = sqlx::query_scalar!(
        r#"SELECT body = $3::text::jsonb AS "unchanged!" FROM page_versions
           WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        current,
        save.body.to_value().to_string(),
    )
    .fetch_one(&mut *conn)
    .await?;
    if !unchanged {
        let images = check_images(conn, scope, course_id, save.body).await?;
        let number = sqlx::query_scalar!(
            r#"SELECT max(number) + 1 AS "next!" FROM page_versions
               WHERE org_id = $1 AND page_id = $2"#,
            scope.org_id(),
            id,
        )
        .fetch_one(&mut *conn)
        .await?;
        let author = (images.as_slice(), save.author_id, now);
        let version = new_id();
        let slot = VersionSlot {
            course_id,
            page_id: id,
            id: version,
            number,
        };
        insert_version(conn, scope, slot, save.body, author).await?;
        sqlx::query!(
            r#"UPDATE pages
               SET current_version_id = $3,
                   updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
               WHERE org_id = $1 AND id = $2"#,
            scope.org_id(),
            id,
            version,
            epoch_micros(now)?,
        )
        .execute(&mut *conn)
        .await?;
    }
    get(conn, scope, id).await?.ok_or(Error::NotFound)
}

/// Save the version `version_id`'s body as the page `id`'s new current version (B5), with
/// [`save`]'s conflict check and image checks, under [`save`]'s course and page locks. A
/// version never changes, so it is read before them. The stored body is parsed again (its
/// structure, not its size, which was checked when it was saved), so one the schema no longer
/// allows is refused as [`Error::Body`].
pub async fn restore(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
    version_id: Uuid,
    (base_version_id, author_id): (Uuid, Uuid),
    now: SystemTime,
) -> Result<Page, Error> {
    let version = version(&mut *conn, scope, id, version_id)
        .await?
        .ok_or(Error::VersionNotFound)?;
    let body = PageDoc::parse_stored(&version.body)?;
    let save_of = Save {
        body: &body,
        base_version_id,
        author_id,
    };
    save(conn, scope, id, save_of, now).await
}

/// `version`'s body as HTML (B14, B15), images as placeholders for `page_doc::fill_images`: the
/// cached render when it was written by this release's renderer
/// ([`crate::page_doc::RENDERER_VERSION`]), otherwise rendered now and cached in place of an
/// older one. The cache holds no credential, only the placeholders.
pub async fn rendered(
    conn: &mut PgConnection,
    scope: OrgScope,
    version: &Version,
) -> Result<String, Error> {
    let renderer = crate::page_doc::RENDERER_VERSION;
    let id = version.summary.id;
    let cached = sqlx::query_scalar!(
        "SELECT html FROM page_renders
         WHERE org_id = $1 AND version_id = $2 AND renderer_version = $3",
        scope.org_id(),
        id,
        renderer,
    )
    .fetch_optional(&mut *conn)
    .await?;
    if let Some(html) = cached {
        return Ok(html);
    }
    let doc = PageDoc::parse_stored(&version.body).map_err(|_| Error::Stored {
        id: version.page_id,
        column: "version body",
    })?;
    let html = crate::page_doc::render(&doc);
    sqlx::query!(
        "INSERT INTO page_renders (org_id, version_id, renderer_version, html)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (version_id) DO UPDATE
         SET renderer_version = EXCLUDED.renderer_version, html = EXCLUDED.html,
             updated_at = now()
         WHERE page_renders.org_id = EXCLUDED.org_id
           AND page_renders.renderer_version < EXCLUDED.renderer_version",
        scope.org_id(),
        id,
        renderer,
        html,
    )
    .execute(conn)
    .await?;
    Ok(html)
}

/// The items that reference the page `id` of the scope's organization, in course order (B18):
/// by their module's position, then theirs.
pub async fn items_referencing(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Vec<Uuid>, Error> {
    let items = sqlx::query_scalar!(
        "SELECT i.id FROM module_items i
         JOIN modules m ON m.org_id = i.org_id AND m.id = i.module_id
         WHERE i.org_id = $1 AND i.kind = 'page' AND i.ref_id = $2
         ORDER BY m.position, i.position",
        scope.org_id(),
        id,
    )
    .fetch_all(db)
    .await?;
    Ok(items)
}

/// Copy every page of the course `from` into the course `to`, both of the scope's
/// organization, in the caller's transaction (B20): each with its title and slug and its current
/// version's body as version 1, its images naming the copies of their course files (`files`
/// maps a source course file's id to its copy's), authored by `author` or, without one, by the
/// source version's author, at `now`. Returns each source page's id mapped to its copy's.
pub async fn copy_course_pages(
    conn: &mut PgConnection,
    scope: OrgScope,
    (from, to): (Uuid, Uuid),
    files: &HashMap<Uuid, Uuid>,
    author: Option<Uuid>,
    now: SystemTime,
) -> Result<HashMap<Uuid, Uuid>, Error> {
    let sources = sqlx::query!(
        r#"SELECT p.id, p.slug, p.title, v.body::text AS "body!", v.author_id
           FROM pages p JOIN page_versions v ON v.org_id = p.org_id AND v.id = p.current_version_id
           WHERE p.org_id = $1 AND p.course_id = $2
           ORDER BY p.slug COLLATE "C""#,
        scope.org_id(),
        from,
    )
    .fetch_all(&mut *conn)
    .await?;
    let mut copies = HashMap::new();
    for source in sources {
        let stored = || Error::Stored {
            id: source.id,
            column: "version body",
        };
        let value: serde_json::Value = serde_json::from_str(&source.body).map_err(|_| stored())?;
        let body = PageDoc::parse_stored(&value)
            .map_err(|_| stored())?
            .with_images_mapped(files)
            .map_err(|_| stored())?;
        let new = NewPage {
            title: source.title,
            slug: Some(Slug::parse(&source.slug).ok_or_else(stored)?),
            body,
        };
        let author = author.unwrap_or(source.author_id);
        let page = insert_page(conn, scope, to, &new, (author, now)).await?;
        copies.insert(source.id, page.id);
    }
    Ok(copies)
}

/// Whether an item references the page `id`, or it is its course's home page (B6, B19).
pub async fn referenced(db: impl PgExecutor<'_>, scope: OrgScope, id: Uuid) -> Result<bool, Error> {
    let referenced = sqlx::query_scalar!(
        r#"SELECT (EXISTS (
                       SELECT 1 FROM module_items
                       WHERE org_id = $1 AND kind = 'page' AND ref_id = $2)
                   OR EXISTS (
                       SELECT 1 FROM pages p JOIN courses c ON c.org_id = p.org_id AND c.id = p.course_id
                       WHERE p.org_id = $1 AND p.id = $2
                         AND c.settings -> 'home' ->> 'kind' = 'page'
                         AND c.settings -> 'home' ->> 'page_id' = $2::text))
               AS "referenced!""#,
        scope.org_id(),
        id,
    )
    .fetch_one(db)
    .await?;
    Ok(referenced)
}

/// Delete the page `id` and its versions, unless an item references it or it is its course's
/// home page ([`Error::InUse`], B6).
pub async fn delete(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    lock_page(conn, scope, id).await?;
    if referenced(&mut *conn, scope, id).await? {
        return Err(Error::InUse);
    }
    sqlx::query!(
        "DELETE FROM pages WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// What [`versions`] and [`version`] read per row.
struct VersionRow {
    id: Uuid,
    number: i32,
    author_id: Uuid,
    created_at_us: i64,
}

impl TryFrom<VersionRow> for VersionSummary {
    type Error = Error;

    fn try_from(row: VersionRow) -> Result<Self, Error> {
        Ok(Self {
            id: row.id,
            number: row.number,
            author_id: row.author_id,
            created_at: from_epoch_micros(row.created_at_us)?,
        })
    }
}

/// Up to `limit` versions of the page `page_id`, newest first, numbered below `before` when it
/// is given (B5). The caller checks the page exists.
pub async fn versions(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    page_id: Uuid,
    before: Option<i32>,
    limit: i64,
) -> Result<Vec<VersionSummary>, Error> {
    let rows = sqlx::query_as!(
        VersionRow,
        r#"SELECT id, number, author_id,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!"
           FROM page_versions
           WHERE org_id = $1 AND page_id = $2 AND ($3::int IS NULL OR number < $3)
           ORDER BY number DESC
           LIMIT $4"#,
        scope.org_id(),
        page_id,
        before,
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(VersionSummary::try_from).collect()
}

/// The version `version_id` of the page `page_id` with its body, or `None`.
pub async fn version(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    page_id: Uuid,
    version_id: Uuid,
) -> Result<Option<Version>, Error> {
    let row = sqlx::query!(
        r#"SELECT id, number, author_id, body::text AS "body!",
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!"
           FROM page_versions
           WHERE org_id = $1 AND page_id = $2 AND id = $3"#,
        scope.org_id(),
        page_id,
        version_id,
    )
    .fetch_optional(db)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let body = serde_json::from_str(&row.body).map_err(|_| Error::Stored {
        id: page_id,
        column: "version body",
    })?;
    let summary = VersionSummary::try_from(VersionRow {
        id: row.id,
        number: row.number,
        author_id: row.author_id,
        created_at_us: row.created_at_us,
    })?;
    Ok(Some(Version {
        summary,
        page_id,
        body,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_runs_of_lowercase_letters_and_digits_joined_by_hyphens() {
        for ok in [
            "a",
            "ground-control-101",
            "x1-2-3",
            &"a".repeat(MAX_SLUG_CHARS),
        ] {
            assert_eq!(Slug::parse(ok).map(|s| s.0), Some(ok.to_owned()), "{ok}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "A",
            "a b",
            "a_b",
            "é",
            "a\0",
            &"a".repeat(MAX_SLUG_CHARS + 1),
        ] {
            assert_eq!(Slug::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn a_slug_is_derived_from_the_title() {
        let derive = |title: &str| Slug::derive(title).map(|s| s.0);
        // T1
        assert_eq!(
            derive("Ground Control 101").as_deref(),
            Some("ground-control-101")
        );
        assert_eq!(
            derive("  Café -- naïve résumé!  ").as_deref(),
            Some("cafe-naive-resume")
        );
        assert_eq!(
            derive("ATC: Phraseology (Part 2)").as_deref(),
            Some("atc-phraseology-part-2")
        );
        assert_eq!(derive("日本語"), None);
        assert_eq!(derive("!!!"), None);
        let long = format!("{} {}", "a".repeat(99), "bcd");
        assert_eq!(derive(&long), Some("a".repeat(99)));
        assert_eq!(derive(&"b".repeat(150)), Some("b".repeat(MAX_SLUG_CHARS)));
    }
}
