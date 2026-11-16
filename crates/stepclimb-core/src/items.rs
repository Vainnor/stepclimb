// SPDX-License-Identifier: AGPL-3.0-or-later
//! The module items repository (docs/specs/m1/03-modules-and-items.md B4 to B12).
//!
//! An item belongs to one module and has a kind ([`Kind`]); its `position` is 0-based and dense
//! within the module. `course_id` repeats the module's course, and the table's three-column
//! foreign key keeps it so: an item moves only between modules of its own course (B11). Every
//! write first locks the course row (`modules::lock_course`, B12) and refuses an archived
//! course, as [`crate::modules`] does. Errors are [`crate::modules::Error`].
//!
//! Which kinds can be created is [`Kind::creatable`]: `header` and `external_link` in this
//! release. Each content spec makes its kind creatable together with the check of its `ref_id`
//! (B5, B6). Every query filters by `org_id = scope.org_id()`.

use std::time::SystemTime;

use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{epoch_micros, from_epoch_micros},
    id::new_id,
    modules::{Error, lock_course, position, same_set, valid_title},
};

/// The most items a module holds (Q4).
pub const MAX_PER_MODULE: i64 = 200;

/// The longest external link URL, in characters (B7).
pub const MAX_URL_CHARS: usize = 2048;

/// What an item points at (B5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A label that groups the items below it; no reference or URL.
    Header,
    /// A `url`.
    ExternalLink,
    /// A course file (spec 06).
    File,
    /// A page (spec 07).
    Page,
    /// A video (specs 08, 09).
    Video,
    /// A quiz (M2).
    Quiz,
    /// An activity (M3).
    Activity,
}

impl Kind {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::ExternalLink => "external_link",
            Self::File => "file",
            Self::Page => "page",
            Self::Video => "video",
            Self::Quiz => "quiz",
            Self::Activity => "activity",
        }
    }

    /// The kind stored as `value`, or `None` for a value the table's check refuses.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::Header,
            Self::ExternalLink,
            Self::File,
            Self::Page,
            Self::Video,
            Self::Quiz,
            Self::Activity,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == value)
    }

    /// Whether this release creates items of the kind (B5). A kind whose spec is not built yet
    /// is refused, and so is any `ref_id`: no creatable kind has one.
    #[must_use]
    pub fn creatable(self) -> bool {
        matches!(self, Self::Header | Self::ExternalLink)
    }

    /// Whether items of the kind carry a `url` (B5, B7).
    #[must_use]
    pub fn has_url(self) -> bool {
        self == Self::ExternalLink
    }
}

/// An item as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: Uuid,
    pub module_id: Uuid,
    pub course_id: Uuid,
    pub kind: Kind,
    pub title: String,
    pub position: i32,
    pub published: bool,
    pub ref_id: Option<Uuid>,
    pub url: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// An item to create. The URL's form (B7) is checked by the caller; this module checks only
/// that it is present exactly for an external link, has no NUL and fits the column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewItem {
    pub kind: Kind,
    pub title: String,
    pub ref_id: Option<Uuid>,
    pub url: Option<String>,
    /// Where to insert it, 0 to the module's item count; `None` puts it last.
    pub position: Option<i64>,
}

/// A change to an item; `None` leaves a member as it is. The kind and `ref_id` never change
/// (B6).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemPatch {
    pub title: Option<String>,
    pub published: Option<bool>,
    /// Only for an external link.
    pub url: Option<String>,
}

struct Row {
    id: Uuid,
    module_id: Uuid,
    course_id: Uuid,
    kind: String,
    title: String,
    position: i32,
    published: bool,
    ref_id: Option<Uuid>,
    url: Option<String>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Item {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        Ok(Self {
            kind: Kind::parse(&row.kind).ok_or(Error::Stored {
                id: row.id,
                column: "kind",
            })?,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            id: row.id,
            module_id: row.module_id,
            course_id: row.course_id,
            title: row.title,
            position: row.position,
            published: row.published,
            ref_id: row.ref_id,
            url: row.url,
        })
    }
}

/// The item `id` of the scope's organization, or `None`.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Item>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, module_id, course_id, kind, title, position, published, ref_id, url,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM module_items WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Item::try_from)
    .transpose()
}

