// SPDX-License-Identifier: AGPL-3.0-or-later
//! Roles: the eight built-in roles and their permission lists, and the roles repository
//! (docs/specs/m0/09-permissions-and-roles.md B4 to B10).
//!
//! Organization admin is defined as [`perm::ALL`] and Instructor as Course designer's list plus
//! its own, so a permission added to the catalog or to Course designer reaches them without a
//! second edit.
//!
//! A role is one row in `roles`. Built-in roles are instance-wide rows (`org_id` null) that
//! [`sync_builtin`] writes from [`BUILTIN`] at startup; custom roles belong to one organization.
//! Every read returns a built-in role's permissions from [`BUILTIN`], never from its row (B6),
//! so two binaries of different versions each apply their own lists. Every other query filters
//! by `org_id = scope.org_id()`, and reads see the scope's custom roles and the built-in rows.
//!
//! Who may define a role with which permissions (B8's escalation rule) is the routes' to
//! decide; this module checks the role itself: the key format, a key no other role of the
//! organization or built-in role has, a non-blank name of at most [`MAX_NAME_CHARS`]
//! characters, and permissions in [`perm::ALL`], each at most once.

use std::{collections::BTreeSet, time::SystemTime};

use serde_json::{Map, Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    name,
    perm::{self, Permission},
};

/// A role shipped with Stepclimb, the same in every organization.
#[derive(Debug)]
pub struct BuiltinRole {
    /// The stable key, in the custom role key format (`^[a-z][a-z0-9_]{0,62}$`).
    pub key: &'static str,
    /// The display name.
    pub name: &'static str,
    permissions: RolePermissions,
}

#[derive(Debug)]
enum RolePermissions {
    /// Every entry in [`perm::ALL`].
    All,
    /// Exactly these keys.
    Keys(&'static [&'static str]),
    /// Another built-in role's permissions, then these keys.
    Extends {
        base: &'static BuiltinRole,
        own: &'static [&'static str],
    },
}

impl BuiltinRole {
    /// The role's permission keys: a base role's first, then its own, in definition order.
    pub fn permissions(&self) -> Vec<&'static str> {
        match self.permissions {
            RolePermissions::All => perm::ALL.iter().map(|p| p.key()).collect(),
            RolePermissions::Keys(keys) => keys.to_vec(),
            RolePermissions::Extends { base, own } => {
                let mut keys = base.permissions();
                keys.extend_from_slice(own);
                keys
            }
        }
    }
}

static ORG_ADMIN: BuiltinRole = BuiltinRole {
    key: "org_admin",
    name: "Organization admin",
    permissions: RolePermissions::All,
};

/// Leaves out `user.manage` and `role.manage`, which are grantable only at organization scope
/// (spec 09 Q2, Q3).
static ACCOUNT_ADMIN: BuiltinRole = BuiltinRole {
    key: "account_admin",
    name: "Account admin",
    permissions: RolePermissions::Keys(&[
        perm::AccountView::KEY,
        perm::AccountManage::KEY,
        perm::UserView::KEY,
        perm::RoleView::KEY,
        perm::RoleAssign::KEY,
        perm::CourseView::KEY,
        perm::CourseViewAll::KEY,
        perm::CourseCreate::KEY,
        perm::CourseManage::KEY,
        perm::CourseContentEdit::KEY,
        perm::EnrollmentView::KEY,
        perm::EnrollmentManage::KEY,
        perm::ProgressView::KEY,
        perm::AssignmentManage::KEY,
    ]),
};

static COURSE_DESIGNER: BuiltinRole = BuiltinRole {
    key: "course_designer",
    name: "Course designer",
    permissions: RolePermissions::Keys(&[
        perm::CourseView::KEY,
        perm::CourseViewAll::KEY,
        perm::CourseContentEdit::KEY,
    ]),
};

/// `user.view` lets an Instructor enroll the users their assignment's scope covers (M1 spec 02
/// B17, owner decision of 2026-11-17); it is not grantable at course scope, so an Instructor
/// enrollment does not give it.
static INSTRUCTOR: BuiltinRole = BuiltinRole {
    key: "instructor",
    name: "Instructor",
    permissions: RolePermissions::Extends {
        base: &COURSE_DESIGNER,
        own: &[
            perm::UserView::KEY,
            perm::EnrollmentView::KEY,
            perm::EnrollmentManage::KEY,
            perm::ProgressView::KEY,
            perm::AssignmentManage::KEY,
        ],
    },
};

