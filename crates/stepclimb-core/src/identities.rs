// SPDX-License-Identifier: AGPL-3.0-or-later
//! The identities repository (docs/specs/m0/03-users-and-identities.md B14 to B16, B20).
//!
//! An identity links one user to one identity provider by the provider's `subject`. Every query
//! here filters by `org_id = scope.org_id()`, and the table's composite foreign keys keep an
//! identity's user and provider in the identity's organization. `(provider_id, subject)` is
//! unique, so one external account maps to at most one user; [`link`] reports a taken pair as
//! [`Error::Taken`].
//!
//! Lookup is by `(provider, subject)` only ([`find_user`]): nothing here matches a user by email
//! (B17). Who may link or unlink is the routes' to decide. [`unlink_unless_last`] refuses to
//! remove a user's last sign-in method (B19); [`unlink`] does not, for writers that own the
//! identity's source. Only an identity at an enabled provider counts as a sign-in method.
//!
//! [`link`] and [`unlink`] write their audit entries themselves (`identity.link`,
//! `identity.unlink`, docs/specs/m0/09-permissions-and-roles.md B22), in the transaction that
//! makes the change, with the user, the provider's key and the subject; never the attributes.
//!
//! [`lock_user`] locks the user's row `FOR NO KEY UPDATE` until the transaction ends, the lock
//! `users::set_status` takes on it, so two [`unlink_unless_last`] calls for one user take turns
//! and the second counts what the first left. A link to the user is not held off; it only adds
//! to the count. Like the other locks of this crate, it protects only at `READ COMMITTED`, where
//! each statement after the wait sees what the previous holder committed; [`lock_user`] refuses
//! any other level with [`Error::IsolationLevel`].
//!
//! `attributes` are the mapped attributes of the latest sign-in (B15), a JSON object stored as
//! given. PostgreSQL `jsonb` cannot store NUL in a string or key, so a NUL anywhere in it is
//! refused with [`Error::ContainsNul`], as is a NUL in the subject.

use std::time::SystemTime;

use serde_json::{Map, Value, json};
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};
use uuid::Uuid;

use crate::{
    OrgScope,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
    id::new_id,
    local,
    users::{self, User},
};

/// The composite foreign key that keeps the user in the identity's organization.
const USER_FKEY: &str = "identities_org_id_user_id_fkey";
/// The composite foreign key that keeps the provider in the identity's organization.
const PROVIDER_FKEY: &str = "identities_org_id_provider_id_fkey";
/// The unique constraint on `(provider_id, subject)` (B14).
const SUBJECT_UNIQUE: &str = "identities_provider_id_subject_key";

/// The longest subject, in characters. At four bytes a character it stays under the size of a
/// btree index entry, so the `(provider_id, subject)` unique index can always hold it; OIDC
/// allows a `sub` of at most 255 ASCII characters (OpenID Connect Core 1.0, section 2).
pub const MAX_SUBJECT_CHARS: usize = 255;

/// An identity as the repository reads it.
#[derive(Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider_id: Uuid,
    /// The provider's `key`, read with the identity.
    pub provider_key: String,
    /// The provider's `label` from its config, read with the identity; `None` when the config
    /// has no string `label`.
    pub provider_label: Option<String>,
    pub subject: String,
    /// The mapped attributes of the latest sign-in (B15).
    pub attributes: Map<String, Value>,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
}

/// Leaves `attributes` out: they can hold personal data the provider sent, which logs never
/// carry (docs/specs/m0/07-admission.md B2).
impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Identity")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("provider_id", &self.provider_id)
            .field("provider_key", &self.provider_key)
            .field("provider_label", &self.provider_label)
            .field("subject", &self.subject)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish_non_exhaustive()
    }
}

/// An identity to create.
#[derive(Clone, PartialEq, Eq)]
pub struct NewIdentity {
    /// A user of the scope's organization.
    pub user_id: Uuid,
    /// A provider of the scope's organization.
    pub provider_id: Uuid,
    /// Stored exactly as given; empty is refused.
    pub subject: String,
    /// Empty for an identity an admin pre-provisions (B18).
    pub attributes: Map<String, Value>,
}

/// What [`find_user`] found: the identity and its user, with the user's status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Linked {
    pub identity_id: Uuid,
    pub user: User,
}

