// SPDX-License-Identifier: AGPL-3.0-or-later
//! The local provider and its method switches (docs/specs/m0/04-local-accounts.md B1 to B3, Q2).
//!
//! Every organization has one `identity_providers` row with kind and key `local` ([`ensure_row`]
//! writes it), which local identities point at. Its `enabled` column switches local sign-in as a
//! whole. The four method switches (passkey, password, TOTP, magic link) are the `local_sign_in`
//! key of [`OrgSettings`](crate::OrgSettings), with passkey, password and TOTP on and the magic
//! link off by default (Q2). A method is offered only when the row is enabled and its switch is
//! on ([`LocalSignIn::offers`]); turning a method off keeps its credentials, which work again
//! once it is turned back on (B2).
//!
//! TOTP is a second step after a password, never a way in by itself: local sign-in is offered
//! only while the passkey, the password or the magic link is ([`LocalSignIn::offered`]).
//!
//! [`update`] is the only writer of the switches: the generic settings patch cannot name
//! `local_sign_in`. Under the organization row's lock it refuses the magic link (no email before
//! M1) and a change that would leave the organization no primary way to sign in (B2: an offered
//! passkey, password or magic link, or an enabled provider that is not local). It then writes the
//! switches with an `organization.update_settings` audit entry and the row's `enabled` with an
//! `identity_provider.update` entry. Every query filters by the scope's organization.

use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};

use crate::{
    OrgScope, OrgSettings,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
    id::new_id,
    identity_providers::{self, Kind},
};

/// The local provider's key, which its kind's spelling equals (the migration ties the two).
pub const PROVIDER_KEY: &str = "local";

/// A local sign-in method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Method {
    Passkey,
    Password,
    Totp,
    MagicLink,
}

impl Method {
    /// Every method, in the order the API lists them.
    pub const ALL: [Self; 4] = [Self::Passkey, Self::Password, Self::Totp, Self::MagicLink];

    /// The methods that sign in by themselves; TOTP is a second step after a password.
    pub const PRIMARY: [Self; 3] = [Self::Passkey, Self::Password, Self::MagicLink];

    /// The method's name in the settings and the API.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passkey => "passkey",
            Self::Password => "password",
            Self::Totp => "totp",
            Self::MagicLink => "magic_link",
        }
    }
}

/// The four method switches, as the `local_sign_in` settings key stores them. A stored object
/// missing a switch reads as that switch's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalMethods {
    pub passkey: bool,
    pub password: bool,
    pub totp: bool,
    pub magic_link: bool,
}

/// Q2: passkey, password and TOTP on; the magic link off, since it needs email (M1).
impl Default for LocalMethods {
    fn default() -> Self {
        Self {
            passkey: true,
            password: true,
            totp: true,
            magic_link: false,
        }
    }
}

impl LocalMethods {
    /// Whether `method`'s switch is on.
    #[must_use]
    pub fn is_on(&self, method: Method) -> bool {
        match method {
            Method::Passkey => self.passkey,
            Method::Password => self.password,
            Method::Totp => self.totp,
            Method::MagicLink => self.magic_link,
        }
    }
}

/// The organization's local sign-in: the local row's `enabled` and the method switches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalSignIn {
    pub enabled: bool,
    pub methods: LocalMethods,
}

impl LocalSignIn {
    /// Whether the organization offers `method`: the local row is enabled and the switch is on.
    #[must_use]
    pub fn offers(&self, method: Method) -> bool {
        self.enabled && self.methods.is_on(method)
    }

    /// Whether the organization offers a primary local method, one that signs in by itself:
    /// the passkey, the password or the magic link. TOTP is only a second step after a password.
    #[must_use]
    pub fn has_primary(&self) -> bool {
        Method::PRIMARY
            .into_iter()
            .any(|method| self.offers(method))
    }

    /// The methods the organization offers, in [`Method::ALL`]'s order. None when no primary
    /// method is offered (the local row disabled, or only TOTP on), and local sign-in is then not
    /// offered at all (B2, docs/specs/m0/08-sessions-and-csrf.md B38).
    #[must_use]
    pub fn offered(&self) -> Vec<Method> {
        if !self.has_primary() {
            return Vec::new();
        }
        Method::ALL
            .into_iter()
            .filter(|method| self.offers(*method))
            .collect()
    }
}

