// SPDX-License-Identifier: AGPL-3.0-or-later
//! Audit entries for permission and account changes (docs/specs/m0/09-permissions-and-roles.md
//! Q5 and "Audit entries", docs/specs/m0/08-sessions-and-csrf.md Q14).
//!
//! The repository function that makes an audited change writes its entry itself, with
//! [`record`], on the transaction it changes the rows in: the entry commits with the change, and
//! a refused or failed change rolls its entry back with it. Each such function takes the
//! [`Actor`], so no caller can make the change without saying who made it. Entries are never
//! changed afterwards.
//!
//! An entry's `data` holds what the change touched, before and after, never a secret. [`list`]
//! reads the scope's entries; there is no route for it in M0.

use std::{net::IpAddr, time::SystemTime};

use serde_json::Value;
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, from_epoch_micros},
    id::new_id,
};

/// Who made a change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    /// A user, through a request from the client address the server derived for it
    /// (`STEPCLIMB_TRUSTED_PROXIES` decides which `X-Forwarded-For` entry that is).
    User { id: Uuid, ip: IpAddr },
    /// A service account's API key, by the service account's id, through a request from the
    /// client address the server derived for it (docs/specs/m0/08-sessions-and-csrf.md I6).
    ApiKey { id: Uuid, ip: IpAddr },
    /// The instance on its own behalf: the `stepclimb admin` CLI, startup, a job. It has no id
    /// and no address.
    Instance,
}

/// What an entry records. Each action has one target type ([`Action::target_type`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    RoleCreate,
    RoleUpdate,
    RoleDelete,
    RoleAssignmentGrant,
    RoleAssignmentRemove,
    AccountCreate,
    AccountRename,
    AccountMove,
    AccountDelete,
    OrganizationCreate,
    OrganizationRename,
    OrganizationUpdateSettings,
    OrganizationUpdateBranding,
    UserSetStatus,
    IdentityLink,
    IdentityUnlink,
    IdentityProviderCreate,
    IdentityProviderUpdate,
    IdentityProviderDelete,
    EnrollmentLinkCreate,
    PasskeyCreate,
    PasskeyRename,
    PasskeyDelete,
    PasswordSet,
    TotpEnable,
    TotpDisable,
    ServiceAccountCreate,
    ServiceAccountDelete,
    AdmissionWebhookCreate,
    AdmissionWebhookUpdate,
    AdmissionWebhookRotateSecret,
    AdmissionWebhookDelete,
    /// A spent refresh token or authorization code was presented again, and its refresh family
    /// was revoked (docs/specs/m0/08-sessions-and-csrf.md B32, B34, Q14). The entry names the
    /// family by its first token's id.
    RefreshTokenReuse,
    CourseCreate,
    CourseUpdate,
    CourseSetStatus,
    CourseMove,
}