/// Why an identities repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// No identity with that id (of that user, where one is named) in the scope's organization.
    #[error("the identity does not exist")]
    NotFound,
    /// No user with that id in the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// No provider with that id in the scope's organization.
    #[error("the identity provider does not exist")]
    ProviderNotFound,
    /// The provider's subject is already linked to a user (B14).
    #[error("this identity is already linked to a user")]
    Taken,
    /// The identity is at the local provider, which these removals do not touch: a local account
    /// is changed through docs/specs/m0/04-local-accounts.md's routes.
    #[error("the identity is a local account's")]
    LocalIdentity,
    /// Removing the identity would leave its user no way to sign in (B19).
    #[error("the identity is the user's last sign-in method")]
    LastSignInMethod,
    /// The transaction runs at an isolation level other than `READ COMMITTED`, where
    /// [`lock_user`] does not protect what is read after it.
    #[error(
        "a user's identities can be changed under lock only at READ COMMITTED isolation, not {found}"
    )]
    IsolationLevel { found: String },
    /// The subject is empty.
    #[error("a subject cannot be empty")]
    SubjectEmpty,
    /// The subject is longer than [`MAX_SUBJECT_CHARS`].
    #[error("a subject is longer than {MAX_SUBJECT_CHARS} characters")]
    SubjectTooLong,
    /// A string the caller passed holds NUL, which PostgreSQL `text` and `jsonb` cannot store.
    #[error("{field} cannot contain the NUL character")]
    ContainsNul { field: &'static str },
    /// Stored `attributes` that are not a JSON object. The table's check constraint keeps this
    /// from happening, and the read fails closed if it does.
    #[error("the stored identity attributes are not a JSON object")]
    StoredAttributes,
    /// Reading the user's local credentials failed.
    #[error(transparent)]
    Local(#[from] local::Error),
    /// The linked user's row does not read back.
    #[error(transparent)]
    User(#[from] users::Error),
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
            Some(USER_FKEY) => Self::UserNotFound,
            Some(PROVIDER_FKEY) => Self::ProviderNotFound,
            Some(SUBJECT_UNIQUE) => Self::Taken,
            _ => Self::Database(error),
        }
    }
}

/// An `identities` row joined to its provider's key, `attributes` as JSON text.
struct Row {
    id: Uuid,
    user_id: Uuid,
    provider_id: Uuid,
    provider_key: String,
    provider_label: Option<String>,
    subject: String,
    attributes: String,
    created_at_us: i64,
    updated_at_us: i64,
}

impl TryFrom<Row> for Identity {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let attributes =
            serde_json::from_str(&row.attributes).map_err(|_| Error::StoredAttributes)?;
        Ok(Self {
            id: row.id,
            user_id: row.user_id,
            provider_id: row.provider_id,
            provider_key: row.provider_key,
            provider_label: row.provider_label,
            subject: row.subject,
            attributes,
            created_at: from_epoch_micros(row.created_at_us)?,
            updated_at: from_epoch_micros(row.updated_at_us)?,
        })
    }
}