/// The course the item `id` of the scope's organization belongs to, or `None`: what
/// `ItemTarget` checks grants at. An item never changes course.
pub async fn course_of(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Uuid>, Error> {
    Ok(sqlx::query_scalar!(
        "SELECT course_id FROM module_items WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?)
}

/// The items of the module `module_id` of the scope's organization, in order.
pub async fn list_for_module(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    module_id: Uuid,
) -> Result<Vec<Item>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, module_id, course_id, kind, title, position, published, ref_id, url,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM module_items WHERE org_id = $1 AND module_id = $2
           ORDER BY position"#,
        scope.org_id(),
        module_id,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Item::try_from).collect()
}

/// The items of every module of the course `course_id` of the scope's organization, ordered by
/// module id, then position.
pub async fn list_for_course(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    course_id: Uuid,
) -> Result<Vec<Item>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, module_id, course_id, kind, title, position, published, ref_id, url,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM module_items WHERE org_id = $1 AND course_id = $2
           ORDER BY module_id, position"#,
        scope.org_id(),
        course_id,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Item::try_from).collect()
}

/// Create an unpublished item in the module `module_id` of the scope's organization, at
/// `new.position` or last, with `created_at` and `updated_at` set to `now` (B4 to B8). Items at
/// and after the position move down one.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    module_id: Uuid,
    new: &NewItem,
    now: SystemTime,
) -> Result<Item, Error> {
    let title = valid_title(&new.title)?;
    check_kind(new)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let course_id = lock_module_course(&mut tx, scope, module_id).await?;
    let count = count_in(&mut tx, scope, module_id).await?;
    if count >= MAX_PER_MODULE {
        return Err(Error::ItemLimitReached);
    }
    let at = match new.position {
        Some(at) if !(0..=count).contains(&at) => return Err(Error::PositionOutOfRange),
        Some(at) => at,
        None => count,
    };
    shift(&mut tx, scope, module_id, at, i64::MAX, 1).await?;
    let id = new_id();
    sqlx::query!(
        "INSERT INTO module_items
             (id, org_id, course_id, module_id, position, kind, ref_id, url, title,
              created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9,
                 timestamptz 'epoch' + $10::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $10::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        course_id,
        module_id,
        position(at)?,
        new.kind.as_str(),
        new.ref_id,
        new.url,
        title,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let item = get(&mut *tx, scope, id).await?.ok_or(Error::ItemNotFound)?;
    tx.commit().await?;
    Ok(item)
}

/// Apply `patch` to the item `id` of the scope's organization and set `updated_at` to `now`. A
/// `url` on an item that is not an external link is [`Error::UrlUnavailable`]. A patch that
/// changes nothing writes nothing and returns the item as it is.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn update<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    patch: &ItemPatch,
    now: SystemTime,
) -> Result<Item, Error> {
    let title = patch.title.as_deref().map(valid_title).transpose()?;
    if let Some(url) = &patch.url {
        check_url_text(url)?;
    }
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let item = locked(&mut tx, scope, id).await?;
    if patch.url.is_some() && !item.kind.has_url() {
        return Err(Error::UrlUnavailable);
    }
    let title = title.unwrap_or(&item.title);
    let published = patch.published.unwrap_or(item.published);
    let url = patch.url.as_ref().or(item.url.as_ref());
    if title == item.title && published == item.published && url == item.url.as_ref() {
        return Ok(item);
    }
    sqlx::query!(
        "UPDATE module_items
         SET title = $3, published = $4, url = $5,
             updated_at = timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        title,
        published,
        url,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let item = get(&mut *tx, scope, id).await?.ok_or(Error::ItemNotFound)?;
    tx.commit().await?;
    Ok(item)
}

/// Delete the item `id` of the scope's organization and close the gap in its module's
/// positions (B9). What the item points at stays.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn delete<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    let item = locked(&mut tx, scope, id).await?;
    sqlx::query!(
        "DELETE FROM module_items WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
    )
    .execute(&mut *tx)
    .await?;
    let after = i64::from(item.position) + 1;
    shift(&mut tx, scope, item.module_id, after, i64::MAX, -1).await?;
    tx.commit().await?;
    Ok(())
}

/// Put the items of the module `module_id` of the scope's organization in the order of `ids`,
/// which must be exactly the module's current item ids (B10): otherwise [`Error::OrderStale`]
/// and nothing changes. The current order writes nothing. Returns the items in their new order.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn reorder<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    module_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Item>, Error> {
    let mut tx = db.begin().await?;
    lock_module_course(&mut tx, scope, module_id).await?;
    let current = sqlx::query_scalar!(
        "SELECT id FROM module_items WHERE org_id = $1 AND module_id = $2 ORDER BY position",
        scope.org_id(),
        module_id,
    )
    .fetch_all(&mut *tx)
    .await?;
    if !same_set(&current, ids) {
        return Err(Error::OrderStale);
    }
    if current != ids {
        sqlx::query!(
            "UPDATE module_items m SET position = (u.ord - 1)::integer
             FROM unnest($3::uuid[]) WITH ORDINALITY AS u(id, ord)
             WHERE m.org_id = $1 AND m.module_id = $2 AND m.id = u.id",
            scope.org_id(),
            module_id,
            ids,
        )
        .execute(&mut *tx)
        .await?;
    }
    let items = list_for_module(&mut *tx, scope, module_id).await?;
    tx.commit().await?;
    Ok(items)
}