/// Why a local sign-in repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no row.
    #[error("the organization does not exist")]
    NotFound,
    /// The organization has no local provider row. [`ensure_row`] gives every organization one
    /// at creation and at startup, so this is a broken invariant, and the read fails closed.
    #[error("the organization has no local provider row")]
    RowMissing,
    /// The change switches the magic link on, which needs email (M1, spec 04 Q1, Q13).
    #[error("the magic link cannot be switched on before the server can send email")]
    MagicLinkNeedsEmail,
    /// The change would leave the organization no primary way to sign in: no offered passkey,
    /// password or magic link, and no enabled provider that is not local (B2).
    #[error("the change would leave the organization no way to sign in")]
    LastMethod,
    /// The stored settings object does not parse as [`OrgSettings`].
    #[error("the stored organization settings do not parse")]
    StoredSettings(#[source] serde_json::Error),
    /// Reading the other providers failed.
    #[error(transparent)]
    Providers(#[from] identity_providers::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// Give the scope's organization its local provider row, enabled, with an empty `config` and
/// stamped `now`, when it has none. Returns whether it inserted the row.
pub async fn ensure_row(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    now: SystemTime,
) -> Result<bool, Error> {
    let now = epoch_micros(now)?;
    let inserted = sqlx::query!(
        "INSERT INTO identity_providers (id, org_id, key, kind, config, enabled, created_at,
                                         updated_at)
         VALUES ($1, $2, $3, $4, '{}'::jsonb, true,
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $5::bigint * interval '1 microsecond')
         ON CONFLICT (org_id, key) DO NOTHING",
        new_id(),
        scope.org_id(),
        PROVIDER_KEY,
        Kind::Local.as_str(),
        now,
    )
    .execute(db)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

/// The scope's organization's local sign-in.
pub async fn get(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<LocalSignIn, Error> {
    let row = sqlx::query!(
        r#"SELECT o.settings::text AS "settings!", p.enabled AS "enabled?"
           FROM organizations o
           LEFT JOIN identity_providers p ON p.org_id = o.id AND p.key = $2
           WHERE o.id = $1"#,
        scope.org_id(),
        PROVIDER_KEY,
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    from_row(&row.settings, row.enabled)
}

/// [`get`], locking the organization's row `FOR NO KEY UPDATE` until the caller's transaction
/// ends: the lock every settings write takes, so a check on the state read here (the escalation
/// rule of the route) holds until the caller's [`update`] commits.
pub async fn get_for_update(
    conn: &mut PgConnection,
    scope: OrgScope,
) -> Result<LocalSignIn, Error> {
    let row = sqlx::query!(
        r#"SELECT o.settings::text AS "settings!", p.enabled AS "enabled?"
           FROM organizations o
           LEFT JOIN identity_providers p ON p.org_id = o.id AND p.key = $2
           WHERE o.id = $1
           FOR NO KEY UPDATE OF o"#,
        scope.org_id(),
        PROVIDER_KEY,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::NotFound)?;
    from_row(&row.settings, row.enabled)
}

fn from_row(settings: &str, enabled: Option<bool>) -> Result<LocalSignIn, Error> {
    let settings: OrgSettings = serde_json::from_str(settings).map_err(Error::StoredSettings)?;
    Ok(LocalSignIn {
        enabled: enabled.ok_or(Error::RowMissing)?,
        methods: settings.local_sign_in,
    })
}

/// Set the scope's organization's local sign-in to `new`, by `actor` at `now`, and return it.
///
/// Locks the organization's row ([`get_for_update`]) and compares `new` with the state read
/// under the lock: a call that changes nothing writes nothing. Otherwise it refuses, writing
/// nothing, a `new` with the magic link on ([`Error::MagicLinkNeedsEmail`]) and a `new` that
/// would leave the organization no primary way to sign in ([`Error::LastMethod`]). The switches,
/// when they change, are merged into the settings with an `organization.update_settings` entry
/// holding `local_sign_in` before and after; the row's `enabled`, when it changes, is written
/// with an `identity_provider.update` entry holding it before and after. Runs in one
/// transaction (a savepoint when `db` is already one).
pub async fn update<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    new: LocalSignIn,
    now: SystemTime,
    actor: Actor,
) -> Result<LocalSignIn, Error> {
    let mut tx = db.begin().await?;
    let current = get_for_update(&mut tx, scope).await?;
    if current == new {
        return Ok(current);
    }
    if new.methods.magic_link {
        return Err(Error::MagicLinkNeedsEmail);
    }
    refuse_lockout(&mut tx, scope, &new).await?;
    if current.methods != new.methods {
        set_methods(&mut tx, scope, current.methods, new.methods, now, actor).await?;
    }
    set_enabled(&mut tx, scope, new.enabled, now, actor).await?;
    tx.commit().await?;
    Ok(new)
}

/// [`Error::LastMethod`] when `new` offers no primary local method and the organization has no
/// enabled provider that is not local (B2). Runs under the organization row's lock that
/// [`update`] takes.
async fn refuse_lockout(
    conn: &mut PgConnection,
    scope: OrgScope,
    new: &LocalSignIn,
) -> Result<(), Error> {
    if new.has_primary() {
        return Ok(());
    }
    if identity_providers::list_enabled(&mut *conn, scope)
        .await?
        .is_empty()
    {
        return Err(Error::LastMethod);
    }
    Ok(())
}

/// Merge `after` into the settings as `local_sign_in`, with the `organization.update_settings`
/// entry holding the key before and after (spec 09 B22).
async fn set_methods(
    conn: &mut PgConnection,
    scope: OrgScope,
    before: LocalMethods,
    after: LocalMethods,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    // Invariant: `LocalMethods` is a struct of four booleans, which serde_json always serializes.
    let value = serde_json::to_value(after).expect("the switches serialize to JSON");
    sqlx::query!(
        "UPDATE organizations
         SET settings = settings || jsonb_build_object('local_sign_in', $2::text::jsonb),
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE id = $1",
        scope.org_id(),
        value.to_string(),
        now,
    )
    .execute(&mut *conn)
    .await?;
    let data = json!({
        "before": { "local_sign_in": before },
        "after": { "local_sign_in": after },
    });
    let action = Action::OrganizationUpdateSettings;
    audit::record(conn, scope, actor, action, scope.org_id(), &data, now).await?;
    Ok(())
}

/// Set the local row's `enabled`, with its audit entry, when it differs from `enabled`.
async fn set_enabled(
    conn: &mut PgConnection,
    scope: OrgScope,
    enabled: bool,
    now: SystemTime,
    actor: Actor,
) -> Result<(), Error> {
    let now = epoch_micros(now)?;
    let row = sqlx::query!(
        "SELECT id, enabled FROM identity_providers WHERE org_id = $1 AND key = $2
         FOR NO KEY UPDATE",
        scope.org_id(),
        PROVIDER_KEY,
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::RowMissing)?;
    let (id, before) = (row.id, row.enabled);
    if before == enabled {
        return Ok(());
    }
    sqlx::query!(
        "UPDATE identity_providers
         SET enabled = $3, updated_at = timestamptz 'epoch' + $4::bigint * interval '1 microsecond'
         WHERE org_id = $1 AND id = $2",
        scope.org_id(),
        id,
        enabled,
        now,
    )
    .execute(&mut *conn)
    .await?;
    let data = json!({ "before": { "enabled": before }, "after": { "enabled": enabled } });
    let action = Action::IdentityProviderUpdate;
    audit::record(conn, scope, actor, action, id, &data, now).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_q2s() {
        let methods = LocalMethods::default();
        assert!(methods.passkey && methods.password && methods.totp);
        assert!(!methods.magic_link);
    }

    #[test]
    fn a_stored_object_missing_a_switch_reads_its_default() {
        let methods: LocalMethods = serde_json::from_str(r#"{"passkey": false}"#).unwrap();
        assert_eq!(
            methods,
            LocalMethods {
                passkey: false,
                ..LocalMethods::default()
            }
        );
    }

    #[test]
    fn a_method_is_offered_only_with_the_row_enabled_and_its_switch_on() {
        let methods = LocalMethods {
            passkey: true,
            password: false,
            totp: true,
            magic_link: false,
        };
        let on = LocalSignIn {
            enabled: true,
            methods,
        };
        assert!(on.offers(Method::Passkey));
        assert!(!on.offers(Method::Password));
        assert_eq!(on.offered(), [Method::Passkey, Method::Totp]);
        let off = LocalSignIn {
            enabled: false,
            methods,
        };
        assert!(Method::ALL.iter().all(|m| !off.offers(*m)));
        assert!(off.offered().is_empty());
    }

    #[test]
    fn totp_alone_offers_nothing() {
        let totp_only = LocalSignIn {
            enabled: true,
            methods: LocalMethods {
                passkey: false,
                password: false,
                totp: true,
                magic_link: false,
            },
        };
        assert!(totp_only.offers(Method::Totp));
        assert!(!totp_only.has_primary());
        assert!(totp_only.offered().is_empty());
        let with_password = LocalSignIn {
            methods: LocalMethods {
                password: true,
                ..totp_only.methods
            },
            ..totp_only
        };
        assert!(with_password.has_primary());
        assert_eq!(with_password.offered(), [Method::Password, Method::Totp]);
    }

    #[test]
    fn every_method_names_its_switch() {
        let json = serde_json::to_value(LocalMethods::default()).unwrap();
        let object = json.as_object().unwrap();
        assert_eq!(object.len(), Method::ALL.len());
        for method in Method::ALL {
            assert!(object.contains_key(method.as_str()), "{}", method.as_str());
        }
    }
}