static GRADER: BuiltinRole = BuiltinRole {
    key: "grader",
    name: "Grader",
    permissions: RolePermissions::Keys(&[]),
};

static STUDENT: BuiltinRole = BuiltinRole {
    key: "student",
    name: "Student",
    permissions: RolePermissions::Keys(&[perm::CourseView::KEY, perm::CourseParticipate::KEY]),
};

static MANAGER: BuiltinRole = BuiltinRole {
    key: "manager",
    name: "Manager",
    permissions: RolePermissions::Keys(&[]),
};

static OBSERVER: BuiltinRole = BuiltinRole {
    key: "observer",
    name: "Observer",
    permissions: RolePermissions::Keys(&[perm::CourseView::KEY, perm::ProgressViewObserved::KEY]),
};

/// Every built-in role.
pub static BUILTIN: [&BuiltinRole; 8] = [
    &ORG_ADMIN,
    &ACCOUNT_ADMIN,
    &COURSE_DESIGNER,
    &INSTRUCTOR,
    &GRADER,
    &STUDENT,
    &MANAGER,
    &OBSERVER,
];

/// The built-in role with this key, if this binary defines one.
#[must_use]
pub fn builtin(key: &str) -> Option<&'static BuiltinRole> {
    BUILTIN.iter().copied().find(|role| role.key == key)
}

/// The permissions a role row grants: a built-in role's list from [`BUILTIN`], never its
/// stored one (B6), and none for a built-in row this binary does not define; a custom role's
/// list as stored.
pub(crate) fn effective_permissions(key: &str, builtin: bool, stored: Vec<String>) -> Vec<String> {
    if !builtin {
        return stored;
    }
    self::builtin(key)
        .map(|role| role.permissions().into_iter().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The ids of the roles the scope's organization can use (its custom roles and the built-in
/// rows) whose effective permissions include `key`, for queries that ask which enrollments make
/// a learner or an Observer (M1 spec 05 B14, B20). Reads lock nothing.
pub(crate) async fn ids_holding(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    key: &str,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let rows = sqlx::query!(
        "SELECT id, key, builtin, permissions FROM roles WHERE org_id IS NULL OR org_id = $1",
        scope.org_id(),
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|role| {
            effective_permissions(&role.key, role.builtin, role.permissions.clone())
                .iter()
                .any(|held| held == key)
        })
        .map(|role| role.id)
        .collect())
}

/// The longest role name, in characters (Unicode scalar values): the organization and account
/// name rule (coordinator decision, 13 Nov 2026).
pub const MAX_NAME_CHARS: usize = name::MAX_CHARS;

/// The longest role key, in characters (spec 09 Q7).
pub const MAX_KEY_CHARS: usize = 63;

/// The partial unique index that keeps a custom role's key unique in its organization.
const ORG_KEY_INDEX: &str = "roles_org_key";

/// The foreign key from `role_assignments` to `roles`, whose default action refuses deleting a
/// role that still has assignments (B10).
const ASSIGNMENT_ROLE_FKEY: &str = "role_assignments_role_id_fkey";

/// The foreign key from `enrollments` to `roles`, which refuses deleting a role that still has
/// enrollments, as B10 does for assignments (M1 spec 02).
const ENROLLMENT_ROLE_FKEY: &str = "enrollments_role_id_fkey";

/// A role as the repository reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    pub id: Uuid,
    pub key: String,
    pub name: String,
    pub builtin: bool,
    /// A built-in role's list from [`BUILTIN`] (empty for a built-in row this binary does not
    /// define, which a newer binary wrote); a custom role's list as stored.
    pub permissions: Vec<String>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// A custom role to create.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewRole {
    /// `^[a-z][a-z0-9_]{0,62}$`, and not a built-in key (Q7, B8).
    pub key: String,
    /// Trimmed before it is stored; blank or longer than [`MAX_NAME_CHARS`] is refused.
    pub name: String,
    /// Every entry must be a key in [`perm::ALL`], each at most once. Stored in the given order.
    pub permissions: Vec<String>,
}