/// Move the item `id` of the scope's organization to `position` in the module `module_id` of
/// the same course (B11), closing the gap it leaves and opening one where it lands. A module of
/// another course is [`Error::ModuleNotFound`]. The position runs from 0 to the destination's
/// item count without the moved item; the item's own place writes nothing. A move into another
/// module sets `updated_at` to `now`.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the course lock.
pub async fn move_to<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    module_id: Uuid,
    to: i64,
    now: SystemTime,
) -> Result<Item, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let item = locked(&mut tx, scope, id).await?;
    let in_course = sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM modules WHERE org_id = $1 AND id = $2 AND course_id = $3)
               AS "exists!""#,
        scope.org_id(),
        module_id,
        item.course_id,
    )
    .fetch_one(&mut *tx)
    .await?;
    if !in_course {
        return Err(Error::ModuleNotFound);
    }
    let from = i64::from(item.position);
    let same_module = module_id == item.module_id;
    let count = count_in(&mut tx, scope, module_id).await?;
    let room = if same_module { count - 1 } else { count };
    if !same_module && count >= MAX_PER_MODULE {
        return Err(Error::ItemLimitReached);
    }
    if !(0..=room).contains(&to) {
        return Err(Error::PositionOutOfRange);
    }
    if same_module && to == from {
        return Ok(item);
    }
    if same_module && to < from {
        shift(&mut tx, scope, module_id, to, from, 1).await?;
    } else if same_module {
        shift(&mut tx, scope, module_id, from + 1, to + 1, -1).await?;
    } else {
        shift(&mut tx, scope, item.module_id, from + 1, i64::MAX, -1).await?;
        shift(&mut tx, scope, module_id, to, i64::MAX, 1).await?;
    }
    sqlx::query!(
        "UPDATE module_items
         SET module_id = $3, position = $4,
             updated_at = CASE WHEN module_id = $3 THEN updated_at
                               ELSE timestamptz 'epoch' + $5::bigint * interval '1 microsecond' END
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        module_id,
        position(to)?,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let item = get(&mut *tx, scope, id).await?.ok_or(Error::ItemNotFound)?;
    tx.commit().await?;
    Ok(item)
}

/// Lock the course of the module `module_id` (B12): its id, or [`Error::ModuleNotFound`].
async fn lock_module_course(
    conn: &mut PgConnection,
    scope: OrgScope,
    module_id: Uuid,
) -> Result<Uuid, Error> {
    let course_id = crate::modules::course_of(&mut *conn, scope, module_id)
        .await?
        .ok_or(Error::ModuleNotFound)?;
    lock_course(&mut *conn, scope, course_id).await?;
    // The module may have been deleted while the lock was awaited.
    crate::modules::course_of(&mut *conn, scope, module_id)
        .await?
        .ok_or(Error::ModuleNotFound)
}

