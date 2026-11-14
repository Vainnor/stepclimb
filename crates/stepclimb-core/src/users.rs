// SPDX-License-Identifier: AGPL-3.0-or-later
//! The users repository (docs/specs/m0/03-users-and-identities.md B1 to B8, B12).
//!
//! A user is one row in `users` and belongs to exactly one organization; every query here filters
//! by `org_id = scope.org_id()`. The repository validates what the spec states for every writer
//! (a non-blank display name, the supported locale, a trimmed email unique in the organization,
//! an IANA time zone, stored in the database's spelling), refuses NUL in every string it is given
//! (PostgreSQL `text` cannot hold one) and strings longer than their limit ([`MAX_NAME_CHARS`],
//! [`MAX_EMAIL_CHARS`], [`MAX_EXTERNAL_ID_CHARS`]), and leaves the
//! admin API's guards to the routes: refusing changes to the source-owned fields of a user an
//! outside source owns (B6) and suspending yourself are decisions about who asks, and the sync
//! API (M4) writes those same users through these functions.
//!
//! What the caller supplies:
//!
//! - `tz`, already resolved: its default is an organization setting (spec 03 Q4), so the column
//!   has no default. This module checks it against the time zone database ([`crate::tz`]).
//! - `now`, the request's clock reading, which `created_at` and `updated_at` are set from
//!   (spec 02 B5).
//!
//! `manager_id` and `profile` are columns this module neither reads nor writes: M0 never sets a
//! manager (B8) and defines no profile fields (B2).

use std::time::SystemTime;

use serde_json::json;
use sqlx::{Acquire, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope, Source,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name, role_assignments, tz,
};

/// The locale of a user created without one (B3).
pub const DEFAULT_LOCALE: &str = "en";

/// The locales a user may have. Stepclimb is English-only at 1.0 (B3).
pub const SUPPORTED_LOCALES: [&str; 1] = [DEFAULT_LOCALE];

/// The longest display, given or family name, in characters (Unicode scalar values), as for
/// organization and account names (spec 02 Q6).
pub const MAX_NAME_CHARS: usize = name::MAX_CHARS;

/// The longest email, in characters: the longest address an SMTP path can carry (RFC 5321
/// section 4.5.3.1.3, 256 octets with the angle brackets).
pub const MAX_EMAIL_CHARS: usize = 254;

/// The longest external id, in characters.
pub const MAX_EXTERNAL_ID_CHARS: usize = 200;

/// The unique index that keeps `external_id` unique within an organization (B5).
const EXTERNAL_ID_INDEX: &str = "users_external_id";

/// The unique index that keeps `lower(email)` unique within an organization (B12, Q1).
const EMAIL_INDEX: &str = "users_email";

/// Whether a user may sign in (B7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Active,
    /// Refused at sign-in, and every credential of theirs stops working on its next request.
    Suspended,
}

impl Status {
    /// Every status, in the order the check constraint lists them.
    pub const ALL: [Self; 2] = [Self::Active, Self::Suspended];

    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Suspended => "suspended",
        }
    }

    /// The status a stored spelling names, or `None` for any other string.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == stored)
    }
}

/// A user as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub id: Uuid,
    pub display_name: String,
    pub given_name: String,
    pub family_name: String,
    pub email: Option<String>,
    pub locale: String,
    pub tz: String,
    pub status: Status,
    pub source: Source,
    pub external_id: Option<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A user to create. The new user is `active`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewUser {
    /// Trimmed before it is stored; blank is refused.
    pub display_name: String,
    /// Stored as given; `''` when a provider sends none.
    pub given_name: String,
    /// Stored as given; `''` when a provider sends none.
    pub family_name: String,
    /// Trimmed before it is stored, its case kept; blank, or holding a control or Unicode format
    /// character, is refused. Unique within the organization, compared case-insensitively
    /// (B12). Not normalized (NFC or otherwise).
    pub email: Option<String>,
    /// [`DEFAULT_LOCALE`] when `None`; anything outside [`SUPPORTED_LOCALES`] is refused.
    pub locale: Option<String>,
    /// An IANA time zone the caller resolved (spec 03 Q4); stored in the database's spelling.
    pub tz: String,
    /// `Local` for an admin's user, `Idp` for one made at sign-in (B4).
    pub source: Source,
    /// Unique within the organization when set (B5).
    pub external_id: Option<String>,
}

