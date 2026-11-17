// SPDX-License-Identifier: AGPL-3.0-or-later
//! Each organization's plan: the `org_plans` repository
//! (docs/specs/m0/10-entitlements-framework.md B5, B6, Data).
//!
//! Every organization has exactly one row, keyed by `org_id`. [`crate::organizations::create`]
//! inserts it with the organization, and the startup task of `stepclimb serve` calls
//! [`ensure_selfhost`] for every organization so one created before the plans migration gets
//! it too. M0 never changes a row: `ends_at` and `stripe_sub` are M6's and nothing here reads
//! or writes them (B6).
//!
//! [`check_room`] is the limit check (B10): the creating transaction calls it before it inserts,
//! and it locks the organization's row so concurrent creations take turns.
//!
//! The lock makes the count current only at `READ COMMITTED`, PostgreSQL's default, where each
//! statement after the lock sees what the previous holder committed. At `REPEATABLE READ` or
//! `SERIALIZABLE` the transaction's snapshot is taken before the lock wait ends, so the count
//! would miss the previous holder's rows and both creations could pass. A database or role
//! default, or a caller's own transaction, can raise the level, so the statement that takes the
//! lock also reads `transaction_isolation`, and any other level is refused with
//! [`Error::IsolationLevel`] before anything is counted.

use std::time::SystemTime;

use sqlx::{Acquire, PgExecutor, Postgres, Transaction};

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros},
    feature::Limit,
    files,
    plans::{self, PlanEntitlements, UNLIMITED},
    users, videos,
};

/// How an organization got its plan. The stored spellings are [`PlanSource::as_str`]; the
/// `org_plans.source` check constraint lists the same four.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlanSource {
    /// A Stripe subscription (M6).
    Stripe,
    /// Granted by the operator (M6).
    Grant,
    /// The non-profit plan (M6).
    Nonprofit,
    /// The self-hosted `unlimited` plan.
    Selfhost,
}

impl PlanSource {
    /// Every source, in the order the check constraint lists them.
    pub const ALL: [Self; 4] = [Self::Stripe, Self::Grant, Self::Nonprofit, Self::Selfhost];

    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stripe => "stripe",
            Self::Grant => "grant",
            Self::Nonprofit => "nonprofit",
            Self::Selfhost => "selfhost",
        }
    }

    /// The source a stored spelling names, or `None` for any other string.
    #[must_use]
    pub fn parse(stored: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|source| source.as_str() == stored)
    }
}

/// An organization's plan.
#[derive(Clone, Debug, PartialEq)]
pub struct OrgPlan {
    /// The plan's key.
    pub plan: String,
    /// How the organization got it.
    pub source: PlanSource,
    /// What the plan allows.
    pub entitlements: PlanEntitlements,
}