/// A partial update of a custom role: `None` leaves a field as it is. The same validation as
/// [`NewRole`] applies to each field it sets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RolePatch {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

/// The sort key of a role in [`list`]: `(custom, key)`, so built-in roles come first.
pub type ListKey = (bool, String);

/// Why a roles repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No built-in role and no custom role of the scope's organization with that id.
    #[error("the role does not exist")]
    NotFound,
    /// The key is not `^[a-z][a-z0-9_]{0,62}$`.
    #[error(
        "a role key must be a lowercase letter then up to 62 lowercase letters, digits or underscores"
    )]
    KeyInvalid,
    /// A built-in role or another custom role of the organization has this key.
    #[error("another role has this key")]
    KeyTaken,
    /// The name is empty once trimmed.
    #[error("a role name cannot be empty")]
    NameEmpty,
    /// The name is longer than [`MAX_NAME_CHARS`] once trimmed.
    #[error("a role name is longer than {MAX_NAME_CHARS} characters")]
    NameTooLong,
    /// A string the caller passed holds NUL, which PostgreSQL `text` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// The permission at `index` is not a key in [`perm::ALL`].
    #[error("permission {index} is not in the catalog")]
    UnknownPermission { index: usize },
    /// The permission at `index` repeats an earlier entry of the list. Refusing repeats bounds a
    /// valid list at [`perm::ALL`]'s length.
    #[error("permission {index} repeats an earlier entry")]
    DuplicatePermission { index: usize },
    /// A built-in role [`sync_builtin`] would add has the key of a custom role of `org_id`.
    #[error(
        "the built-in role key {key:?} is already a custom role's key in organization {org_id}"
    )]
    BuiltinKeyConflict { org_id: Uuid, key: String },
    /// Built-in roles cannot be changed or deleted (B7).
    #[error("a built-in role cannot be changed or deleted")]
    BuiltinImmutable,
    /// The custom role still has assignments or enrollments (B10; M1 spec 02).
    #[error("a role with assignments or enrollments cannot be deleted")]
    InUse,
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(ORG_KEY_INDEX) => Self::KeyTaken,
            Some(ASSIGNMENT_ROLE_FKEY | ENROLLMENT_ROLE_FKEY) => Self::InUse,
            _ => Self::Database(error),
        }
    }
}