/// A partial update of a user: `None` leaves a field as it is. The same validation as
/// [`NewUser`] applies to each field it sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserPatch {
    pub display_name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    /// `Some(None)` clears the email.
    pub email: Option<Option<String>>,
    pub locale: Option<String>,
    pub tz: Option<String>,
}

/// The filters of [`list`]; `None` filters nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListFilter {
    pub status: Option<Status>,
    pub source: Option<Source>,
    /// A case-insensitive prefix of the display name or the email (B10). `%`, `_` and `\` match
    /// themselves.
    pub q: Option<String>,
}

/// The sort key of a user in [`list`]: `(display_name, id)`.
pub type ListKey = (String, Uuid);

/// Why a users repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    NotFound,
    /// The display name is empty once trimmed.
    #[error("a display name cannot be empty")]
    DisplayNameEmpty,
    /// The email is empty once trimmed.
    #[error("an email cannot be empty; omit it instead")]
    EmailEmpty,
    /// The email holds a control character or a Unicode format character (such as U+200B).
    #[error("an email cannot contain control or invisible formatting characters")]
    EmailInvalidCharacter,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// A string is longer than its limit once trimmed (for the trimmed fields).
    #[error("{field} is longer than {max} characters")]
    TooLong { field: &'static str, max: usize },
    /// The time zone is no time zone of the IANA database ([`crate::tz`]).
    #[error("the time zone is not an IANA time zone")]
    TzUnknown,
    /// The locale is not one of [`SUPPORTED_LOCALES`].
    #[error("the locale is not supported")]
    LocaleUnsupported,
    /// Another user of the organization has this external id.
    #[error("the external id belongs to another user of the organization")]
    ExternalIdTaken,
    /// Another user of the organization has this email, compared case-insensitively.
    #[error("the email belongs to another user of the organization")]
    EmailTaken,
    /// A stored `status` or `source` this release does not know.
    #[error("the stored {column} {value:?} is not one this release knows")]
    UnknownStoredValue { column: &'static str, value: String },
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    /// Suspending the user would leave the organization without an active organization-scope
    /// Organization admin (spec 09 B14).
    #[error("the last active organization admin cannot be suspended")]
    LastOrgAdmin,
    /// The transaction runs at an isolation level other than `READ COMMITTED`, where the
    /// last-admin count could miss a concurrent change.
    #[error("users can be suspended only at READ COMMITTED isolation, not {found}")]
    IsolationLevel { found: String },
    /// Reading the organization's admins before a suspension failed otherwise.
    #[error("checking the organization's admins failed: {0}")]
    AdminCheck(#[source] Box<role_assignments::Error>),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<role_assignments::Error> for Error {
    /// The errors of [`role_assignments::refuse_suspending_last_org_admin`].
    fn from(error: role_assignments::Error) -> Self {
        use role_assignments::Error as E;
        match error {
            E::NotFound => Self::NotFound,
            E::LastOrgAdmin => Self::LastOrgAdmin,
            E::IsolationLevel { found } => Self::IsolationLevel { found },
            E::Database(error) => Self::Database(error),
            other => Self::AdminCheck(Box::new(other)),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(EXTERNAL_ID_INDEX) => Self::ExternalIdTaken,
            Some(EMAIL_INDEX) => Self::EmailTaken,
            _ => Self::Database(error),
        }
    }
}

/// A `users` row as the queries select it. `identities::find_user` builds one from its join.
pub(crate) struct Row {
    pub(crate) id: Uuid,
    pub(crate) display_name: String,
    pub(crate) given_name: String,
    pub(crate) family_name: String,
    pub(crate) email: Option<String>,
    pub(crate) locale: String,
    pub(crate) tz: String,
    pub(crate) status: String,
    pub(crate) source: String,
    pub(crate) external_id: Option<String>,
    pub(crate) created_at_us: i64,
    pub(crate) updated_at_us: i64,
}

impl TryFrom<Row> for User {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let status = Status::parse(&row.status).ok_or(Error::UnknownStoredValue {
            column: "status",
            value: row.status,
        })?;
        let source = Source::parse(&row.source).ok_or(Error::UnknownStoredValue {
            column: "source",
            value: row.source,
        })?;
        Ok(Self {
            id: row.id,
            display_name: row.display_name,
            given_name: row.given_name,
            family_name: row.family_name,
            email: row.email,
            locale: row.locale,
            tz: row.tz,
            status,
            source,
            external_id: row.external_id,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The user `id` of the scope's organization, or `None` when the organization has no such user.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<User>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, display_name, given_name, family_name, email, locale, tz, status, source,
                  external_id,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM users
           WHERE org_id = $1 AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(User::try_from)
    .transpose()
}

/// The scope's users matching `filter`, sorted by display name then id, starting after `after`
/// (the sort key of the previous page's last user) and at most `limit` of them.
///
/// `after` comes from a client's cursor: it only bounds a query already filtered by the scope.
/// A NUL in `q` or in the cursor's display name is refused with [`Error::ContainsNul`].
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    filter: &ListFilter,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<User>, Error> {
    let q = filter.q.as_deref().map(|q| no_nul("q", q)).transpose()?;
    let after_name = after.map(|(name, _)| no_nul("cursor", name)).transpose()?;
    let pattern = q.map(prefix_pattern);
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, display_name, given_name, family_name, email, locale, tz, status, source,
                  external_id,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM users
           WHERE org_id = $1
             AND ($2::text IS NULL OR status = $2)
             AND ($3::text IS NULL OR source = $3)
             AND ($4::text IS NULL OR display_name ILIKE $4 OR email ILIKE $4)
             AND ($5::text IS NULL OR (display_name, id) > ($5, $6::uuid))
           ORDER BY display_name, id
           LIMIT $7"#,
        scope.org_id(),
        filter.status.map(Status::as_str),
        filter.source.map(Source::as_str),
        pattern,
        after_name,
        after.map(|(_, id)| *id),
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(User::try_from).collect()
}

/// Create a user in the scope's organization, `active`, with `created_at` and `updated_at` set
/// to `now`.
pub async fn create(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user: &NewUser,
    now: SystemTime,
) -> Result<User, Error> {
    let user = valid_new_user(user)?;
    let now = epoch_micros(now)?;
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO users (id, org_id, external_id, source, display_name, given_name,
                              family_name, email, locale, tz, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10,
                   timestamptz 'epoch' + $11::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $11::bigint * interval '1 microsecond')
           RETURNING id, display_name, given_name, family_name, email, locale, tz, status, source,
                     external_id,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        new_id(),
        scope.org_id(),
        user.external_id,
        user.source.as_str(),
        user.display_name,
        user.given_name,
        user.family_name,
        user.email,
        user.locale,
        user.tz,
        now,
    )
    .fetch_one(db)
    .await?;
    User::try_from(row)
}

/// Apply `patch` to the user `id` of the scope's organization and set `updated_at` to `now`.
pub async fn update(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
    patch: &UserPatch,
    now: SystemTime,
) -> Result<User, Error> {
    let patch = valid_patch(patch)?;
    let now = epoch_micros(now)?;
    let row = sqlx::query_as!(
        Row,
        r#"UPDATE users
           SET display_name = COALESCE($3, display_name),
               given_name = COALESCE($4, given_name),
               family_name = COALESCE($5, family_name),
               email = CASE WHEN $6 THEN $7 ELSE email END,
               locale = COALESCE($8, locale),
               tz = COALESCE($9, tz),
               updated_at = timestamptz 'epoch' + $10::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2
           RETURNING id, display_name, given_name, family_name, email, locale, tz, status, source,
                     external_id,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        id,
        patch.display_name,
        patch.given_name,
        patch.family_name,
        patch.set_email,
        patch.email,
        patch.locale,
        patch.tz,
        now,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    User::try_from(row)
}

/// Set the status of the user `id` of the scope's organization, set `updated_at` to `now`, and
/// write the `user.set_status` audit entry by `actor` with the status before and after. The
/// status the user already has writes nothing and returns the user as they are (spec 09 B22).
///
/// Suspending keeps the organization's last active organization-scope Organization admin
/// ([`Error::LastOrgAdmin`], spec 09 B14): it takes the lock assignment removals take, so a
/// suspension and a removal cannot both pass the count, and refuses an isolation level above
/// `READ COMMITTED` ([`Error::IsolationLevel`]). Runs in one transaction (a savepoint when `db`
/// is already one).
pub async fn set_status<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    status: Status,
    now: SystemTime,
    actor: Actor,
) -> Result<User, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    if status == Status::Suspended {
        role_assignments::refuse_suspending_last_org_admin(&mut tx, scope, id).await?;
    }
    let before = sqlx::query_scalar!(
        "SELECT status FROM users WHERE org_id = $1 AND id = $2 FOR NO KEY UPDATE",
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    if before == status.as_str() {
        return get(&mut *tx, scope, id).await?.ok_or(Error::NotFound);
    }
    let row = sqlx::query_as!(
        Row,
        r#"UPDATE users
           SET status = $3,
               updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2
           RETURNING id, display_name, given_name, family_name, email, locale, tz, status, source,
                     external_id,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        id,
        status.as_str(),
        now,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = json!({ "before": { "status": before }, "after": { "status": row.status } });
    audit::record(&mut tx, scope, actor, Action::UserSetStatus, id, &data, now).await?;
    tx.commit().await?;
    User::try_from(row)
}

/// How many users count against the organization's `users` limit
/// (docs/specs/m0/10-entitlements-framework.md B10).
///
/// Every user of the organization counts, suspended ones included: unsuspending runs no limit
/// check, so a count of active users alone would let an organization pass its limit by
/// suspending, creating and unsuspending. Which users count per tier is M6's to decide.
pub async fn count_for_limit(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<i64, Error> {
    let count = sqlx::query_scalar!(
        r#"SELECT count(*) AS "count!" FROM users WHERE org_id = $1"#,
        scope.org_id(),
    )
    .fetch_one(db)
    .await?;
    Ok(count)
}

/// Every check [`create`] makes of `user` before it inserts, without a database: a caller that
/// must refuse an invalid user before it takes a lock (the users limit, spec 10 B10) runs it
/// first. [`create`] still makes the same checks itself.
pub fn validate(user: &NewUser) -> Result<(), Error> {
    valid_new_user(user).map(|_| ())
}

/// A [`NewUser`] after validation, borrowing from it.
struct ValidNewUser<'a> {
    display_name: &'a str,
    given_name: &'a str,
    family_name: &'a str,
    email: Option<&'a str>,
    locale: &'a str,
    tz: String,
    source: Source,
    external_id: Option<&'a str>,
}

fn valid_new_user(user: &NewUser) -> Result<ValidNewUser<'_>, Error> {
    Ok(ValidNewUser {
        display_name: valid_display_name(&user.display_name)?,
        given_name: valid_text("given_name", &user.given_name, MAX_NAME_CHARS)?,
        family_name: valid_text("family_name", &user.family_name, MAX_NAME_CHARS)?,
        email: user.email.as_deref().map(valid_email).transpose()?,
        locale: valid_locale(user.locale.as_deref().unwrap_or(DEFAULT_LOCALE))?,
        tz: valid_tz(&user.tz)?,
        source: user.source,
        external_id: user
            .external_id
            .as_deref()
            .map(|id| valid_text("external_id", id, MAX_EXTERNAL_ID_CHARS))
            .transpose()?,
    })
}

/// A [`UserPatch`] after validation, borrowing from it. `set_email` says whether `email` is
/// written at all, so `email: None` with `set_email` clears it.
struct ValidPatch<'a> {
    display_name: Option<&'a str>,
    given_name: Option<&'a str>,
    family_name: Option<&'a str>,
    set_email: bool,
    email: Option<&'a str>,
    locale: Option<&'a str>,
    tz: Option<String>,
}

fn valid_patch(patch: &UserPatch) -> Result<ValidPatch<'_>, Error> {
    Ok(ValidPatch {
        display_name: patch
            .display_name
            .as_deref()
            .map(valid_display_name)
            .transpose()?,
        given_name: optional("given_name", patch.given_name.as_ref())?,
        family_name: optional("family_name", patch.family_name.as_ref())?,
        set_email: patch.email.is_some(),
        email: patch
            .email
            .as_ref()
            .and_then(Option::as_deref)
            .map(valid_email)
            .transpose()?,
        locale: patch.locale.as_deref().map(valid_locale).transpose()?,
        tz: patch.tz.as_deref().map(valid_tz).transpose()?,
    })
}

/// [`valid_text`] for a name field a patch may leave out.
fn optional<'a>(field: &'static str, value: Option<&'a String>) -> Result<Option<&'a str>, Error> {
    value
        .map(|v| valid_text(field, v, MAX_NAME_CHARS))
        .transpose()
}

/// `value` unchanged when it holds no NUL and at most `max` characters.
fn valid_text<'a>(field: &'static str, value: &'a str, max: usize) -> Result<&'a str, Error> {
    let value = no_nul(field, value)?;
    if value.chars().count() > max {
        return Err(Error::TooLong { field, max });
    }
    Ok(value)
}

/// The time zone database's spelling of `name` (`america/new_york` is `America/New_York`).
fn valid_tz(name: &str) -> Result<String, Error> {
    no_nul("tz", name)?;
    tz::canonical(name).ok_or(Error::TzUnknown)
}

/// `value` unchanged, or [`Error::ContainsNul`] naming `field`.
fn no_nul<'a>(field: &'static str, value: &'a str) -> Result<&'a str, Error> {
    if value.contains('\0') {
        return Err(Error::ContainsNul { field });
    }
    Ok(value)
}

fn valid_display_name(name: &str) -> Result<&str, Error> {
    let name = no_nul("display_name", name)?.trim();
    if name.is_empty() {
        return Err(Error::DisplayNameEmpty);
    }
    valid_text("display_name", name, MAX_NAME_CHARS)
}

fn valid_email(email: &str) -> Result<&str, Error> {
    let email = no_nul("email", email)?.trim();
    if email.is_empty() {
        return Err(Error::EmailEmpty);
    }
    valid_text("email", email, MAX_EMAIL_CHARS)?;
    if email.chars().any(|c| c.is_control() || is_format_char(c)) {
        return Err(Error::EmailInvalidCharacter);
    }
    Ok(email)
}

/// Unicode's format characters (general category Cf), invisible in most renderings: soft
/// hyphen, zero-width space and joiners, bidirectional controls, the byte order mark, tags.
/// Taken from the Unicode 16.0 character database (Python's `unicodedata`); a character a later
/// version adds to Cf is not refused until this table is regenerated. To regenerate it, scan every
/// code point for category `Cf`, collapse runs into ranges and note the database version:
///
/// ```text
/// python3 -I -c "import unicodedata as u; r=[]
/// for c in range(0x110000):
///     if u.category(chr(c)) == 'Cf':
///         if r and r[-1][1] == c - 1: r[-1][1] = c
///         else: r.append([c, c])
/// print(u.unidata_version); print(*(f'(0x{a:04X}, 0x{b:04X}),' for a, b in r), sep='\n')"
/// ```
const FORMAT_CHARS: [(u32, u32); 21] = [
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE0001),
    (0xE0020, 0xE007F),
];