impl Action {
    /// The stored action, `target_type.verb`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoleCreate => "role.create",
            Self::RoleUpdate => "role.update",
            Self::RoleDelete => "role.delete",
            Self::RoleAssignmentGrant => "role_assignment.grant",
            Self::RoleAssignmentRemove => "role_assignment.remove",
            Self::AccountCreate => "account.create",
            Self::AccountRename => "account.rename",
            Self::AccountMove => "account.move",
            Self::AccountDelete => "account.delete",
            Self::OrganizationCreate => "organization.create",
            Self::OrganizationRename => "organization.rename",
            Self::OrganizationUpdateSettings => "organization.update_settings",
            Self::OrganizationUpdateBranding => "organization.update_branding",
            Self::UserSetStatus => "user.set_status",
            Self::IdentityLink => "identity.link",
            Self::IdentityUnlink => "identity.unlink",
            Self::IdentityProviderCreate => "identity_provider.create",
            Self::IdentityProviderUpdate => "identity_provider.update",
            Self::IdentityProviderDelete => "identity_provider.delete",
            Self::EnrollmentLinkCreate => "enrollment_link.create",
            Self::PasskeyCreate => "passkey.create",
            Self::PasskeyRename => "passkey.rename",
            Self::PasskeyDelete => "passkey.delete",
            Self::PasswordSet => "password.set",
            Self::TotpEnable => "totp.enable",
            Self::TotpDisable => "totp.disable",
            Self::ServiceAccountCreate => "service_account.create",
            Self::ServiceAccountDelete => "service_account.delete",
            Self::AdmissionWebhookCreate => "admission_webhook.create",
            Self::AdmissionWebhookUpdate => "admission_webhook.update",
            Self::AdmissionWebhookRotateSecret => "admission_webhook.rotate_secret",
            Self::AdmissionWebhookDelete => "admission_webhook.delete",
            Self::RefreshTokenReuse => "refresh_token.reuse",
            Self::CourseCreate => "course.create",
            Self::CourseUpdate => "course.update",
            Self::CourseSetStatus => "course.set_status",
            Self::CourseMove => "course.move",
        }
    }

    /// The kind of row the entry's `target_id` names: the action's prefix.
    #[must_use]
    pub fn target_type(self) -> &'static str {
        match self {
            Self::RoleCreate | Self::RoleUpdate | Self::RoleDelete => "role",
            Self::RoleAssignmentGrant | Self::RoleAssignmentRemove => "role_assignment",
            Self::AccountCreate | Self::AccountRename | Self::AccountMove | Self::AccountDelete => {
                "account"
            }
            Self::OrganizationCreate
            | Self::OrganizationRename
            | Self::OrganizationUpdateSettings
            | Self::OrganizationUpdateBranding => "organization",
            Self::UserSetStatus => "user",
            Self::IdentityLink | Self::IdentityUnlink => "identity",
            Self::IdentityProviderCreate
            | Self::IdentityProviderUpdate
            | Self::IdentityProviderDelete => "identity_provider",
            Self::EnrollmentLinkCreate => "enrollment_link",
            Self::PasskeyCreate | Self::PasskeyRename | Self::PasskeyDelete => "passkey",
            // A password has no row id of its own; the entry names its user.
            Self::PasswordSet => "password",
            // Nor has a user's TOTP; the entry names its user.
            Self::TotpEnable | Self::TotpDisable => "totp",
            Self::ServiceAccountCreate | Self::ServiceAccountDelete => "service_account",
            Self::AdmissionWebhookCreate
            | Self::AdmissionWebhookUpdate
            | Self::AdmissionWebhookRotateSecret
            | Self::AdmissionWebhookDelete => "admission_webhook",
            Self::RefreshTokenReuse => "refresh_token",
            Self::CourseCreate | Self::CourseUpdate | Self::CourseSetStatus | Self::CourseMove => {
                "course"
            }
        }
    }
}

/// One stored entry. `action` and `target_type` stay strings, so an entry a newer release wrote
/// still reads.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: Uuid,
    pub actor: Actor,
    pub action: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub data: Value,
    pub at: SystemTime,
}

/// Why reading entries failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A stored column this release cannot read: an actor that is neither a user with an
    /// address nor the instance, an address or `data` that does not parse. The table's checks
    /// and column types keep this from happening.
    #[error("the {column} of audit entry {id} cannot be read")]
    Stored { id: Uuid, column: &'static str },
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Write the entry for `action` on `target_id` in the scope's organization, by `actor`, at
/// `at_micros` (the change's own clock reading, as `db::epoch_micros` gives it), with `data`.
///
/// Called only by the repository function making the change, on its transaction, after the
/// change succeeded and before it commits.
pub(crate) async fn record(
    conn: &mut PgConnection,
    scope: OrgScope,
    actor: Actor,
    action: Action,
    target_id: Uuid,
    data: &Value,
    at_micros: i64,
) -> Result<(), sqlx::Error> {
    let (actor_type, actor_id, ip) = match actor {
        Actor::User { id, ip } => ("user", Some(id), Some(ip.to_string())),
        Actor::ApiKey { id, ip } => ("api_key", Some(id), Some(ip.to_string())),
        Actor::Instance => ("instance", None, None),
    };
    sqlx::query!(
        "INSERT INTO audit_events (id, org_id, actor_type, actor_id, action, target_type,
                                   target_id, data, at, ip, created_at, updated_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::text::jsonb,
                 timestamptz 'epoch' + $9::bigint * interval '1 microsecond', $10::text::inet,
                 timestamptz 'epoch' + $9::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $9::bigint * interval '1 microsecond')",
        new_id(),
        scope.org_id(),
        actor_type,
        actor_id,
        action.as_str(),
        action.target_type(),
        target_id,
        data.to_string(),
        at_micros,
        ip,
    )
    .execute(conn)
    .await?;
    Ok(())
}

