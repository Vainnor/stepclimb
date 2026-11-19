// SPDX-License-Identifier: AGPL-3.0-or-later
//! The courses repository (docs/specs/m1/02-courses-and-enrollments.md B1 to B11, B27).
//!
//! A course belongs to one account of the organization. Its scope path is read through that
//! account on every request, so an account move carries its courses with it. Every query
//! filters by `org_id = scope.org_id()`.
//!
//! [`create`], [`copy`] and [`move_to`] take the organization's account tree lock
//! (`accounts::lock_tree`, spec 02 B22), so a caller that checks its permission at the account
//! under that lock on the same transaction knows the account cannot move before the write.
//! [`lock`] reads a course `FOR NO KEY UPDATE` and its account `FOR SHARE`, which waits for an
//! account move in progress over it, so the path it returns holds until the transaction ends.
//!
//! Who may do what to which course is the routes' to decide; this module checks only the rules
//! of the course itself: its title, code and dates, the code's uniqueness (Q2), and which status
//! changes are allowed (B4, Q4, Q5).
//!
//! [`copy`] (B12 to B14) copies the course row and its outline: the new course's account, code,
//! title and `template` flag come from the caller, and its dates, `settings` and completion
//! validity (spec 14 B3) from the source; every module and item is copied with a new id, its
//! position and its `published` flag (spec 03), each module with its validity. It copies no enrollment, progress or other learner data. The content specs add their
//! parts to it, in its transaction.

use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    accounts::{self, Reach},
    audit::{self, Action, Actor},
    completions,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    enrollments,
    id::new_id,
    items, name,
};

/// The longest course title, in characters once trimmed: the name rule's limit.
pub const MAX_TITLE_CHARS: usize = name::MAX_CHARS;

/// The longest course code, in characters once trimmed (Q2).
pub const MAX_CODE_CHARS: usize = 50;

/// The first instant a course date cannot be, in seconds since the Unix epoch:
/// 2200-01-01T00:00:00Z. Dates are bound to SQL as microseconds through a `float8` interval,
/// exact only up to 2^53 microseconds (the year 2255).
pub const DATES_END_SECS: u64 = 7_258_118_400;

/// The unique index that keeps a course code unique in its organization, ignoring ASCII case.
const CODE_INDEX: &str = "courses_code";

/// A course's status (B4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Draft,
    Published,
    Archived,
}

impl Status {
    /// The stored and API name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Published => "published",
            Self::Archived => "archived",
        }
    }

    /// The status stored as `value`, or `None` for a value the table's check refuses.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "draft" => Some(Self::Draft),
            "published" => Some(Self::Published),
            "archived" => Some(Self::Archived),
            _ => None,
        }
    }

    /// Whether B4 allows the change from `self` to `to`, another status. Published back to
    /// draft is allowed here; [`set_status`] refuses it while the course has learners.
    fn may_become(self, to: Self) -> bool {
        matches!(
            (self, to),
            (Self::Draft, Self::Published)
                | (Self::Published, Self::Archived)
                | (Self::Archived, Self::Published)
                | (Self::Published, Self::Draft)
        )
    }
}

/// What a learner sees first when opening the course (B6, Q6).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Home {
    /// The module list.
    #[default]
    Modules,
    /// A page of the course (spec 07 B19).
    Page { page_id: Uuid },
}

/// The typed form of `courses.settings`. A key missing from the stored object reads as its
/// default, so a new course stores `{}`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CourseSettings {
    pub home: Home,
}

/// A course as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Course {
    pub id: Uuid,
    pub account_id: Uuid,
    /// The ids on the account's path: the root first, the course's account last.
    pub account_path: Vec<Uuid>,
    pub code: String,
    pub title: String,
    pub status: Status,
    pub template: bool,
    pub start_at: Option<SystemTime>,
    pub end_at: Option<SystemTime>,
    pub settings: CourseSettings,
    /// How many months a completion of the course stays valid, 1 to 120; `None` for ever
    /// (spec 14 B3).
    pub completion_valid_months: Option<i16>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

impl Course {
    /// Whether B10 shows the course only to holders of `course.view_all`: a draft or a template.
    #[must_use]
    pub fn needs_view_all(&self) -> bool {
        self.status == Status::Draft || self.template
    }
}

/// A course to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewCourse {
    pub account_id: Uuid,
    /// Trimmed; 1 to [`MAX_TITLE_CHARS`] characters.
    pub title: String,
    /// Trimmed; 1 to [`MAX_CODE_CHARS`] characters, unique in the organization ignoring ASCII
    /// case.
    pub code: String,
}

/// What [`copy`] makes of a source course (B12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CourseCopy {
    /// The account the copy belongs to.
    pub account_id: Uuid,
    /// Trimmed; 1 to [`MAX_TITLE_CHARS`] characters. `None` keeps the source's title.
    pub title: Option<String>,
    /// Trimmed; 1 to [`MAX_CODE_CHARS`] characters, unique in the organization ignoring ASCII
    /// case.
    pub code: String,
    /// Whether the copy is a template; copying a template without it makes an ordinary course.
    pub template: bool,
}

/// A change to a course; `None` leaves a member as it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoursePatch {
    pub title: Option<String>,
    pub code: Option<String>,
    /// `Some(None)` clears the date.
    pub start_at: Option<Option<SystemTime>>,
    /// `Some(None)` clears the date.
    pub end_at: Option<Option<SystemTime>>,
    pub home: Option<Home>,
    /// `Some(None)` clears the validity (spec 14 B3).
    pub completion_valid_months: Option<Option<i16>>,
}

/// The filters of [`list_visible`]; `None` filters nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ListFilter {
    /// Only courses in this account's subtree.
    pub account_id: Option<Uuid>,
    pub status: Option<Status>,
    pub template: Option<bool>,
}