/// Why an `org_plans` repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scope's organization has no `org_plans` row.
    #[error("the organization has no plan")]
    NotFound,
    /// The `unlimited` plan has no row yet: [`plans::sync_unlimited`] has not run.
    #[error("the unlimited plan does not exist")]
    UnlimitedMissing,
    /// The stored source is not one of [`PlanSource::ALL`].
    #[error("the stored plan source {0:?} is unknown")]
    StoredSource(String),
    #[error(transparent)]
    Plan(#[from] plans::Error),
    /// The transaction runs at an isolation level other than `READ COMMITTED`, where the limit
    /// check's lock does not make its count current (see the module documentation).
    #[error("the limit check runs only at READ COMMITTED isolation, not {found}")]
    IsolationLevel { found: String },
    /// Counting what a limit counts failed: the repository function's error.
    #[error("counting the units of the limit failed")]
    Count(#[source] Box<dyn std::error::Error + Send + Sync>),
    /// A time that cannot be stored.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

/// The scope's organization's plan, with one query (`org_plans` joined to `plans`).
pub async fn get_with_plan(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<OrgPlan, Error> {
    let row = sqlx::query!(
        r#"SELECT p.key, op.source, p.features::text AS "features!", p.limits::text AS "limits!"
           FROM org_plans op JOIN plans p ON p.id = op.plan_id
           WHERE op.org_id = $1"#,
        scope.org_id(),
    )
    .fetch_optional(db)
    .await?
    .ok_or(Error::NotFound)?;
    let source = PlanSource::parse(&row.source).ok_or(Error::StoredSource(row.source))?;
    let entitlements = PlanEntitlements::from_row(&row.key, &row.features, &row.limits)?;
    Ok(OrgPlan {
        plan: row.key,
        source,
        entitlements,
    })
}

/// Give the scope's organization the `unlimited` plan with source `selfhost`, stamped `now`,
/// when it has no `org_plans` row. An existing row, whatever plan it names, is left as it is
/// (B5). Returns whether it inserted the row. Refuses with [`Error::UnlimitedMissing`] when
/// [`plans::sync_unlimited`] has not written the plan yet.
pub async fn ensure_selfhost<'c>(
    db: impl Acquire<'c, Database = Postgres>,
    scope: OrgScope,
    now: SystemTime,
) -> Result<bool, Error> {
    let now = epoch_micros(now)?;
    let mut conn = db.acquire().await?;
    // Instance-wide on purpose, so it takes no `OrgScope` filter: plans belong to no
    // organization. It reads the `unlimited` plan's id only.
    let plan_id = sqlx::query_scalar!("SELECT id FROM plans WHERE key = $1", UNLIMITED)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or(Error::UnlimitedMissing)?;
    let inserted = sqlx::query!(
        "INSERT INTO org_plans (org_id, plan_id, source, created_at, updated_at)
         VALUES ($1, $2, $3,
                 timestamptz 'epoch' + $4::bigint * interval '1 microsecond',
                 timestamptz 'epoch' + $4::bigint * interval '1 microsecond')
         ON CONFLICT (org_id) DO NOTHING",
        scope.org_id(),
        plan_id,
        PlanSource::Selfhost.as_str(),
        now,
    )
    .execute(&mut *conn)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

/// Whether the units a creation adds fit the plan's limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    /// They fit, or the limit has no value.
    Fits,
    /// They would take the organization over `max`, the plan's value for the limit.
    Full {
        /// The plan's value for the limit.
        max: u64,
    },
}

/// Whether `adding` more units of `limit` fit `plan`, the scope's organization's plan as the
/// request loaded it (B10). Called inside the transaction that inserts the units, before the
/// insert.
///
/// A limit with no value (`null`, and every limit of `unlimited`) fits without a query. Otherwise
/// it locks the organization's `org_plans` row for the rest of `tx`, then counts with the
/// repository function the limit names. The lock makes concurrent creations in one
/// organization take turns: the second waits until the first commits or rolls back, and at
/// `READ COMMITTED` its count, a later statement, sees the first one's rows. Without it two
/// creations at `max - 1` would both count `max - 1` and both insert. A transaction at any other
/// isolation level is [`Error::IsolationLevel`], and an organization with no `org_plans` row is
/// [`Error::NotFound`]; neither is ever room. Nothing ties the caller's insert to `tx`: the
/// caller inserts in the same transaction and commits after it.
pub async fn check_room(
    tx: &mut Transaction<'_, Postgres>,
    scope: OrgScope,
    plan: &PlanEntitlements,
    limit: Limit,
    adding: u64,
) -> Result<Room, Error> {
    let Some(max) = plan.limit(limit) else {
        return Ok(Room::Fits);
    };
    let isolation = sqlx::query_scalar!(
        r#"SELECT current_setting('transaction_isolation') AS "isolation!"
           FROM org_plans WHERE org_id = $1 FOR UPDATE"#,
        scope.org_id(),
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(Error::NotFound)?;
    if isolation != "read committed" {
        return Err(Error::IsolationLevel { found: isolation });
    }
    let used = match limit {
        Limit::Users => users::count_for_limit(&mut **tx, scope)
            .await
            .map_err(|e| Error::Count(e.into()))?,
        Limit::Storage => files::count_for_limit(&mut **tx, scope)
            .await
            .map_err(|e| Error::Count(e.into()))?,
        Limit::VideoStorage => videos::count_for_limit(&mut **tx, scope)
            .await
            .map_err(|e| Error::Count(e.into()))?,
    };
    Ok(room(used, adding, max))
}

/// [`Room::Fits`] when `used + adding` is at most `max`. A negative count, which `count(*)`
/// never returns, and a sum past `u64::MAX` do not fit.
fn room(used: i64, adding: u64, max: u64) -> Room {
    match u64::try_from(used)
        .ok()
        .and_then(|used| used.checked_add(adding))
    {
        Some(total) if total <= max => Room::Fits,
        _ => Room::Full { max },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_source_round_trips_through_its_stored_spelling() {
        for source in PlanSource::ALL {
            assert_eq!(PlanSource::parse(source.as_str()), Some(source));
        }
        assert_eq!(PlanSource::parse("Selfhost"), None);
        assert_eq!(PlanSource::parse(""), None);
    }

    #[test]
    fn room_fits_up_to_max_and_no_further() {
        assert_eq!(room(0, 1, 2), Room::Fits);
        assert_eq!(room(1, 1, 2), Room::Fits);
        assert_eq!(room(2, 1, 2), Room::Full { max: 2 });
        assert_eq!(room(0, 3, 2), Room::Full { max: 2 });
        assert_eq!(room(0, 1, 0), Room::Full { max: 0 });
        assert_eq!(room(5, 0, 2), Room::Full { max: 2 });
        assert_eq!(room(2, 0, 2), Room::Fits);
        assert_eq!(room(1, u64::MAX, u64::MAX), Room::Full { max: u64::MAX });
        assert_eq!(room(-1, 0, 2), Room::Full { max: 2 });
    }

    #[test]
    fn the_stored_spellings_are_the_data_models() {
        let spellings: Vec<_> = PlanSource::ALL.iter().map(|s| s.as_str()).collect();
        assert_eq!(spellings, ["stripe", "grant", "nonprofit", "selfhost"]);
    }
}