fn is_format_char(c: char) -> bool {
    let c = u32::from(c);
    FORMAT_CHARS
        .iter()
        .any(|&(first, last)| (first..=last).contains(&c))
}

fn valid_locale(locale: &str) -> Result<&str, Error> {
    if SUPPORTED_LOCALES.contains(&locale) {
        Ok(locale)
    } else {
        Err(Error::LocaleUnsupported)
    }
}

/// An `ILIKE` pattern matching values that start with `prefix`, with `ILIKE`'s own wildcards and
/// its escape character in `prefix` matching only themselves.
fn prefix_pattern(prefix: &str) -> String {
    let mut pattern = String::with_capacity(prefix.len() + 1);
    for c in prefix.chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_are_trimmed_and_blank_is_refused() {
        assert_eq!(valid_display_name("  Sam Ortiz \n").unwrap(), "Sam Ortiz");
        for name in ["", "   ", "\t\n"] {
            assert!(
                matches!(valid_display_name(name), Err(Error::DisplayNameEmpty)),
                "{name:?}"
            );
        }
    }

    #[test]
    fn emails_are_trimmed_and_blank_is_refused() {
        assert_eq!(
            valid_email(" Sam@Example.test ").unwrap(),
            "Sam@Example.test"
        );
        assert!(matches!(valid_email("  "), Err(Error::EmailEmpty)));
    }

    #[test]
    fn nul_is_refused_in_every_validated_string() {
        assert!(matches!(
            valid_display_name("Sam\0"),
            Err(Error::ContainsNul {
                field: "display_name"
            })
        ));
        assert!(matches!(
            valid_email("sam\0@example.test"),
            Err(Error::ContainsNul { field: "email" })
        ));
        assert!(matches!(
            valid_tz("UTC\0"),
            Err(Error::ContainsNul { field: "tz" })
        ));
        assert_eq!(no_nul("tz", "UTC").unwrap(), "UTC");
    }

    #[test]
    fn emails_with_control_or_format_characters_are_refused() {
        for email in [
            "sam\u{200B}@example.test",
            "\u{FEFF}sam@example.test",
            "sam@exa\u{00AD}mple.test",
            "sam@example.test\u{202E}",
            "sam\u{0007}@example.test",
            "sam\r\n@example.test",
            "sam@example.test\u{E0041}",
        ] {
            assert!(
                matches!(valid_email(email), Err(Error::EmailInvalidCharacter)),
                "{email:?}"
            );
        }
        assert_eq!(
            valid_email("sam.o'neil+tag@example.test").unwrap(),
            "sam.o'neil+tag@example.test"
        );
        assert_eq!(
            valid_email("josé@exämple.test").unwrap(),
            "josé@exämple.test"
        );
    }

    #[test]
    fn the_format_table_is_sorted_and_its_ranges_are_well_formed() {
        assert!(FORMAT_CHARS.iter().all(|&(first, last)| first <= last));
        assert!(FORMAT_CHARS.windows(2).all(|pair| pair[0].1 < pair[1].0));
        assert!(is_format_char('\u{200B}'));
        assert!(!is_format_char('a'));
        assert!(
            !is_format_char('\u{2065}'),
            "unassigned, between two Cf ranges"
        );
    }

    #[test]
    fn time_zones_are_checked_and_stored_in_the_databases_spelling() {
        assert_eq!(valid_tz("america/new_york").unwrap(), "America/New_York");
        assert_eq!(valid_tz("UTC").unwrap(), "UTC");
        for name in ["Mars/Olympus_Mons", "", "Etc/Unknown", "+05:00"] {
            assert!(matches!(valid_tz(name), Err(Error::TzUnknown)), "{name:?}");
        }
    }

    #[test]
    fn the_length_limits_count_characters() {
        let at_limit = "é".repeat(MAX_NAME_CHARS);
        assert_eq!(
            valid_display_name(&format!(" {at_limit} ")).unwrap(),
            at_limit
        );
        let over = "a".repeat(MAX_NAME_CHARS + 1);
        for (field, result) in [
            ("display_name", valid_display_name(&over)),
            (
                "given_name",
                valid_text("given_name", &over, MAX_NAME_CHARS),
            ),
        ] {
            assert!(
                matches!(result, Err(Error::TooLong { field: f, max: MAX_NAME_CHARS }) if f == field),
                "{field}"
            );
        }
        let local = "a".repeat(MAX_EMAIL_CHARS - "@example.test".len());
        let email = format!("{local}@example.test");
        assert_eq!(valid_email(&email).unwrap(), email);
        assert!(matches!(
            valid_email(&format!("a{email}")),
            Err(Error::TooLong {
                field: "email",
                max: MAX_EMAIL_CHARS
            })
        ));
    }

    #[test]
    fn only_english_is_a_supported_locale() {
        assert_eq!(valid_locale("en").unwrap(), "en");
        for locale in ["fr", "EN", "en-US", ""] {
            assert!(
                matches!(valid_locale(locale), Err(Error::LocaleUnsupported)),
                "{locale:?}"
            );
        }
    }

    #[test]
    fn prefix_patterns_escape_the_wildcards() {
        assert_eq!(prefix_pattern("sa"), "sa%");
        assert_eq!(prefix_pattern("50%_a\\b"), "50\\%\\_a\\\\b%");
        assert_eq!(prefix_pattern(""), "%");
    }

    #[test]
    fn every_status_round_trips_through_its_stored_spelling() {
        for status in Status::ALL {
            assert_eq!(Status::parse(status.as_str()), Some(status));
        }
        assert_eq!(Status::parse("deleted"), None);
    }
}