/// Which courses [`list_visible`] may return (B10, B11): those in `view` or `view_courses`,
/// and of those the drafts and templates only when also in `view_all` or `view_all_courses`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visibility<'a> {
    /// Where the caller holds `course.view` through the organization or accounts.
    pub view: Reach<'a>,
    /// The courses where an enrollment gives the caller `course.view` (B16).
    pub view_courses: &'a [Uuid],
    /// Where the caller holds `course.view_all` through the organization or accounts.
    pub view_all: Reach<'a>,
    /// The courses where an enrollment gives the caller `course.view_all`.
    pub view_all_courses: &'a [Uuid],
}

/// The sort key of a course in [`list_visible`]: `(title, id)`.
pub type ListKey = (String, Uuid);

/// Which date a date error names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateField {
    StartAt,
    EndAt,
}

/// Why a courses repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No course with that id in the scope's organization.
    #[error("the course does not exist")]
    NotFound,
    /// No account with that id in the scope's organization.
    #[error("the account does not exist")]
    AccountNotFound,
    #[error("a course title cannot be empty")]
    TitleEmpty,
    #[error("a course title is longer than {MAX_TITLE_CHARS} characters")]
    TitleTooLong,
    #[error("a course code cannot be empty")]
    CodeEmpty,
    #[error("a course code is longer than {MAX_CODE_CHARS} characters")]
    CodeTooLong,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// Another course of the organization has the code, ignoring ASCII case (Q2).
    #[error("another course of the organization has this code")]
    CodeTaken,
    /// `end_at` would be before `start_at` (B3).
    #[error("a course cannot end before it starts")]
    DatesInverted,
    /// A `completion_valid_months` outside 1 to 120 (spec 14 B3).
    #[error("a completion validity must be from 1 to 120 months")]
    ValidityOutOfRange,
    /// A date before 1970 or from [`DATES_END_SECS`] on.
    #[error("a course date must be from 1970 up to 2200")]
    DateOutOfRange { field: DateField },
    /// B4 does not allow the status change.
    #[error("the course cannot change from {from} to {to}")]
    StatusInvalid {
        from: &'static str,
        to: &'static str,
    },
    /// A template cannot be published (Q5).
    #[error("a template cannot be published")]
    Template,
    /// A published course with learners cannot go back to draft (B4).
    #[error("the course has learners and cannot go back to draft")]
    HasLearners,
    /// A page of the source could not be copied (spec 07 B20).
    #[error(transparent)]
    Page(crate::pages::Error),
    /// A home page that is not a page of the course (spec 07 B19).
    #[error("the page does not exist in the course")]
    PageNotFound,
    /// A stored value this release cannot read: a status or `settings` the table's checks and
    /// [`CourseSettings`] keep from happening.
    #[error("the {column} of course {id} cannot be read")]
    Stored { id: Uuid, column: &'static str },
    /// The source has an item [`copy`] cannot copy yet: a kind whose content spec, which copies
    /// what its `ref_id` names, is not built (spec 03 B5, Data). No write of this release makes
    /// one.
    #[error("item {item_id} is a {kind}, which a course copy cannot copy yet")]
    ContentNotCopyable { item_id: Uuid, kind: String },
    /// The source has an external link whose stored URL [`items::url_is_valid`] refuses, which
    /// no write of this release stores; the copy does not carry it into a new course.
    #[error("item {item_id} has a URL that is not an absolute http or https link")]
    CopiedUrlInvalid { item_id: Uuid },
    /// The tree lock or a path read failed.
    #[error(transparent)]
    Account(#[from] accounts::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(CODE_INDEX) => Self::CodeTaken,
            _ => Self::Database(error),
        }
    }
}

/// A course row as the queries select it, with its account's path.
struct Row {
    id: Uuid,
    account_id: Uuid,
    account_path: String,
    code: String,
    title: String,
    status: String,
    template: bool,
    start_at_us: Option<i64>,
    end_at_us: Option<i64>,
    settings: String,
    completion_valid_months: Option<i16>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Course {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let stored = |column| Error::Stored { id: row.id, column };
        let time = |us: Option<i64>| us.map(from_epoch_micros).transpose();
        Ok(Self {
            account_path: accounts::parse_path(&row.account_path).ok_or(stored("account path"))?,
            status: Status::parse(&row.status).ok_or(stored("status"))?,
            settings: serde_json::from_str(&row.settings).map_err(|_| stored("settings"))?,
            start_at: time(row.start_at_us)?,
            end_at: time(row.end_at_us)?,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
            id: row.id,
            account_id: row.account_id,
            code: row.code,
            title: row.title,
            template: row.template,
            completion_valid_months: row.completion_valid_months,
        })
    }
}

/// The course `id` of the scope's organization, or `None` when it has no such course.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Course>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT c.id, c.account_id, a.path::text AS "account_path!", c.code, c.title, c.status,
                  c.template,
                  (extract(epoch FROM c.start_at) * 1000000)::bigint AS start_at_us,
                  (extract(epoch FROM c.end_at) * 1000000)::bigint AS end_at_us,
                  c.settings::text AS "settings!", c.completion_valid_months,
                  (extract(epoch FROM c.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM c.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM courses c JOIN accounts a ON a.org_id = c.org_id AND a.id = c.account_id
           WHERE c.org_id = $1 AND c.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Course::try_from)
    .transpose()
}