/// A `roles` row as the queries select it.
struct Row {
    id: Uuid,
    key: String,
    name: String,
    builtin: bool,
    permissions: Vec<String>,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Role {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let permissions = effective_permissions(&row.key, row.builtin, row.permissions);
        Ok(Self {
            id: row.id,
            key: row.key,
            name: row.name,
            builtin: row.builtin,
            permissions,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// Write every role of [`BUILTIN`] to its instance-wide row by key, inserting the missing ones
/// and rewriting `name` and `permissions` where they differ, so SQL readers see this binary's
/// lists (B6). `updated_at` moves to `now` only on a row it changes. Built-in rows this binary
/// does not define are left as they are. Runs in one transaction (a savepoint when `db` is
/// already one).
///
/// Before inserting a built-in row that does not exist yet, it refuses with
/// [`Error::BuiltinKeyConflict`], writing nothing, when a custom role of any organization has
/// that key (the role a later release adds collides with one an organization defined).
///
/// The startup task of `stepclimb serve` calls this before serving, and the server does not
/// start while it fails.
pub async fn sync_builtin<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    now: SystemTime,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    for role in BUILTIN {
        refuse_custom_holder(&mut tx, role.key).await?;
        let permissions: Vec<String> = role.permissions().into_iter().map(str::to_owned).collect();
        // Instance-wide on purpose, so it takes no `OrgScope`: built-in roles belong to no
        // organization (`org_id` null), and the conflict target is the built-in key index.
        sqlx::query!(
            "INSERT INTO roles (id, org_id, key, name, builtin, permissions, created_at, updated_at)
             VALUES ($1, NULL, $2, $3, true, $4,
                     timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                     timestamptz 'epoch' + $5::bigint * interval '1 microsecond')
             ON CONFLICT (key) WHERE org_id IS NULL DO UPDATE
             SET name = EXCLUDED.name, permissions = EXCLUDED.permissions,
                 updated_at = EXCLUDED.updated_at
             WHERE (roles.name, roles.permissions)
                   IS DISTINCT FROM (EXCLUDED.name, EXCLUDED.permissions)",
            new_id(),
            role.key,
            role.name,
            &permissions,
            now,
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// [`Error::BuiltinKeyConflict`] when no built-in row has `key` yet and a custom role does.
async fn refuse_custom_holder(conn: &mut PgConnection, key: &str) -> Result<(), Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: a new built-in key must be free in
    // every organization. It returns the first such organization's id for the error, never a
    // row's contents.
    let holder = sqlx::query_scalar!(
        r#"SELECT org_id AS "org_id!" FROM roles
           WHERE org_id IS NOT NULL AND key = $1
             AND NOT EXISTS (SELECT 1 FROM roles WHERE org_id IS NULL AND key = $1)
           ORDER BY org_id
           LIMIT 1"#,
        key,
    )
    .fetch_optional(conn)
    .await?;
    match holder {
        Some(org_id) => Err(Error::BuiltinKeyConflict {
            org_id,
            key: key.to_owned(),
        }),
        None => Ok(()),
    }
}

/// The role `id` when it is built-in or a custom role of the scope's organization, else `None`.
pub async fn get(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Role>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, key, name, builtin, permissions,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM roles
           WHERE (org_id IS NULL OR org_id = $1) AND id = $2"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .map(Role::try_from)
    .transpose()
}

/// The id of the built-in role row with `key`, or `None` when [`sync_builtin`] has not written
/// one. `stepclimb admin create` reads the Organization admin role's id with it to assign it to
/// a new organization's first user (spec 02 B27).
pub async fn builtin_id(db: impl PgExecutor<'_>, key: &str) -> Result<Option<Uuid>, Error> {
    // Instance-wide on purpose, so it takes no `OrgScope`: built-in roles belong to no
    // organization (`org_id` null), and this returns only the id of such a row.
    let id = sqlx::query_scalar!(
        "SELECT id FROM roles WHERE org_id IS NULL AND key = $1",
        no_nul("key", key)?,
    )
    .fetch_optional(db)
    .await?;
    Ok(id)
}

/// [`get`], with the role's row locked `FOR KEY SHARE` until the caller's transaction ends: a
/// change of the role through [`lock_custom`] waits for it, so a check on the permissions read
/// here holds until the caller commits (spec 09 B12 step 5, B14).
pub async fn get_for_key_share(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Option<Role>, Error> {
    sqlx::query_as!(
        Row,
        r#"SELECT id, key, name, builtin, permissions,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM roles
           WHERE (org_id IS NULL OR org_id = $1) AND id = $2
           FOR KEY SHARE"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .map(Role::try_from)
    .transpose()
}

/// Lock `FOR KEY SHARE`, until the caller's transaction ends, every custom role of the scope's
/// organization that the user `user_id` holds an assignment or an enrollment of, in id order. A change of one of
/// them through [`lock_custom`] (and so [`update_custom`]) waits for the caller to commit, so a
/// check on the user's grants read after this holds until then (the user routes' escalation
/// rule, spec 03 B6). Built-in roles change only when startup syncs them and are not locked.
pub async fn key_share_custom_of_user(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<(), Error> {
    sqlx::query_scalar!(
        "SELECT r.id FROM roles r
         WHERE r.org_id = $1
           AND r.id IN (SELECT a.role_id FROM role_assignments a
                        WHERE a.org_id = $1 AND a.user_id = $2
                        UNION
                        SELECT e.role_id FROM enrollments e
                        WHERE e.org_id = $1 AND e.user_id = $2)
         ORDER BY r.id
         FOR KEY SHARE",
        scope.org_id(),
        user_id,
    )
    .fetch_all(conn)
    .await?;
    Ok(())
}

/// The built-in roles, then the scope's custom roles, each group sorted by key, starting after
/// `after` (the sort key of the previous page's last role) and at most `limit` of them.
///
/// `after` comes from a client's cursor: it only bounds a query already filtered by the scope.
/// A NUL in its key is refused with [`Error::ContainsNul`].
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    after: Option<&ListKey>,
    limit: i64,
) -> Result<Vec<Role>, Error> {
    let after_key = after.map(|(_, key)| no_nul("cursor", key)).transpose()?;
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, key, name, builtin, permissions,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM roles
           WHERE (org_id IS NULL OR org_id = $1)
             AND ($2::bool IS NULL OR (org_id IS NOT NULL, key) > ($2, $3::text))
           ORDER BY org_id IS NOT NULL, key
           LIMIT $4"#,
        scope.org_id(),
        after.map(|(custom, _)| *custom),
        after_key,
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Role::try_from).collect()
}

/// Create a custom role in the scope's organization, with `created_at` and `updated_at` set to
/// `now` (B8), and its `role.create` audit entry by `actor`.
///
/// Runs in one transaction (a savepoint when `db` is already one).
pub async fn create_custom<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    role: &NewRole,
    now: SystemTime,
    actor: Actor,
) -> Result<Role, Error> {
    valid_key(&role.key)?;
    let name = valid_name(&role.name)?;
    valid_permissions(&role.permissions)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    // The NOT EXISTS refuses a key a built-in row has, also one only a newer binary defines.
    let row = sqlx::query_as!(
        Row,
        r#"INSERT INTO roles (id, org_id, key, name, builtin, permissions, created_at, updated_at)
           SELECT $1, $2, $3, $4, false, $5,
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond',
                  timestamptz 'epoch' + $6::bigint * interval '1 microsecond'
           WHERE NOT EXISTS (SELECT 1 FROM roles WHERE org_id IS NULL AND key = $3)
           RETURNING id, key, name, builtin, permissions,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        new_id(),
        scope.org_id(),
        role.key,
        name,
        &role.permissions,
        now,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::KeyTaken)?;
    let data = json!({ "after": {
        "key": row.key, "name": row.name, "permissions": row.permissions,
    }});
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::RoleCreate,
        row.id,
        &data,
        now,
    )
    .await?;
    tx.commit().await?;
    Role::try_from(row)
}

/// Change the `name` or `permissions` of the custom role `id` of the scope's organization, set
/// `updated_at` to `now` (B9), and write the `role.update` audit entry by `actor` with the
/// changed members before and after. A built-in role is refused with
/// [`Error::BuiltinImmutable`]. A patch that names nothing, or only values the role already has
/// (its trimmed name, the same set of permissions), writes nothing and returns the role as it
/// is (B22).
///
/// Runs in one transaction (a savepoint when `db` is already one).
pub async fn update_custom<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    patch: &RolePatch,
    now: SystemTime,
    actor: Actor,
) -> Result<Role, Error> {
    let name = patch.name.as_deref().map(valid_name).transpose()?;
    if let Some(permissions) = &patch.permissions {
        valid_permissions(permissions)?;
    }
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let before = lock_custom(&mut tx, scope, id).await?;
    if !changes(&before, name, patch.permissions.as_deref()) {
        return Ok(before);
    }
    let row = sqlx::query_as!(
        Row,
        r#"UPDATE roles
           SET name = COALESCE($3, name), permissions = COALESCE($4, permissions),
               updated_at = timestamptz 'epoch' + $5::bigint * interval '1 microsecond'
           WHERE org_id = $1 AND id = $2
           RETURNING id, key, name, builtin, permissions,
                     (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                     (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!""#,
        scope.org_id(),
        id,
        name,
        patch.permissions.as_deref(),
        now,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = update_data(&before, &row);
    audit::record(&mut tx, scope, actor, Action::RoleUpdate, id, &data, now).await?;
    tx.commit().await?;
    Role::try_from(row)
}

/// Whether a patch to `role` with the valid `name` and `permissions` changes anything. The
/// permissions compare as sets: [`valid_permissions`] refused repeats, and an order change
/// grants nothing new.
fn changes(role: &Role, name: Option<&str>, permissions: Option<&[String]>) -> bool {
    let renamed = name.is_some_and(|name| name != role.name);
    let regranted = permissions.is_some_and(|keys| {
        let new: BTreeSet<&String> = keys.iter().collect();
        new != role.permissions.iter().collect()
    });
    renamed || regranted
}

/// The `role.update` entry's data: each member the update changed, before and after.
fn update_data(before: &Role, after: &Row) -> Value {
    let (mut old, mut new) = (Map::new(), Map::new());
    if before.name != after.name {
        old.insert("name".into(), json!(before.name));
        new.insert("name".into(), json!(after.name));
    }
    if before.permissions != after.permissions {
        old.insert("permissions".into(), json!(before.permissions));
        new.insert("permissions".into(), json!(after.permissions));
    }
    json!({ "before": old, "after": new })
}

/// Delete the custom role `id` of the scope's organization and write its `role.delete` audit
/// entry by `actor` at `now`, with the role as it was. A built-in role is refused with
/// [`Error::BuiltinImmutable`], a role with any assignment or enrollment with [`Error::InUse`]
/// (B7, B10).
///
/// Runs in one transaction (a savepoint when `db` is already one).
pub async fn delete_custom<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    refuse_builtin(&mut *tx, scope, id).await?;
    let deleted = sqlx::query!(
        "DELETE FROM roles WHERE org_id = $1 AND id = $2 RETURNING key, name, permissions",
        scope.org_id(),
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = json!({ "before": {
        "key": deleted.key, "name": deleted.name, "permissions": deleted.permissions,
    }});
    audit::record(&mut tx, scope, actor, Action::RoleDelete, id, &data, now).await?;
    tx.commit().await?;
    Ok(())
}

/// The custom role `id` of the scope's organization, with its row locked until the end of the
/// caller's transaction, so a check on its current permissions holds until the caller's
/// [`update_custom`] in the same transaction writes. A built-in role is refused with
/// [`Error::BuiltinImmutable`] without locking its instance-wide row.
pub async fn lock_custom(
    conn: &mut PgConnection,
    scope: OrgScope,
    id: Uuid,
) -> Result<Role, Error> {
    refuse_builtin(&mut *conn, scope, id).await?;
    sqlx::query_as!(
        Row,
        r#"SELECT id, key, name, builtin, permissions,
                  (extract(epoch FROM created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM roles
           WHERE org_id = $1 AND id = $2
           FOR UPDATE"#,
        scope.org_id(),
        id,
    )
    .fetch_optional(conn)
    .await?
    .ok_or(Error::NotFound)
    .and_then(Role::try_from)
}

/// [`Error::BuiltinImmutable`] when `id` is a built-in role, [`Error::NotFound`] when it is
/// neither built-in nor a custom role of the scope's organization.
async fn refuse_builtin(db: impl PgExecutor<'_>, scope: OrgScope, id: Uuid) -> Result<(), Error> {
    let builtin = sqlx::query_scalar!(
        "SELECT builtin FROM roles WHERE (org_id IS NULL OR org_id = $1) AND id = $2",
        scope.org_id(),
        id,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    if builtin {
        return Err(Error::BuiltinImmutable);
    }
    Ok(())
}

/// Spec 09 Q7: `^[a-z][a-z0-9_]{0,62}$`; a key of [`BUILTIN`] is taken.
fn valid_key(key: &str) -> Result<(), Error> {
    let bytes = key.as_bytes();
    let well_formed = (1..=MAX_KEY_CHARS).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_');
    if !well_formed {
        return Err(Error::KeyInvalid);
    }
    if builtin(key).is_some() {
        return Err(Error::KeyTaken);
    }
    Ok(())
}

fn valid_name(name: &str) -> Result<&str, Error> {
    name::valid(name).map_err(|invalid| match invalid {
        name::Invalid::Empty => Error::NameEmpty,
        name::Invalid::TooLong => Error::NameTooLong,
        name::Invalid::ContainsNul => Error::ContainsNul { field: "name" },
    })
}

/// Every entry is a catalog key and none repeats; the first entry that breaks either rule is
/// named. It stops there, so a long list costs at most [`perm::ALL`]'s length plus one checks.
fn valid_permissions(permissions: &[String]) -> Result<(), Error> {
    let mut seen = Vec::with_capacity(perm::ALL.len());
    for (index, key) in permissions.iter().enumerate() {
        if !perm::ALL.iter().any(|p| p.key() == key) {
            return Err(Error::UnknownPermission { index });
        }
        if seen.contains(&key) {
            return Err(Error::DuplicatePermission { index });
        }
        seen.push(key);
    }
    Ok(())
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
    use std::collections::HashSet;

    fn role(key: &str) -> &'static BuiltinRole {
        BUILTIN
            .iter()
            .copied()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("no built-in role {key}"))
    }

    fn set(keys: Vec<&'static str>) -> HashSet<&'static str> {
        keys.into_iter().collect()
    }

    #[test]
    fn the_eight_roles_with_unique_keys() {
        let keys: Vec<_> = BUILTIN.iter().map(|r| r.key).collect();
        assert_eq!(
            keys,
            [
                "org_admin",
                "account_admin",
                "course_designer",
                "instructor",
                "grader",
                "student",
                "manager",
                "observer",
            ]
        );
    }

    /// The custom role key format (spec 09 Q7), which built-in keys share.
    #[test]
    fn keys_match_the_role_key_format() {
        for r in BUILTIN {
            let mut chars = r.key.chars();
            let first_ok = chars.next().is_some_and(|c| c.is_ascii_lowercase());
            let rest_ok = chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            assert!(first_ok && rest_ok && r.key.len() <= 63, "{}", r.key);
            assert!(!r.name.trim().is_empty(), "{} has no name", r.key);
        }
    }

    #[test]
    fn org_admin_holds_every_catalog_key() {
        let all: Vec<_> = perm::ALL.iter().map(|p| p.key()).collect();
        assert_eq!(role("org_admin").permissions(), all);
    }

    /// Organization admin is defined as the whole catalog, not a list that matches it today.
    #[test]
    fn org_admin_is_defined_as_all() {
        assert!(matches!(ORG_ADMIN.permissions, RolePermissions::All));
    }

    /// Instructor is defined by extending Course designer, so the designer's later keys reach it.
    #[test]
    fn instructor_is_defined_as_extending_course_designer() {
        assert!(matches!(
            INSTRUCTOR.permissions,
            RolePermissions::Extends { base, .. } if std::ptr::eq(base, &COURSE_DESIGNER)
        ));
    }

    #[test]
    fn instructor_holds_every_course_designer_key() {
        let instructor = set(role("instructor").permissions());
        let designer = set(role("course_designer").permissions());
        assert!(instructor.is_superset(&designer));
    }

    #[test]
    fn extends_puts_the_base_first_then_its_own() {
        static BASE: BuiltinRole = BuiltinRole {
            key: "base",
            name: "Base",
            permissions: RolePermissions::Keys(&[perm::AccountView::KEY]),
        };
        let child = BuiltinRole {
            key: "child",
            name: "Child",
            permissions: RolePermissions::Extends {
                base: &BASE,
                own: &[perm::RoleView::KEY],
            },
        };
        assert_eq!(child.permissions(), ["account.view", "role.view"]);
    }

    #[test]
    fn account_admin_holds_exactly_its_list() {
        assert_eq!(
            set(role("account_admin").permissions()),
            set(vec![
                "account.view",
                "account.manage",
                "user.view",
                "role.view",
                "role.assign",
                "course.view",
                "course.view_all",
                "course.create",
                "course.manage",
                "course.content_edit",
                "enrollment.view",
                "enrollment.manage",
                "progress.view",
                "assignment.manage",
            ])
        );
    }

    /// The M1 lists of M1 spec 02 -> Permissions, with spec 05's progress keys (Q2, B18),
    /// Instructor's `user.view` (spec 02 B17) and spec 13's `assignment.manage` (Q8).
    #[test]
    fn the_course_roles_hold_their_m1_lists() {
        let cases: [(&str, &[&str]); 6] = [
            (
                "course_designer",
                &["course.view", "course.view_all", "course.content_edit"],
            ),
            (
                "instructor",
                &[
                    "course.view",
                    "course.view_all",
                    "course.content_edit",
                    "user.view",
                    "enrollment.view",
                    "enrollment.manage",
                    "progress.view",
                    "assignment.manage",
                ],
            ),
            ("grader", &[]),
            ("student", &["course.view", "course.participate"]),
            ("manager", &[]),
            ("observer", &["course.view", "progress.view_observed"]),
        ];
        for (key, expected) in cases {
            assert_eq!(role(key).permissions(), expected, "{key}");
        }
    }

    #[test]
    fn every_role_key_is_in_the_catalog_once() {
        let catalog: HashSet<_> = perm::ALL.iter().map(|p| p.key()).collect();
        for r in BUILTIN {
            let keys = r.permissions();
            assert_eq!(
                set(keys.clone()).len(),
                keys.len(),
                "{} repeats a key",
                r.key
            );
            for k in keys {
                assert!(catalog.contains(k), "{} lists {k}, not in perm::ALL", r.key);
            }
        }
    }

    #[test]
    fn builtin_finds_a_role_by_key() {
        assert_eq!(
            builtin("account_admin").map(|r| r.name),
            Some("Account admin")
        );
        assert!(builtin("tower_admin").is_none());
    }

    #[test]
    fn keys_of_the_q7_format_are_accepted() {
        let longest = format!("a{}", "b".repeat(MAX_KEY_CHARS - 1));
        for key in ["a", "tower_mentor", "t1", "a_", longest.as_str()] {
            assert!(valid_key(key).is_ok(), "{key:?}");
        }
    }

    #[test]
    fn keys_outside_the_q7_format_are_refused() {
        let too_long = "a".repeat(MAX_KEY_CHARS + 1);
        for key in [
            "",
            "_a",
            "1a",
            "Tower",
            "tower-mentor",
            "tower.mentor",
            "tower mentor",
            "tówer",
            "a\0",
            too_long.as_str(),
        ] {
            assert!(matches!(valid_key(key), Err(Error::KeyInvalid)), "{key:?}");
        }
    }

    #[test]
    fn every_builtin_key_is_taken() {
        for role in BUILTIN {
            assert!(
                matches!(valid_key(role.key), Err(Error::KeyTaken)),
                "{}",
                role.key
            );
        }
    }

    #[test]
    fn names_are_trimmed_and_blank_or_nul_is_refused() {
        assert_eq!(valid_name("  Tower mentor ").unwrap(), "Tower mentor");
        assert!(matches!(valid_name(" \t"), Err(Error::NameEmpty)));
        assert!(matches!(
            valid_name("T\0"),
            Err(Error::ContainsNul { field: "name" })
        ));
        let at_limit = "é".repeat(MAX_NAME_CHARS);
        assert_eq!(valid_name(&format!(" {at_limit} ")).unwrap(), at_limit);
        let over = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(matches!(valid_name(&over), Err(Error::NameTooLong)));
    }

    #[test]
    fn the_first_permission_outside_the_catalog_is_named_by_index() {
        let keys = |ks: &[&str]| ks.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
        assert!(valid_permissions(&keys(&[])).is_ok());
        assert!(valid_permissions(&keys(&["account.view", "role.assign"])).is_ok());
        assert!(matches!(
            valid_permissions(&keys(&["account.view", "nonexistent.perm", "x"])),
            Err(Error::UnknownPermission { index: 1 })
        ));
        assert!(matches!(
            valid_permissions(&keys(&["Account.View"])),
            Err(Error::UnknownPermission { index: 0 })
        ));
    }

    #[test]
    fn the_first_repeated_permission_is_named_by_index() {
        let keys = |ks: &[&str]| ks.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>();
        assert!(matches!(
            valid_permissions(&keys(&["user.view", "role.view", "user.view"])),
            Err(Error::DuplicatePermission { index: 2 })
        ));
        // The first broken rule wins, in list order.
        assert!(matches!(
            valid_permissions(&keys(&["user.view", "user.view", "nope"])),
            Err(Error::DuplicatePermission { index: 1 })
        ));
        assert!(matches!(
            valid_permissions(&keys(&["user.view", "nope", "user.view"])),
            Err(Error::UnknownPermission { index: 1 })
        ));
        let every: Vec<String> = perm::ALL.iter().map(|p| p.key().to_owned()).collect();
        assert!(valid_permissions(&every).is_ok());
    }

    #[test]
    fn builtin_permissions_come_from_code_and_custom_ones_as_stored() {
        let stored = vec!["branding.manage".to_owned()];
        assert_eq!(
            effective_permissions("account_admin", true, stored.clone()),
            [
                "account.view",
                "account.manage",
                "user.view",
                "role.view",
                "role.assign",
                "course.view",
                "course.view_all",
                "course.create",
                "course.manage",
                "course.content_edit",
                "enrollment.view",
                "enrollment.manage",
                "progress.view",
                "assignment.manage",
            ]
        );
        assert_eq!(
            effective_permissions("tower_mentor", false, stored.clone()),
            stored
        );
        assert!(effective_permissions("from_a_newer_release", true, stored).is_empty());
    }
}
