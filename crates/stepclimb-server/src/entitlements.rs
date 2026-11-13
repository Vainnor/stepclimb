// SPDX-License-Identifier: AGPL-3.0-or-later
//! The request's entitlements: what its organization's plan allows
//! (docs/specs/m0/10-entitlements-framework.md B8 to B11).
//!
//! [`Entitlements`] is loaded once per request, on first use, with one query (`org_plans` joined
//! to `plans`, [`org_plans::get_with_plan`]) for the principal's organization, and kept in the
//! request's extensions under a type private to this module, so every `Require<Feature>` and the
//! handler's own [`Entitlements`] share it and nothing outside this module can plant one. Nothing
//! is kept across requests, so a plan change applies on the next request.
//!
//! Three checks read it:
//!
//! - `Require<F>` with `F: Feature` ([`crate::require`]): 403 `entitlement.feature_unavailable`
//!   when the plan does not include `F`;
//! - [`Entitlements::require`], the same check for an unauthenticated route, on entitlements it
//!   loads with [`Entitlements::load`] for the organization the request-organization resolver
//!   gives it;
//! - [`Entitlements::check_room`], the limit check a creating transaction runs before it
//!   inserts: 403 `entitlement.limit_reached` with the problem's `limit` and `max` members.
//!
//! Each fails closed: an organization without a plan row, a stored plan that does not parse or a
//! failed query answers 500, never a pass. M0 only refuses new units over a limit; it never
//! removes or hides what an organization already has (B11).

use std::sync::Arc;

use axum::{
    extract::FromRequestParts,
    http::{StatusCode, request::Parts},
};
use sqlx::{PgExecutor, Postgres, Transaction};
use stepclimb_core::{
    OrgScope,
    feature::{Feature, Limit},
    org_plans::{self, OrgPlan, Room},
};

use crate::{
    AppState,
    error::ApiError,
    require::{self, Rejection},
};

/// The codes this module produces.
pub mod code {
    /// The organization's plan does not include the feature the operation needs (403).
    pub const FEATURE_UNAVAILABLE: &str = "entitlement.feature_unavailable";
    /// The operation would take the organization over a limit of its plan (403).
    pub const LIMIT_REACHED: &str = "entitlement.limit_reached";
}

/// What an organization's plan allows, as one request loaded it.
#[derive(Clone, Debug)]
pub struct Entitlements {
    scope: OrgScope,
    plan: Arc<OrgPlan>,
}

impl Entitlements {
    /// The entitlements of `scope`'s organization, with one query. For an unauthenticated route,
    /// with the scope the request-organization resolver gives; an authenticated handler takes
    /// [`Entitlements`] as an extractor instead, which loads once per request. 500 when the
    /// organization has no plan or its plan cannot be read.
    pub async fn load(db: impl PgExecutor<'_>, scope: OrgScope) -> Result<Self, ApiError> {
        let plan = org_plans::get_with_plan(db, scope)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        Ok(Self {
            scope,
            plan: Arc::new(plan),
        })
    }

    /// The organization's plan: its key, source and what it allows.
    pub fn plan(&self) -> &OrgPlan {
        &self.plan
    }

    /// `Ok` when the plan includes `F`, otherwise 403 `entitlement.feature_unavailable`.
    pub fn require<F: Feature>(&self) -> Result<(), ApiError> {
        if self.plan.entitlements.has::<F>() {
            Ok(())
        } else {
            Err(ApiError::new(
                StatusCode::FORBIDDEN,
                code::FEATURE_UNAVAILABLE,
                "The organization's plan does not include the feature this operation needs.",
            ))
        }
    }

    /// `Ok` when `adding` more units of `limit` fit the plan, otherwise 403
    /// `entitlement.limit_reached` with `limit` and `max` (B10). Call it inside the transaction
    /// that inserts the units, before the insert, and commit that transaction after it: a limit
    /// with a value locks the organization's plan row until `tx` ends, so concurrent creations
    /// take turns and the second counts the first one's rows. A limit with no value runs no
    /// query.
    pub async fn check_room(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        limit: Limit,
        adding: u64,
    ) -> Result<(), ApiError> {
        let room = org_plans::check_room(tx, self.scope, &self.plan.entitlements, limit, adding)
            .await
            .map_err(|error| ApiError::internal(&error))?;
        match room {
            Room::Fits => Ok(()),
            Room::Full { max } => Err(ApiError::new(
                StatusCode::FORBIDDEN,
                code::LIMIT_REACHED,
                "The organization's plan does not allow more of what this operation adds.",
            )
            .with_limit(limit.key(), max)),
        }
    }
}

/// The entitlements of the request, kept in its extensions after the first load.
#[derive(Clone)]
struct Loaded(Entitlements);

/// The entitlements of the request's principal's organization: from the extensions when an
/// earlier extractor of the request loaded them, otherwise loaded now and kept there. No
/// principal answers the session extractor's 401.
pub(crate) async fn of_request(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Entitlements, Rejection> {
    if let Some(Loaded(entitlements)) = parts.extensions.get::<Loaded>() {
        return Ok(entitlements.clone());
    }
    let principal = require::principal(parts, state).await?;
    let entitlements = Entitlements::load(&state.db, principal.scope()).await?;
    parts.extensions.insert(Loaded(entitlements.clone()));
    Ok(entitlements)
}

/// For a handler that runs [`Entitlements::check_room`] or reads the plan. Authenticates the
/// request as `Require` does.
impl FromRequestParts<AppState> for Entitlements {
    type Rejection = Rejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        of_request(parts, state).await
    }
}