/// The user linked to `(provider_id, subject)` in the scope's organization, with the identity's
/// id, or `None` when no identity of the organization matches (B16). Matches the subject exactly
/// and never by email (B17).
pub async fn find_user(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    provider_id: Uuid,
    subject: &str,
) -> Result<Option<Linked>, Error> {
    let subject = no_nul("subject", subject)?;
    let Some(row) = sqlx::query!(
        r#"SELECT i.id AS identity_id,
                  u.id, u.display_name, u.given_name, u.family_name, u.email, u.locale, u.tz,
                  u.status, u.source, u.external_id, u.manager_id,
                  (extract(epoch FROM u.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM u.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identities i
           JOIN users u ON u.org_id = i.org_id AND u.id = i.user_id
           WHERE i.org_id = $1 AND i.provider_id = $2 AND i.subject = $3"#,
        scope.org_id(),
        provider_id,
        subject,
    )
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };
    let user = User::try_from(users::Row {
        id: row.id,
        display_name: row.display_name,
        given_name: row.given_name,
        family_name: row.family_name,
        email: row.email,
        locale: row.locale,
        tz: row.tz,
        status: row.status,
        source: row.source,
        external_id: row.external_id,
        manager_id: row.manager_id,
        created_at_us: row.created_at_us,
        updated_at_us: row.updated_at_us,
    })?;
    Ok(Some(Linked {
        identity_id: row.identity_id,
        user,
    }))
}

/// Link a user to a provider's subject in the scope's organization, with `created_at` and
/// `updated_at` set to `now` (B14, B18, B20), and write its `identity.link` audit entry by
/// `actor`. Runs in one transaction (a savepoint when `db` is already one).
///
/// Refuses a user or provider outside the organization ([`Error::UserNotFound`],
/// [`Error::ProviderNotFound`]) and a `(provider, subject)` already linked to any user
/// ([`Error::Taken`]).
///
/// The insert takes the provider from a `SELECT` filtered by the scope, so another
/// organization's provider inserts no row and is [`Error::ProviderNotFound`] before the
/// `(provider_id, subject)` unique index is consulted. Left to the composite foreign key alone,
/// PostgreSQL checks that index first, and [`Error::Taken`] would tell org A whether a subject
/// is linked at org B's provider.
pub async fn link<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    identity: &NewIdentity,
    now: SystemTime,
    actor: Actor,
) -> Result<Identity, Error> {
    let subject = valid_subject(&identity.subject)?;
    let attributes = valid_attributes(&identity.attributes)?;
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let row = sqlx::query_as!(
        Row,
        r#"WITH inserted AS (
               INSERT INTO identities (id, org_id, user_id, provider_id, subject, attributes,
                                       created_at, updated_at)
               SELECT $1, p.org_id, $3, p.id, $5, $6::text::jsonb,
                      timestamptz 'epoch' + $7::bigint * interval '1 microsecond',
                      timestamptz 'epoch' + $7::bigint * interval '1 microsecond'
               FROM identity_providers p
               WHERE p.org_id = $2 AND p.id = $4
               RETURNING id, org_id, user_id, provider_id, subject, attributes, created_at,
                         updated_at
           )
           SELECT i.id AS "id!", i.user_id AS "user_id!", i.provider_id AS "provider_id!",
                  p.key AS "provider_key!",
                  CASE WHEN jsonb_typeof(p.config -> 'label') = 'string'
                       THEN p.config ->> 'label' END AS provider_label,
                  i.subject AS "subject!", i.attributes::text AS "attributes!",
                  (extract(epoch FROM i.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM i.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM inserted i
           JOIN identity_providers p ON p.org_id = i.org_id AND p.id = i.provider_id"#,
        new_id(),
        scope.org_id(),
        identity.user_id,
        identity.provider_id,
        subject,
        attributes,
        now,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::ProviderNotFound)?;
    let linked = Identity::try_from(row)?;
    let data =
        json!({ "after": audit_data(linked.user_id, &linked.provider_key, &linked.subject) });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::IdentityLink,
        linked.id,
        &data,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(linked)
}

/// What an audit entry records of an identity: whose it is, the provider's key and the subject.
/// Never the attributes, which can hold personal data the provider sent.
fn audit_data(user_id: Uuid, provider_key: &str, subject: &str) -> Value {
    json!({ "user_id": user_id, "provider_key": provider_key, "subject": subject })
}

/// Delete the identity `id` of the user `user_id` in the scope's organization, and write its
/// `identity.unlink` audit entry by `actor` at `now`, with the identity as it was. An identity
/// of another user, or of another organization, is [`Error::NotFound`]. Runs in one transaction
/// (a savepoint when `db` is already one).
pub async fn unlink<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let removed = sqlx::query!(
        "DELETE FROM identities i
         USING identity_providers p
         WHERE i.org_id = $1 AND i.user_id = $2 AND i.id = $3
           AND p.org_id = i.org_id AND p.id = i.provider_id
         RETURNING i.subject, p.key",
        scope.org_id(),
        user_id,
        id,
    )
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(Error::NotFound)?;
    let data = json!({ "before": audit_data(user_id, &removed.key, &removed.subject) });
    audit::record(
        &mut tx,
        scope,
        actor,
        Action::IdentityUnlink,
        id,
        &data,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(())
}

/// Delete the identity `id` of the user `user_id` in the scope's organization, as [`unlink`] does
/// (audit entry included), unless it is at the local provider ([`Error::LocalIdentity`]) or is the
/// user's last sign-in method ([`Error::LastSignInMethod`], B19). An identity of another user, or
/// of another organization, is [`Error::NotFound`]; a user the organization does not have is
/// [`Error::UserNotFound`].
///
/// A user signs in with an identity at an enabled provider or a local credential, so the user
/// must keep another identity at an enabled provider: one at a disabled provider does not
/// count, and a user with none at an enabled provider can have no identity removed here. A
/// local identity does not count either: what signs a local account in is its passkey or
/// password, not the identity, and [`local::has_credential`] counts those.
///
/// Takes [`lock_user`] first, so two removals for one user take turns and the second counts
/// what the first left. Runs in one transaction (a savepoint when `db` is already one); the lock
/// is held until the outermost transaction ends.
pub async fn unlink_unless_last<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    user_id: Uuid,
    id: Uuid,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let mut tx = db.begin().await?;
    lock_user(&mut tx, scope, user_id).await?;
    let counts = sqlx::query!(
        r#"SELECT count(*) FILTER (WHERE i.id = $3) AS "this!",
                  count(*) FILTER (WHERE i.id = $3 AND p.kind = 'local') AS "this_local!",
                  count(*) FILTER (WHERE i.id <> $3 AND p.enabled AND p.kind <> 'local')
                      AS "others!"
           FROM identities i
           JOIN identity_providers p ON p.org_id = i.org_id AND p.id = i.provider_id
           WHERE i.org_id = $1 AND i.user_id = $2"#,
        scope.org_id(),
        user_id,
        id,
    )
    .fetch_one(&mut *tx)
    .await?;
    if counts.this == 0 {
        return Err(Error::NotFound);
    }
    if counts.this_local > 0 {
        return Err(Error::LocalIdentity);
    }
    if counts.others == 0 && !local::has_credential(&mut tx, scope, user_id).await? {
        return Err(Error::LastSignInMethod);
    }
    unlink(&mut *tx, scope, user_id, id, now, actor).await?;
    tx.commit().await?;
    Ok(())
}