/// [`get`] for a caller about to change the course in its transaction: the course row
/// `FOR NO KEY UPDATE` and its account's row `FOR SHARE`, so neither the course nor its
/// account's path can change until the transaction ends. An account move in progress over the
/// course is waited for, and the path it committed is the one returned.
pub async fn lock(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Course>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT c.id, c.account_id, a.path::text AS "account_path!", c.code, c.title, c.status,
                  c.template,
                  (extract(epoch FROM c.start_at) * 1000000)::bigint AS start_at_us,
                  (extract(epoch FROM c.end_at) * 1000000)::bigint AS end_at_us,
                  c.settings::text AS "settings!", c.completion_valid_months,
                  (extract(epoch FROM c.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM c.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM courses c JOIN accounts a ON a.org_id = c.org_id AND a.id = c.account_id
           WHERE c.org_id = $1 AND c.id = $2
           FOR NO KEY UPDATE OF c FOR SHARE OF a"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .map(Course::try_from)
    .transpose()
}

/// [`lock`] with the course row `FOR SHARE`: for a write that must wait for a status change in
/// progress and keep the course from changing until it commits, without making other such
/// writes to the same course wait for it (spec m1/08 D14: chunks of different uploads).
pub async fn lock_shared(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Course>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT c.id, c.account_id, a.path::text AS "account_path!", c.code, c.title, c.status,
                  c.template,
                  (extract(epoch FROM c.start_at) * 1000000)::bigint AS start_at_us,
                  (extract(epoch FROM c.end_at) * 1000000)::bigint AS end_at_us,
                  c.settings::text AS "settings!", c.completion_valid_months,
                  (extract(epoch FROM c.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM c.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM courses c JOIN accounts a ON a.org_id = c.org_id AND a.id = c.account_id
           WHERE c.org_id = $1 AND c.id = $2
           FOR SHARE OF c FOR SHARE OF a"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .map(Course::try_from)
    .transpose()
}

/// The ids on the path of the account the course `id` belongs to, the root first, or `None`
/// when the scope's organization has no such course. [`locate`] reads it with B10's draft and
/// template flag, for `CourseTarget` (B24).
pub async fn path(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Vec<Uuid>>, Error> {
    let path = sqlx::query_scalar!(
        r#"SELECT a.path::text AS "path!"
           FROM courses c JOIN accounts a ON a.org_id = c.org_id AND a.id = c.account_id
           WHERE c.org_id = $1 AND c.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    path.map(|path| {
        accounts::parse_path(&path).ok_or(Error::Stored {
            id,
            column: "account path",
        })
    })
    .transpose()
}

/// Where a course is and whether B10 hides it: what `CourseTarget` and the targets reached
/// through a course check (B24, Q13).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    /// The ids on the path of the course's account, the root first.
    pub account_path: Vec<Uuid>,
    /// A draft or a template ([`Course::needs_view_all`]): visible only to holders of
    /// `course.view_all` at the course.
    pub needs_view_all: bool,
}

/// [`path`] with whether the course is a draft or a template, in one read, or `None` when the
/// scope's organization has no course `id`.
pub async fn locate(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Located>, Error> {
    let row = sqlx::query!(
        r#"SELECT a.path::text AS "path!", (c.status = 'draft' OR c.template) AS "hidden!"
           FROM courses c JOIN accounts a ON a.org_id = c.org_id AND a.id = c.account_id
           WHERE c.org_id = $1 AND c.id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?;
    row.map(|row| {
        let account_path = accounts::parse_path(&row.path).ok_or(Error::Stored {
            id,
            column: "account path",
        })?;
        Ok(Located {
            account_path,
            needs_view_all: row.hidden,
        })
    })
    .transpose()
}

/// Whether any course, in any status, belongs to the account `account_id` of the scope's
/// organization: `accounts::delete` refuses such an account (B8).
pub async fn any_in_account(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    account_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar!(
        r#"SELECT EXISTS (SELECT 1 FROM courses WHERE org_id = $1 AND account_id = $2)
               AS "exists!""#,
        scope.org_id(),
        account_id,
    )
    .fetch_one(db)
    .await
}

/// The scope's courses `visibility` allows and `filter` matches, sorted by title then id,
/// starting after `after` (the sort key of the previous page's last course) and at most `limit`
/// of them (B11).
///
/// `after` comes from a client's cursor: it only bounds a query already filtered by the scope.
/// A NUL in its title is refused with [`Error::ContainsNul`].
pub async fn list_visible(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    visibility: Visibility<'_>,
    filter: ListFilter,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Course>, Error> {
    let after_title = after.map(|(t, _)| no_nul("cursor", t)).transpose()?;
    let (view_org, view_ids) = reach_parts(visibility.view);
    let (all_org, all_ids) = reach_parts(visibility.view_all);
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT c.id, c.account_id, a.path::text AS "account_path!", c.code, c.title, c.status,
                  c.template,
                  (extract(epoch FROM c.start_at) * 1000000)::bigint AS start_at_us,
                  (extract(epoch FROM c.end_at) * 1000000)::bigint AS end_at_us,
                  c.settings::text AS "settings!", c.completion_valid_months,
                  (extract(epoch FROM c.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM c.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM courses c JOIN accounts a ON a.org_id = c.org_id AND a.id = c.account_id
           WHERE c.org_id = $1
             AND ($2::bool OR c.id = ANY($12::uuid[]) OR EXISTS (
                     SELECT 1 FROM accounts v
                     WHERE v.org_id = $1 AND v.id = ANY($3::uuid[]) AND a.path <@ v.path))
             AND ((c.status <> 'draft' AND NOT c.template) OR $4::bool
                  OR c.id = ANY($13::uuid[]) OR EXISTS (
                     SELECT 1 FROM accounts v
                     WHERE v.org_id = $1 AND v.id = ANY($5::uuid[]) AND a.path <@ v.path))
             AND ($6::uuid IS NULL OR a.path <@ (
                     SELECT s.path FROM accounts s WHERE s.org_id = $1 AND s.id = $6))
             AND ($7::text IS NULL OR c.status = $7)
             AND ($8::bool IS NULL OR c.template = $8)
             AND ($9::text IS NULL OR (c.title, c.id) > ($9, $10::uuid))
           ORDER BY c.title, c.id
           LIMIT $11"#,
        scope.org_id(),
        view_org,
        view_ids,
        all_org,
        all_ids,
        filter.account_id,
        filter.status.map(Status::as_str),
        filter.template,
        after_title,
        after.map(|(_, id)| *id),
        limit,
        visibility.view_courses,
        visibility.view_all_courses,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Course::try_from).collect()
}

/// A reach as the list query binds it: whether it is the whole organization, and the accounts.
fn reach_parts(reach: Reach<'_>) -> (bool, &[Uuid]) {
    match reach {
        Reach::Organization => (true, &[]),
        Reach::Accounts(ids) => (false, ids),
    }
}

/// Create a `draft` course in the account `new.account_id` of the scope's organization, with
/// `created_at` and `updated_at` set to `now` (B1, B2), and its `course.create` audit entry by
/// `actor`.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the tree lock, after
/// reading the account there: a caller that checked its permission at the account under the
/// lock on the same transaction keeps that check true up to the insert.
pub async fn create<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    new: &NewCourse,
    now: SystemTime,
    actor: Actor,
) -> Result<Course, Error> {
    let title = valid_title(&new.title)?;
    let code = valid_code(&new.code)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    accounts::lock_tree(&mut tx, scope).await?;
    accounts::ancestors_path(&mut *tx, scope, new.account_id)
        .await?
        .ok_or(Error::AccountNotFound)?;
    let id = new_id();
    sqlx::query!(
        "INSERT INTO courses (id, org_id, account_id, code, title, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5,
                 timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $6::bigint * interval '1 microsecond')",
        id,
        scope.org_id(),
        new.account_id,
        code,
        title,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({ "after": {
        "account_id": new.account_id, "title": title, "code": code, "template": false,
    } });
    audit::record(&mut tx, scope, actor, Action::CourseCreate, id, &data, now).await?;
    let course = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(course)
}

/// Copy the course `source_id` of the scope's organization into a new `draft` course as `copy`
/// says, with `created_at` and `updated_at` set to `now`, and write the `course.copy` audit entry
/// on the new course by `actor`, naming the source, the account and `template` (B12, B27).
///
/// The new course takes the source's dates, `settings` and `completion_valid_months`, and its
/// title unless `copy` names one; it is a draft whatever the source's status, and a template
/// exactly when `copy.template` is. Every module and item of the source is copied with a new
/// id, its position, title and `published` flag, and an item's kind and URL, with the unlock and
/// completion rules, requirements and prerequisites of spec 04 and each module's validity, and
/// the course files with the `file` items naming their copies (spec 06 B20) ([`copy_outline`]).
/// Nothing of the source's learners is copied: no enrollment, progress, attempt or other learner
/// data (B13).
///
/// The source course row is read `FOR SHARE`, which every outline write's `FOR NO KEY UPDATE`
/// (spec 03 B12) waits for, so the outline cannot change while it is copied.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the tree lock, after
/// reading the destination account there, as [`create`] does: a caller that checked its
/// permissions at the source and the destination under the lock on the same transaction keeps
/// those checks true up to the insert.
pub async fn copy<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    source_id: Uuid,
    copy: &CourseCopy,
    now: SystemTime,
    actor: Actor,
) -> Result<Course, Error> {
    let title = copy.title.as_deref().map(valid_title).transpose()?;
    let code = valid_code(&copy.code)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    accounts::lock_tree(&mut tx, scope).await?;
    accounts::ancestors_path(&mut *tx, scope, copy.account_id)
        .await?
        .ok_or(Error::AccountNotFound)?;
    sqlx::query_scalar!(
        "SELECT id FROM courses WHERE org_id = $1 AND id = $2 FOR SHARE",
        scope.org_id(),
        source_id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let id = new_id();
    let inserted = sqlx::query!(
        "INSERT INTO courses (id, org_id, account_id, code, title, status, template, start_at,
                              end_at, settings, completion_valid_months, created_at, updated_at)
         SELECT $1, org_id, $3, $4, COALESCE($5, title), 'draft', $6, start_at, end_at, settings,
                completion_valid_months,
                timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $7::bigint * interval '1 microsecond'
         FROM courses
         WHERE org_id = $2 AND id = $8",
        id,
        scope.org_id(),
        copy.account_id,
        code,
        title,
        copy.template,
        now,
        source_id,
    )
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if inserted == 0 {
        return Err(Error::NotFound);
    }
    let author = match actor {
        Actor::User { id, .. } => Some(id),
        Actor::ApiKey { .. } | Actor::Instance => None,
    };
    copy_outline(&mut tx, scope, (source_id, id), now, author).await?;
    let data = json!({ "after": {
        "source_id": source_id, "account_id": copy.account_id, "template": copy.template,
    } });
    audit::record(&mut tx, scope, actor, Action::CourseCopy, id, &data, now).await?;
    let course = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(course)
}

/// Copy every module and item of the course `from` into the new course `to`, both of the scope's
/// organization, with new ids and the same positions, titles and `published` flags; each item
/// keeps its kind and URL and goes into the copy of its module. Each module keeps its unlock and
/// completion rules and each item its requirement (spec 04 B1, B3), and the prerequisites are
/// copied between the copies of their modules ([`copy_prerequisites`]). The course files are
/// copied ([`copy_course_files`]) and each `file` item names its course file's copy. `created_at` and
/// `updated_at` are `now_us`. An item of a kind this release does not copy ([`items::Kind::copyable`]) is
/// [`Error::ContentNotCopyable`]: its `ref_id` names content no copy of this release copies. A
/// stored URL [`items::url_is_valid`] refuses is [`Error::CopiedUrlInvalid`], as every writer of
/// an item's URL checks it (spec 03, "B7's absolute").
async fn copy_outline(
    conn: &mut PgConnection,
    scope: OrgScope,
    (from, to): (Uuid, Uuid),
    now_us: i64,
    author: Option<Uuid>,
) -> Result<(), Error> {
    let sources = sqlx::query_scalar!(
        "SELECT id FROM modules WHERE org_id = $1 AND course_id = $2 ORDER BY position",
        scope.org_id(),
        from,
    )
    .fetch_all(&mut *conn)
    .await?;
    let copies: Vec<Uuid> = sources.iter().map(|_| new_id()).collect();
    sqlx::query!(
        "INSERT INTO modules (id, org_id, course_id, position, title, published, unlock_at,
                              require_sequential, completion_rule, completion_valid_months,
                              created_at, updated_at)
         SELECT map.new_id, $1, $2, m.position, m.title, m.published, m.unlock_at,
                m.require_sequential, m.completion_rule, m.completion_valid_months,
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         FROM UNNEST($3::uuid[], $4::uuid[]) AS map (old_id, new_id)
         JOIN modules m ON m.org_id = $1 AND m.course_id = $5 AND m.id = map.old_id",
        scope.org_id(),
        to,
        &sources,
        &copies,
        from,
        now_us,
    )
    .execute(&mut *conn)
    .await?;
    let new_ids: HashMap<Uuid, Uuid> = sources
        .iter()
        .copied()
        .zip(copies.iter().copied())
        .collect();
    copy_prerequisites(conn, scope, from, to, (&sources, &copies), now_us).await?;
    let files = copy_course_files(conn, scope, from, to, now_us).await?;
    let pages = crate::pages::copy_course_pages(
        conn,
        scope,
        (from, to),
        &files,
        author,
        from_epoch_micros(now_us)?,
    )
    .await
    .map_err(Error::Page)?;
    copy_home(conn, scope, to, &pages).await?;
    let refs: HashMap<Uuid, Uuid> = files.into_iter().chain(pages).collect();
    copy_items(conn, scope, from, to, (&new_ids, &refs), now_us).await
}

/// Point the copy `to`'s home at the copy of its source's home page, when it is a page (spec 07
/// B20); `pages` maps a source page's id to its copy's.
async fn copy_home(
    conn: &mut PgConnection,
    scope: OrgScope,
    to: Uuid,
    pages: &HashMap<Uuid, Uuid>,
) -> Result<(), Error> {
    let course = get(&mut *conn, scope, to).await?.ok_or(Error::NotFound)?;
    let Home::Page { page_id } = course.settings.home else {
        return Ok(());
    };
    let copied = pages.get(&page_id).copied().ok_or(Error::Stored {
        id: to,
        column: "settings",
    })?;
    let settings = CourseSettings {
        home: Home::Page { page_id: copied },
    };
    let settings = serde_json::to_string(&settings).map_err(|_| Error::Stored {
        id: to,
        column: "settings",
    })?;
    sqlx::query!(
        "UPDATE courses SET settings = $3::text::jsonb WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        to,
        settings,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// [`copy_outline`]'s course files (spec 06 B20): each course file of `from` as one of `to` in
/// the same folder under the same name, pointing at the same stored file, so no byte is added
/// and the storage limit is not charged again. Returns each source course file's id mapped to
/// its copy's.
async fn copy_course_files(
    conn: &mut PgConnection,
    scope: OrgScope,
    from: Uuid,
    to: Uuid,
    now_us: i64,
) -> Result<HashMap<Uuid, Uuid>, Error> {
    let sources = sqlx::query_scalar!(
        "SELECT id FROM course_files WHERE org_id = $1 AND course_id = $2",
        scope.org_id(),
        from,
    )
    .fetch_all(&mut *conn)
    .await?;
    let copies: Vec<Uuid> = sources.iter().map(|_| new_id()).collect();
    sqlx::query!(
        "INSERT INTO course_files (id, org_id, course_id, file_id, folder, name, created_at,
                                   updated_at)
         SELECT map.new_id, $1, $2, cf.file_id, cf.folder, cf.name,
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         FROM UNNEST($3::uuid[], $4::uuid[]) AS map (old_id, new_id)
         JOIN course_files cf ON cf.org_id = $1 AND cf.course_id = $5 AND cf.id = map.old_id",
        scope.org_id(),
        to,
        &sources,
        &copies,
        from,
        now_us,
    )
    .execute(&mut *conn)
    .await?;
    Ok(sources.into_iter().zip(copies).collect())
}

/// [`copy_outline`]'s prerequisites: each row of `from` as a row between the copies of its two
/// modules in `to`, `map` pairing each source module's id with its copy's. Both modules of a
/// row are modules of `from` (the table's foreign keys), so every row is copied; a count that
/// differs is a stored row this function cannot place.
async fn copy_prerequisites(
    conn: &mut PgConnection,
    scope: OrgScope,
    from: Uuid,
    to: Uuid,
    (sources, copies): (&[Uuid], &[Uuid]),
    now_us: i64,
) -> Result<(), Error> {
    let source_rows = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM module_prerequisites
           WHERE org_id = $1 AND course_id = $2"#,
        scope.org_id(),
        from,
    )
    .fetch_one(&mut *conn)
    .await?;
    let copied = sqlx::query!(
        "INSERT INTO module_prerequisites
             (org_id, course_id, module_id, requires_id, created_at, updated_at)
         SELECT $1, $2, mm.new_id, mr.new_id,
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
         FROM module_prerequisites p
         JOIN UNNEST($3::uuid[], $4::uuid[]) AS mm (old_id, new_id) ON mm.old_id = p.module_id
         JOIN UNNEST($3::uuid[], $4::uuid[]) AS mr (old_id, new_id) ON mr.old_id = p.requires_id
         WHERE p.org_id = $1 AND p.course_id = $5",
        scope.org_id(),
        to,
        sources,
        copies,
        from,
        now_us,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    if i64::try_from(copied).ok() != Some(source_rows) {
        return Err(Error::Stored {
            id: from,
            column: "module_prerequisites",
        });
    }
    Ok(())
}

/// [`copy_outline`]'s items: each into the copy of its module, `modules` mapping a source
/// module's id to its copy's; a `file` or `page` item names the copy of its course file or page,
/// `refs` mapping a source course file's or page's id to its copy's.
async fn copy_items(
    conn: &mut PgConnection,
    scope: OrgScope,
    from: Uuid,
    to: Uuid,
    (modules, refs): (&HashMap<Uuid, Uuid>, &HashMap<Uuid, Uuid>),
    now_us: i64,
) -> Result<(), Error> {
    let rows = sqlx::query!(
        "SELECT id, module_id, position, kind, ref_id, url, title, published,
                requirement::text AS requirement
         FROM module_items
         WHERE org_id = $1 AND course_id = $2 ORDER BY module_id, position",
        scope.org_id(),
        from,
    )
    .fetch_all(&mut *conn)
    .await?;
    if let Some(row) = rows
        .iter()
        .find(|r| !items::Kind::parse(&r.kind).is_some_and(items::Kind::copyable))
    {
        return Err(Error::ContentNotCopyable {
            item_id: row.id,
            kind: row.kind.clone(),
        });
    }
    if let Some(row) = rows.iter().find(|r| {
        r.url
            .as_deref()
            .is_some_and(|url| !items::url_is_valid(url))
    }) {
        return Err(Error::CopiedUrlInvalid { item_id: row.id });
    }
    // Every item's module is a module of `from` (the table's three-column foreign key), read in
    // this transaction under the course row lock that every outline write waits for, so the
    // lookup finds it; a miss is a stored row this function cannot place.
    let module_ids = rows
        .iter()
        .map(|r| {
            modules.get(&r.module_id).copied().ok_or(Error::Stored {
                id: r.id,
                column: "module_id",
            })
        })
        .collect::<Result<Vec<Uuid>, Error>>()?;
    // A `file` or `page` item's course file or page is one of `from` (checked when the item was
    // made, and one an item names cannot be deleted), copied above in this transaction; a miss
    // is a stored row this function cannot place. Copyable kinds name nothing else.
    let ref_ids = rows
        .iter()
        .map(|r| match r.ref_id {
            None => Ok(None),
            Some(ref_id) => refs.get(&ref_id).copied().map(Some).ok_or(Error::Stored {
                id: r.id,
                column: "ref_id",
            }),
        })
        .collect::<Result<Vec<Option<Uuid>>, Error>>()?;
    sqlx::query!(
        "INSERT INTO module_items (id, org_id, course_id, module_id, position, kind, ref_id, url,
                                   title, published, requirement, created_at, updated_at)
         SELECT i.id, $1, $2, i.module_id, i.position, i.kind, i.ref_id, i.url, i.title,
                i.published, i.requirement::jsonb,
                timestamptz 'epoch' + $10::bigint * interval '1 microsecond',
                timestamptz 'epoch' + $10::bigint * interval '1 microsecond'
         FROM UNNEST($3::uuid[], $4::uuid[], $5::int4[], $6::text[], $7::text[], $8::text[],
                     $9::bool[], $11::text[], $12::uuid[])
              AS i (id, module_id, position, kind, url, title, published, requirement, ref_id)",
        scope.org_id(),
        to,
        &rows.iter().map(|_| new_id()).collect::<Vec<_>>(),
        &module_ids,
        &rows.iter().map(|r| r.position).collect::<Vec<_>>(),
        &rows.iter().map(|r| r.kind.clone()).collect::<Vec<_>>(),
        &rows
            .iter()
            .map(|r| r.url.clone())
            .collect::<Vec<Option<String>>>() as &[Option<String>],
        &rows.iter().map(|r| r.title.clone()).collect::<Vec<_>>(),
        &rows.iter().map(|r| r.published).collect::<Vec<_>>(),
        now_us,
        &rows
            .iter()
            .map(|r| r.requirement.clone())
            .collect::<Vec<Option<String>>>() as &[Option<String>],
        &ref_ids as &[Option<Uuid>],
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// A course's changeable members, as [`update`] compares and writes them.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Details {
    title: String,
    code: String,
    start_at: Option<SystemTime>,
    end_at: Option<SystemTime>,
    settings: CourseSettings,
    completion_valid_months: Option<i16>,
}

impl Details {
    fn of(course: &Course) -> Self {
        Self {
            title: course.title.clone(),
            code: course.code.clone(),
            start_at: course.start_at,
            end_at: course.end_at,
            settings: course.settings,
            completion_valid_months: course.completion_valid_months,
        }
    }

    /// The details with `patch` applied, or why the patch is refused.
    fn patched(&self, patch: &CoursePatch) -> Result<Self, Error> {
        let date = |field, value: Option<Option<SystemTime>>, current| match value {
            Some(Some(time)) => valid_date(field, time).map(|()| Some(time)),
            Some(None) => Ok(None),
            None => Ok(current),
        };
        let after = Self {
            title: match &patch.title {
                Some(title) => valid_title(title)?.to_owned(),
                None => self.title.clone(),
            },
            code: match &patch.code {
                Some(code) => valid_code(code)?.to_owned(),
                None => self.code.clone(),
            },
            start_at: date(DateField::StartAt, patch.start_at, self.start_at)?,
            end_at: date(DateField::EndAt, patch.end_at, self.end_at)?,
            settings: CourseSettings {
                home: patch.home.unwrap_or(self.settings.home),
            },
            completion_valid_months: match patch.completion_valid_months {
                Some(Some(months)) if !completions::valid_months(months) => {
                    return Err(Error::ValidityOutOfRange);
                }
                Some(months) => months,
                None => self.completion_valid_months,
            },
        };
        if let (Some(start), Some(end)) = (after.start_at, after.end_at)
            && end < start
        {
            return Err(Error::DatesInverted);
        }
        Ok(after)
    }

    /// The `course.update` entry's data: each member that differs, before and after.
    fn changes(&self, after: &Self) -> Result<Value, Error> {
        let (mut old, mut new) = (Map::new(), Map::new());
        let mut put = |key: &str, before: Value, now: Value| {
            if before != now {
                old.insert(key.into(), before);
                new.insert(key.into(), now);
            }
        };
        put("title", json!(self.title), json!(after.title));
        put("code", json!(self.code), json!(after.code));
        put(
            "start_at",
            audit_time(self.start_at)?,
            audit_time(after.start_at)?,
        );
        put(
            "end_at",
            audit_time(self.end_at)?,
            audit_time(after.end_at)?,
        );
        put("settings", json!(self.settings), json!(after.settings));
        put(
            "completion_valid_months",
            json!(self.completion_valid_months),
            json!(after.completion_valid_months),
        );
        Ok(json!({ "before": old, "after": new }))
    }
}

/// Apply `patch` to the course `id` of the scope's organization, set `updated_at` to `now`, and
/// write the `course.update` audit entry by `actor` with each changed member before and after
/// (B3). A patch that changes nothing writes nothing and returns the course as it is (spec 09
/// B22).
///
/// Runs in one transaction (a savepoint when `db` is already one); the course is read with
/// [`lock`].
pub async fn update<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    patch: &CoursePatch,
    now: SystemTime,
    actor: Actor,
) -> Result<Course, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let course = lock(&mut tx, scope, id).await?.ok_or(Error::NotFound)?;
    let before = Details::of(&course);
    let after = before.patched(patch)?;
    if after == before {
        return Ok(course);
    }
    if let Home::Page { page_id } = after.settings.home {
        // The course row is locked, and a page's delete locks it too, so the page stays.
        let found = sqlx::query_scalar!(
            r#"SELECT EXISTS (SELECT 1 FROM pages WHERE org_id = $1 AND course_id = $2 AND id = $3)
               AS "found!""#,
            scope.org_id(),
            id,
            page_id,
        )
        .fetch_one(&mut *tx)
        .await?;
        if !found {
            return Err(Error::PageNotFound);
        }
    }
    let settings = serde_json::to_string(&after.settings).map_err(|_| Error::Stored {
        id,
        column: "settings",
    })?;
    sqlx::query!(
        "UPDATE courses
         SET title = $3, code = $4,
             start_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
             end_at = timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
             settings = $7::text::jsonb,
             updated_at = timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
             completion_valid_months = $9
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        after.title,
        after.code,
        after.start_at.map(epoch_micros).transpose()?,
        after.end_at.map(epoch_micros).transpose()?,
        settings,
        now,
        after.completion_valid_months,
    )
    .execute(&mut *tx)
    .await?;
    let data = before.changes(&after)?;
    audit::record(&mut tx, scope, actor, Action::CourseUpdate, id, &data, now).await?;
    let course = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(course)
}

/// Change the status of the course `id` of the scope's organization to `to`, set `updated_at`
/// to `now`, and write the `course.set_status` audit entry by `actor` with the status before
/// and after (B4). The current status writes nothing and returns the course as it is; a
/// template asked to publish is [`Error::Template`]; any other change B4 does not list is
/// [`Error::StatusInvalid`].
///
/// Runs in one transaction (a savepoint when `db` is already one); the course is read with
/// [`lock`].
pub async fn set_status<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    to: Status,
    now: SystemTime,
    actor: Actor,
) -> Result<Course, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let course = lock(&mut tx, scope, id).await?.ok_or(Error::NotFound)?;
    let from = course.status;
    if from == to {
        return Ok(course);
    }
    if course.template && to == Status::Published {
        return Err(Error::Template);
    }
    if !from.may_become(to) {
        return Err(Error::StatusInvalid {
            from: from.as_str(),
            to: to.as_str(),
        });
    }
    if to == Status::Draft && enrollments::has_learners(&mut tx, scope, id).await? {
        return Err(Error::HasLearners);
    }
    sqlx::query!(
        "UPDATE courses
         SET status = $3, updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        to.as_str(),
        now,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({ "before": { "status": from.as_str() }, "after": { "status": to.as_str() } });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::CourseSetStatus,
        id,
        &data,
        now,
    )
    .await?;
    let course = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(course)
}

/// Move the course `id` of the scope's organization, in any status, to the account
/// `account_id`, set `updated_at` to `now`, and write the `course.move` audit entry by `actor`
/// with the account before and after (B9). A move to the course's own account writes nothing
/// and returns the course as it is.
///
/// Runs in one transaction (a savepoint when `db` is already one) under the tree lock, as
/// [`create`] does.
pub async fn move_to<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    account_id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<Course, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    accounts::lock_tree(&mut tx, scope).await?;
    let course = lock(&mut tx, scope, id).await?.ok_or(Error::NotFound)?;
    accounts::ancestors_path(&mut *tx, scope, account_id)
        .await?
        .ok_or(Error::AccountNotFound)?;
    if course.account_id == account_id {
        return Ok(course);
    }
    sqlx::query!(
        "UPDATE courses
         SET account_id = $3,
             updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        account_id,
        now,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({
        "before": { "account_id": course.account_id },
        "after": { "account_id": account_id },
    });
    audit::record(&mut tx, scope, actor, Action::CourseMove, id, &data, now).await?;
    let moved = get(&mut *tx, scope, id).await?.ok_or(Error::NotFound)?;
    tx.commit().await?;
    Ok(moved)
}

/// The instant an RFC 3339 timestamp with an offset (`Z` or `±hh:mm`) names, or `None` when
/// `value` is not one. The API's course dates are read with it; [`update`] checks the range.
#[must_use]
pub fn parse_timestamp(value: &str) -> Option<SystemTime> {
    value.parse::<jiff::Timestamp>().ok().map(SystemTime::from)
}

/// A date as an audit entry records it: RFC 3339 in UTC, or `null`.
fn audit_time(time: Option<SystemTime>) -> Result<Value, Error> {
    let Some(time) = time else {
        return Ok(Value::Null);
    };
    let stamp = jiff::Timestamp::try_from(time).map_err(|_| TimeOutOfRange)?;
    Ok(json!(stamp.to_string()))
}

/// [`DATES_END_SECS`] as a time.
fn dates_end() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(DATES_END_SECS)
}

fn valid_date(field: DateField, time: SystemTime) -> Result<(), Error> {
    if time < UNIX_EPOCH || time >= dates_end() {
        return Err(Error::DateOutOfRange { field });
    }
    Ok(())
}

fn valid_title(title: &str) -> Result<&str, Error> {
    name::valid(title).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::TitleEmpty,
        name::Invalid::TooLong => Error::TitleTooLong,
        name::Invalid::ContainsNul => Error::ContainsNul { field: "title" },
    })
}