/// The item `id`, read after locking its course.
async fn locked(conn: &mut PgConnection, scope: OrgScope, id: Uuid) -> Result<Item, Error> {
    let course_id = course_of(&mut *conn, scope, id)
        .await?
        .ok_or(Error::ItemNotFound)?;
    lock_course(&mut *conn, scope, course_id).await?;
    get(&mut *conn, scope, id).await?.ok_or(Error::ItemNotFound)
}

async fn count_in(conn: &mut PgConnection, scope: OrgScope, module_id: Uuid) -> Result<i64, Error> {
    Ok(sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM module_items WHERE org_id = $1 AND module_id = $2"#,
        scope.org_id(),
        module_id,
    )
    .fetch_one(conn)
    .await?)
}

/// Add `by` to the position of every item of `module_id` from `from` up to, not including,
/// `until`. The positions' unique constraint is deferred, so the run may overlap mid-statement.
async fn shift(
    conn: &mut PgConnection,
    scope: OrgScope,
    module_id: Uuid,
    from: i64,
    until: i64,
    by: i32,
) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE module_items SET position = position + $5::integer
         WHERE org_id = $1 AND module_id = $2 AND position >= $3::bigint AND position < $4::bigint",
        scope.org_id(),
        module_id,
        from,
        until,
        by,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// The kind's rules for a new item: creatable (B5), a URL exactly for an external link, and no
/// `ref_id`, which no creatable kind has.
fn check_kind(new: &NewItem) -> Result<(), Error> {
    if !new.kind.creatable() {
        return Err(Error::KindUnavailable);
    }
    if new.ref_id.is_some() {
        return Err(Error::RefIdUnavailable);
    }
    match (&new.url, new.kind.has_url()) {
        (Some(url), true) => check_url_text(url),
        (None, true) => Err(Error::UrlRequired),
        (Some(_), false) => Err(Error::UrlUnavailable),
        (None, false) => Ok(()),
    }
}

/// What this module checks of a URL: no NUL, and at most [`MAX_URL_CHARS`] characters, which
/// the column also enforces.
fn check_url_text(url: &str) -> Result<(), Error> {
    if url.contains('\0') {
        return Err(Error::ContainsNul { field: "url" });
    }
    if url.chars().count() > MAX_URL_CHARS {
        return Err(Error::UrlInvalid);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new(kind: Kind) -> NewItem {
        NewItem {
            kind,
            title: "Item".into(),
            ref_id: None,
            url: None,
            position: None,
        }
    }

    #[test]
    fn kinds_round_trip_their_names() {
        for kind in [
            Kind::Header,
            Kind::ExternalLink,
            Kind::File,
            Kind::Page,
            Kind::Video,
            Kind::Quiz,
            Kind::Activity,
        ] {
            assert_eq!(Kind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(Kind::parse("lti"), None);
    }

    #[test]
    fn only_headers_and_external_links_are_creatable() {
        assert!(check_kind(&new(Kind::Header)).is_ok());
        for kind in [
            Kind::File,
            Kind::Page,
            Kind::Video,
            Kind::Quiz,
            Kind::Activity,
        ] {
            assert!(
                matches!(check_kind(&new(kind)), Err(Error::KindUnavailable)),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn a_url_belongs_to_an_external_link_only_and_no_kind_takes_a_reference() {
        let link = |url: Option<&str>| NewItem {
            url: url.map(str::to_owned),
            ..new(Kind::ExternalLink)
        };
        assert!(check_kind(&link(Some("https://example.com/"))).is_ok());
        assert!(matches!(check_kind(&link(None)), Err(Error::UrlRequired)));
        let header_with_url = NewItem {
            url: Some("https://example.com/".into()),
            ..new(Kind::Header)
        };
        assert!(matches!(
            check_kind(&header_with_url),
            Err(Error::UrlUnavailable)
        ));
        let header_with_ref = NewItem {
            ref_id: Some(new_id()),
            ..new(Kind::Header)
        };
        assert!(matches!(
            check_kind(&header_with_ref),
            Err(Error::RefIdUnavailable)
        ));
        assert!(matches!(
            check_kind(&link(Some("https://a\0b"))),
            Err(Error::ContainsNul { field: "url" })
        ));
    }
}