/// Whether the user `user_id` of the scope's organization has an identity at an enabled
/// provider that is not local: a way to sign in that is not a local credential (B19's count).
pub async fn has_enabled_identity(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<bool, Error> {
    let found = sqlx::query_scalar!(
        r#"SELECT EXISTS (
               SELECT 1 FROM identities i
               JOIN identity_providers p ON p.org_id = i.org_id AND p.id = i.provider_id
               WHERE i.org_id = $1 AND i.user_id = $2 AND p.enabled AND p.kind <> 'local'
           ) AS "found!""#,
        scope.org_id(),
        user_id,
    )
    .fetch_one(db)
    .await?;
    Ok(found)
}

/// Lock the row of the user `user_id` in the scope's organization `FOR NO KEY UPDATE` until the
/// transaction ends (see the module documentation), and return the user. A user the
/// organization does not have is [`Error::UserNotFound`]; a transaction whose isolation level is
/// not `READ COMMITTED` is [`Error::IsolationLevel`].
pub async fn lock_user(
    conn: &mut PgConnection,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<User, Error> {
    let isolation = sqlx::query_scalar!(
        r#"SELECT current_setting('transaction_isolation') AS "isolation!"
           FROM users WHERE org_id = $1 AND id = $2
           FOR NO KEY UPDATE"#,
        scope.org_id(),
        user_id,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::UserNotFound)?;
    if isolation != "read committed" {
        return Err(Error::IsolationLevel { found: isolation });
    }
    users::get(&mut *conn, scope, user_id)
        .await?
        .ok_or(Error::UserNotFound)
}

/// The identities of the user `user_id` in the scope's organization, sorted by id (creation
/// order). Empty for a user the organization does not have.
pub async fn list_for_user(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    user_id: Uuid,
) -> Result<Vec<Identity>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT i.id, i.user_id, i.provider_id, p.key AS "provider_key!",
                  CASE WHEN jsonb_typeof(p.config -> 'label') = 'string'
                       THEN p.config ->> 'label' END AS provider_label,
                  i.subject, i.attributes::text AS "attributes!",
                  (extract(epoch FROM i.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM i.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM identities i
           JOIN identity_providers p ON p.org_id = i.org_id AND p.id = i.provider_id
           WHERE i.org_id = $1 AND i.user_id = $2
           ORDER BY i.id"#,
        scope.org_id(),
        user_id,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Identity::try_from).collect()
}

/// Replace the attributes of the identity `id` in the scope's organization with `attributes`
/// and set `updated_at` to `now` (B15; docs/specs/m0/07-admission.md B7).
pub async fn update_attributes(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    id: Uuid,
    attributes: &Map<String, Value>,
    now: SystemTime,
) -> Result<Identity, Error> {
    let attributes = valid_attributes(attributes)?;
    let now = epoch_micros(now)?;
    let row = sqlx::query_as!(
        Row,
        r#"WITH updated AS (
               UPDATE identities
               SET attributes = $3::text::jsonb,
                   updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
               WHERE org_id = $1 AND id = $2
               RETURNING id, org_id, user_id, provider_id, subject, attributes, created_at,
                         updated_at
           )
           SELECT i.id AS "id!", i.user_id AS "user_id!", i.provider_id AS "provider_id!",
                  p.key AS "provider_key!",
                  CASE WHEN jsonb_typeof(p.config -> 'label') = 'string'
                       THEN p.config ->> 'label' END AS provider_label,
                  i.subject AS "subject!", i.attributes::text AS "attributes!",
                  (extract(epoch FROM i.created_at) * 1000000)::bigint AS "created_at_us!",
                  (extract(epoch FROM i.updated_at) * 1000000)::bigint AS "updated_at_us!"
           FROM updated i
           JOIN identity_providers p ON p.org_id = i.org_id AND p.id = i.provider_id"#,
        scope.org_id(),
        id,
        attributes,
        now,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    Identity::try_from(row)
}

/// `value` unchanged, or [`Error::ContainsNul`] naming `field`.
fn no_nul<'a>(field: &'static str, value: &'a str) -> Result<&'a str, Error> {
    if value.contains('\0') {
        return Err(Error::ContainsNul { field });
    }
    Ok(value)
}

/// The subject check [`link`] makes, without a database, for a caller that must refuse an
/// invalid subject before it takes a lock: [`Error::ContainsNul`], [`Error::SubjectEmpty`] or
/// [`Error::SubjectTooLong`].
pub fn validate_subject(subject: &str) -> Result<(), Error> {
    valid_subject(subject).map(|_| ())
}

fn valid_subject(subject: &str) -> Result<&str, Error> {
    let subject = no_nul("subject", subject)?;
    if subject.is_empty() {
        return Err(Error::SubjectEmpty);
    }
    if subject.chars().count() > MAX_SUBJECT_CHARS {
        return Err(Error::SubjectTooLong);
    }
    Ok(subject)
}

/// `attributes` as JSON text, or [`Error::ContainsNul`] when a key or string anywhere in it
/// holds NUL.
fn valid_attributes(attributes: &Map<String, Value>) -> Result<String, Error> {
    if attributes
        .iter()
        .any(|(key, value)| key.contains('\0') || value_has_nul(value))
    {
        return Err(Error::ContainsNul {
            field: "attributes",
        });
    }
    // Invariant: a `Map<String, Value>` has string keys and plain values, which serde_json
    // always serializes.
    Ok(serde_json::to_string(attributes).expect("a JSON object serializes"))
}

fn value_has_nul(value: &Value) -> bool {
    match value {
        Value::String(s) => s.contains('\0'),
        Value::Array(items) => items.iter().any(value_has_nul),
        Value::Object(map) => map
            .iter()
            .any(|(key, value)| key.contains('\0') || value_has_nul(value)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        match value {
            Value::Object(map) => map,
            other => panic!("not an object: {other}"),
        }
    }

    #[test]
    fn a_subject_is_kept_exactly_and_empty_is_refused() {
        assert_eq!(valid_subject(" 1000001 ").unwrap(), " 1000001 ");
        assert!(matches!(valid_subject(""), Err(Error::SubjectEmpty)));
        assert!(matches!(
            valid_subject("10\0"),
            Err(Error::ContainsNul { field: "subject" })
        ));
    }

    #[test]
    fn nul_anywhere_in_the_attributes_is_refused() {
        let cases = [
            json!({ "rating\u{0}": 5 }),
            json!({ "rating": "S\u{0}1" }),
            json!({ "teams": ["a", "b\u{0}"] }),
            json!({ "nested": { "deeper": [{ "k\u{0}": 1 }] } }),
        ];
        for case in cases {
            assert!(
                matches!(
                    valid_attributes(&object(case.clone())),
                    Err(Error::ContainsNul {
                        field: "attributes"
                    })
                ),
                "{case}"
            );
        }
    }

    #[test]
    fn attributes_without_nul_serialize_as_given() {
        let attributes = object(json!({
            "rating": 5, "subdivision": "EXA", "teams": ["a", null, true, 1.5],
            "nested": { "k": "v" }
        }));
        let text = valid_attributes(&attributes).unwrap();
        let back: Map<String, Value> = serde_json::from_str(&text).unwrap();
        assert_eq!(back, attributes);
    }

    #[test]
    fn debug_leaves_the_attributes_out() {
        let identity = Identity {
            id: Uuid::nil(),
            user_id: Uuid::nil(),
            provider_id: Uuid::nil(),
            provider_key: "example".into(),
            provider_label: Some("Example".into()),
            subject: "1000001".into(),
            attributes: object(json!({ "email_hint": "private-value" })),
            created_at: SystemTime::UNIX_EPOCH,
            updated_at: SystemTime::UNIX_EPOCH,
        };
        let debug = format!("{identity:?}");
        assert!(debug.contains("1000001"), "{debug}");
        assert!(!debug.contains("private-value"), "{debug}");
    }
}