fn valid_code(code: &str) -> Result<&str, Error> {
    let code = no_nul("code", code)?.trim();
    if code.is_empty() {
        return Err(Error::CodeEmpty);
    }
    if code.chars().count() > MAX_CODE_CHARS {
        return Err(Error::CodeTooLong);
    }
    Ok(code)
}

/// `value` unchanged, or [`Error::ContainsNul`] naming `field`.
fn no_nul<'a>(field: &'static str, value: &'a str) -> Result<&'a str, Error> {
    if value.contains('\0') {
        return Err(Error::ContainsNul { field });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b4_allows_exactly_its_four_changes() {
        use Status::{Archived, Draft, Published};
        let all = [Draft, Published, Archived];
        let allowed = [
            (Draft, Published),
            (Published, Archived),
            (Archived, Published),
            (Published, Draft),
        ];
        for from in all {
            for to in all {
                assert_eq!(
                    from.may_become(to),
                    allowed.contains(&(from, to)),
                    "{from:?} to {to:?}"
                );
            }
        }
    }

    #[test]
    fn statuses_round_trip_their_names() {
        for status in [Status::Draft, Status::Published, Status::Archived] {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
        assert_eq!(Status::parse("Draft"), None);
    }

    #[test]
    fn settings_read_a_missing_home_as_the_module_list() {
        let empty: CourseSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.home, Home::Modules);
        assert_eq!(
            serde_json::to_value(CourseSettings::default()).unwrap(),
            json!({ "home": { "kind": "modules" } })
        );
        assert!(serde_json::from_str::<Home>(r#"{"kind":"page"}"#).is_err());
        assert_eq!(
            serde_json::from_str::<Home>(
                r#"{"kind":"page","page_id":"01900000-0000-7000-8000-000000000001"}"#
            )
            .unwrap(),
            Home::Page {
                page_id: Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001)
            }
        );
    }

    #[test]
    fn titles_and_codes_are_trimmed_and_bounded() {
        assert_eq!(valid_title("  S1 Ground ").unwrap(), "S1 Ground");
        assert!(matches!(valid_title(" "), Err(Error::TitleEmpty)));
        let over = "a".repeat(MAX_TITLE_CHARS + 1);
        assert!(matches!(valid_title(&over), Err(Error::TitleTooLong)));
        assert_eq!(valid_code(" S1 ").unwrap(), "S1");
        assert!(matches!(valid_code("\t"), Err(Error::CodeEmpty)));
        let at_limit = "é".repeat(MAX_CODE_CHARS);
        assert_eq!(valid_code(&at_limit).unwrap(), at_limit);
        let over = "a".repeat(MAX_CODE_CHARS + 1);
        assert!(matches!(valid_code(&over), Err(Error::CodeTooLong)));
        assert!(matches!(
            valid_code("S\0X"),
            Err(Error::ContainsNul { field: "code" })
        ));
    }

    #[test]
    fn timestamps_need_an_offset_and_dates_a_range() {
        let parsed = parse_timestamp("2026-01-02T03:04:05.123456Z").unwrap();
        assert_eq!(
            parsed.duration_since(UNIX_EPOCH).unwrap(),
            Duration::new(1_767_323_045, 123_456_000)
        );
        assert_eq!(
            parse_timestamp("2026-01-02T05:04:05+02:00"),
            parse_timestamp("2026-01-02T03:04:05Z")
        );
        for bad in ["", "2026-01-02", "2026-01-02T03:04:05", "tomorrow"] {
            assert_eq!(parse_timestamp(bad), None, "{bad}");
        }
        let early = parse_timestamp("1969-12-31T23:59:59Z").unwrap();
        assert!(valid_date(DateField::StartAt, early).is_err());
        assert!(valid_date(DateField::EndAt, dates_end()).is_err());
        let last = dates_end() - Duration::from_micros(1);
        assert!(valid_date(DateField::EndAt, last).is_ok());
        assert_eq!(parse_timestamp("2200-01-01T00:00:00Z"), Some(dates_end()));
    }

    fn details() -> Details {
        Details {
            title: "S1".into(),
            code: "S1".into(),
            start_at: None,
            end_at: None,
            settings: CourseSettings::default(),
            completion_valid_months: None,
        }
    }

    #[test]
    fn a_validity_of_1_to_120_months_is_set_and_null_clears_it() {
        let before = details();
        for months in [1, 12, 120] {
            let patch = CoursePatch {
                completion_valid_months: Some(Some(months)),
                ..CoursePatch::default()
            };
            let after = before.patched(&patch).unwrap();
            assert_eq!(after.completion_valid_months, Some(months));
            let clear = CoursePatch {
                completion_valid_months: Some(None),
                ..CoursePatch::default()
            };
            assert_eq!(after.patched(&clear).unwrap(), before);
            assert_eq!(after.patched(&CoursePatch::default()).unwrap(), after);
        }
        for months in [-1, 0, 121] {
            let patch = CoursePatch {
                completion_valid_months: Some(Some(months)),
                ..CoursePatch::default()
            };
            assert!(
                matches!(before.patched(&patch), Err(Error::ValidityOutOfRange)),
                "{months}"
            );
        }
        let mut after = before.clone();
        after.completion_valid_months = Some(12);
        assert_eq!(
            before.changes(&after).unwrap(),
            json!({
                "before": { "completion_valid_months": null },
                "after": { "completion_valid_months": 12 },
            })
        );
    }

    #[test]
    fn a_patch_of_current_values_changes_nothing_and_inverted_dates_are_refused() {
        let before = details();
        let same = CoursePatch {
            title: Some(" S1 ".into()),
            home: Some(Home::Modules),
            start_at: Some(None),
            ..CoursePatch::default()
        };
        assert_eq!(before.patched(&same).unwrap(), before);
        let start = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
        let inverted = CoursePatch {
            start_at: Some(Some(start)),
            end_at: Some(Some(start - Duration::from_secs(1))),
            ..CoursePatch::default()
        };
        assert!(matches!(
            before.patched(&inverted),
            Err(Error::DatesInverted)
        ));
        let equal = CoursePatch {
            start_at: Some(Some(start)),
            end_at: Some(Some(start)),
            ..CoursePatch::default()
        };
        assert!(before.patched(&equal).is_ok());
    }

    #[test]
    fn the_update_entry_lists_only_changed_members() {
        let before = details();
        let mut after = before.clone();
        after.title = "S1 Ground".into();
        after.end_at = Some(UNIX_EPOCH + Duration::from_secs(1_767_323_045));
        assert_eq!(
            before.changes(&after).unwrap(),
            json!({
                "before": { "title": "S1", "end_at": null },
                "after": { "title": "S1 Ground", "end_at": "2026-01-02T03:04:05Z" },
            })
        );
    }
}
