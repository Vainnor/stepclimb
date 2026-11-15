// SPDX-License-Identifier: AGPL-3.0-or-later
//! The admission switches (docs/specs/m0/07-admission.md B12, B20, B23, Q2).
//!
//! The switches are the `admission` key of [`OrgSettings`]: `signup_allowed` and `sync_only`,
//! both off unless stored on ([`AdmissionSwitches`]). [`update`] is their only writer; the
//! generic settings patch cannot name `admission` ([`crate::OrgSettingsPatch::WRITTEN_ELSEWHERE`]).
//! It takes an [`AdmissionPatch`] naming only the switches to change, so a change of one switch
//! never writes back a stale value of the other that another admin changed in the meantime.
//! Admission reads both at each sign-in (spec 07 B11, B20). Every query filters by the scope's
//! organization.

use std::time::SystemTime;

use serde_json::json;
use sqlx::{Acquire, PgConnection, PgExecutor, Postgres};

use crate::{
    OrgScope, OrgSettings,
    audit::{self, Action, Actor},
    db::{TimeOutOfRange, epoch_micros},
};

pub use crate::settings::AdmissionSwitches;

/// A change to the admission switches: `None` leaves a switch as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AdmissionPatch {
    pub signup_allowed: Option<bool>,
    pub sync_only: Option<bool>,
}

impl AdmissionPatch {
    /// `current` with the switches this patch names replaced.
    #[must_use]
    pub fn applied_to(self, current: AdmissionSwitches) -> AdmissionSwitches {
        AdmissionSwitches {
            signup_allowed: self.signup_allowed.unwrap_or(current.signup_allowed),
            sync_only: self.sync_only.unwrap_or(current.sync_only),
        }
    }
}

/// Why an admission settings repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no row.
    #[error("the organization does not exist")]
    NotFound,
    /// The stored settings object does not parse as [`OrgSettings`].
    #[error("the stored organization settings do not parse")]
    StoredSettings(#[source] serde_json::Error),
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The scope's organization's admission switches, each with its effective value.
pub async fn get(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<AdmissionSwitches, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT settings::text AS "settings!" FROM organizations WHERE id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    parse(&stored)
}

/// [`get`], locking the organization's row `FOR NO KEY UPDATE` until the caller's transaction
/// ends: the lock every settings write takes, so the switches read here are the ones a write in
/// the same transaction replaces.
async fn get_for_update(
    conn: &mut PgConnection,
    scope: OrgScope,
) -> Result<AdmissionSwitches, Error> {
    let stored = sqlx::query_scalar!(
        r#"SELECT settings::text AS "settings!" FROM organizations WHERE id = $1
           FOR NO KEY UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(&mut *conn)
    .await?
    .ok_or(Error::NotFound)?;
    parse(&stored)
}

fn parse(stored: &str) -> Result<AdmissionSwitches, Error> {
    let settings: OrgSettings = serde_json::from_str(stored).map_err(Error::StoredSettings)?;
    Ok(settings.admission)
}

/// Change the switches `patch` names in the scope's organization, by `actor` at `now`, and
/// return every switch after the change.
///
/// The call locks the organization's row and applies `patch` to the switches read under the
/// lock, so a switch it does not name keeps the value stored then. Unchanged (an empty patch,
/// or only current values), it writes nothing, neither the settings, nor `updated_at`, nor an
/// audit entry. Different, it merges the switches into the stored `admission` object, so a
/// member this release does not know stays, sets `updated_at` to `now` and writes the
/// `organization.update_settings` entry with `admission` before and after (spec 09 B22). Runs
/// in one transaction (a savepoint when `db` is already one).
pub async fn update<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    patch: AdmissionPatch,
    now: SystemTime,
    actor: Actor,
) -> Result<AdmissionSwitches, Error> {
    let now = epoch_micros(now)?;
    let mut tx = db.begin().await?;
    let current = get_for_update(&mut tx, scope).await?;
    let new = patch.applied_to(current);
    if current == new {
        return Ok(current);
    }
    // Invariant: `AdmissionSwitches` is a struct of booleans, which serde_json always
    // serializes.
    let value = serde_json::to_value(new).expect("the switches serialize to JSON");
    sqlx::query!(
        "UPDATE organizations
         SET settings = settings || jsonb_build_object(
                 'admission',
                 COALESCE(settings -> 'admission', '{}'::jsonb) || $2::text::jsonb),
             updated_at = timestamptz 'epoch' + $3::bigint * interval '1 microsecond'
         WHERE id = $1",
        scope.org_id(),
        value.to_string(),
        now,
    )
    .execute(&mut *tx)
    .await?;
    let data = json!({
        "before": { "admission": current },
        "after": { "admission": new },
    });
    let action = Action::OrganizationUpdateSettings;
    audit::record(&mut tx, scope, actor, action, scope.org_id(), &data, now).await?;
    tx.commit().await?;
    Ok(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_patch_replaces_only_the_switches_it_names() {
        let current = AdmissionSwitches {
            signup_allowed: false,
            sync_only: true,
        };
        assert_eq!(AdmissionPatch::default().applied_to(current), current);
        let open = AdmissionPatch {
            signup_allowed: Some(true),
            ..AdmissionPatch::default()
        };
        let after = open.applied_to(current);
        assert!(after.signup_allowed);
        assert!(after.sync_only);
        let off = AdmissionPatch {
            sync_only: Some(false),
            ..AdmissionPatch::default()
        };
        assert_eq!(
            off.applied_to(current),
            AdmissionSwitches {
                signup_allowed: false,
                sync_only: false
            }
        );
    }
}