/// The scope's entries in the order they were written (by `at`, then id), at most `limit`.
pub async fn list(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    limit: i64,
) -> Result<Vec<Entry>, Error> {
    let rows = sqlx::query_as!(
        Row,
        r#"SELECT id, actor_type, actor_id, host(ip) AS ip, action, target_type, target_id,
                  data::text AS "data!",
                  (extract(epoch FROM at) * 1000000)::bigint AS "at_us!"
           FROM audit_events
           WHERE org_id = $1
           ORDER BY at, id
           LIMIT $2"#,
        scope.org_id(),
        limit,
    )
    .fetch_all(db)
    .await?;
    rows.into_iter().map(Entry::try_from).collect()
}

/// An `audit_events` row as [`list`] selects it.
struct Row {
    id: Uuid,
    actor_type: String,
    actor_id: Option<Uuid>,
    ip: Option<String>,
    action: String,
    target_type: String,
    target_id: Uuid,
    data: String,
    at_us: i64,
}

impl TryFrom<Row> for Entry {
    type Error = Error;

    fn try_from(row: Row) -> Result<Self, Error> {
        let stored = |column| Error::Stored { id: row.id, column };
        let actor = match (row.actor_type.as_str(), row.actor_id, row.ip.as_deref()) {
            ("user", Some(id), Some(ip)) => Actor::User {
                id,
                ip: ip.parse().map_err(|_| stored("ip"))?,
            },
            ("api_key", Some(id), Some(ip)) => Actor::ApiKey {
                id,
                ip: ip.parse().map_err(|_| stored("ip"))?,
            },
            ("instance", None, None) => Actor::Instance,
            _ => return Err(stored("actor")),
        };
        Ok(Self {
            id: row.id,
            actor,
            data: serde_json::from_str(&row.data).map_err(|_| stored("data"))?,
            at: from_epoch_micros(row.at_us)?,
            action: row.action,
            target_type: row.target_type,
            target_id: row.target_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Action; 33] = [
        Action::RoleCreate,
        Action::RoleUpdate,
        Action::RoleDelete,
        Action::RoleAssignmentGrant,
        Action::RoleAssignmentRemove,
        Action::AccountCreate,
        Action::AccountRename,
        Action::AccountMove,
        Action::AccountDelete,
        Action::OrganizationCreate,
        Action::OrganizationRename,
        Action::OrganizationUpdateSettings,
        Action::OrganizationUpdateBranding,
        Action::UserSetStatus,
        Action::IdentityLink,
        Action::IdentityUnlink,
        Action::IdentityProviderCreate,
        Action::IdentityProviderUpdate,
        Action::IdentityProviderDelete,
        Action::EnrollmentLinkCreate,
        Action::PasskeyCreate,
        Action::PasskeyRename,
        Action::PasskeyDelete,
        Action::PasswordSet,
        Action::TotpEnable,
        Action::TotpDisable,
        Action::ServiceAccountCreate,
        Action::ServiceAccountDelete,
        Action::RefreshTokenReuse,
        Action::CourseCreate,
        Action::CourseUpdate,
        Action::CourseSetStatus,
        Action::CourseMove,
    ];

    #[test]
    fn every_action_starts_with_its_target_type() {
        for action in ALL {
            let (prefix, verb) = action.as_str().split_once('.').unwrap();
            assert_eq!(prefix, action.target_type(), "{action:?}");
            assert!(!verb.is_empty() && !verb.contains('.'), "{action:?}");
        }
    }

    #[test]
    fn actions_are_distinct() {
        let mut names: Vec<_> = ALL.iter().map(|a| a.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ALL.len());
    }
}
