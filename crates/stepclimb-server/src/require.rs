// SPDX-License-Identifier: AGPL-3.0-or-later
//! `Require<R, T>`: the permission check every protected handler takes
//! (docs/specs/m0/09-permissions-and-roles.md B16 to B18), and the feature check of a route a
//! plan can switch off (docs/specs/m0/10-entitlements-framework.md B9).
//!
//! `R` is a permission of `stepclimb_core::perm` or a feature of `stepclimb_core::feature`: both
//! catalogs implement `stepclimb_core::requirement::Requirement`, whose `Kind` picks the check
//! ([`Check`]). A handler takes `Require<feature::X>` before `Require<perm::Y, T>`, and the
//! extractors run before the handler body, in B17's order:
//!
//! 1. no authenticated principal: the session extractor's 401 (`auth.unauthenticated`, or its
//!    more specific `auth.*` code);
//! 2. each `Require<F>` with `F: Feature`: 403 `entitlement.feature_unavailable` when the
//!    request's plan does not include `F` ([`crate::entitlements`]). It runs before any target is
//!    resolved, so the answer never depends on whether an id exists. A `Require<F>` that runs
//!    after a permission `Require` of the same request answers 500: the handler lists them in
//!    the wrong order, which every allowed request then shows;
//! 3. the target `T` is resolved inside the principal's organization; one that does not exist
//!    there answers 404 with the target's code;
//! 4. a target the principal holds no effective permission over answers the same 404;
//! 5. a visible target where `P` is not effective answers 403 `permission.denied`;
//! 6. otherwise the handler runs with the principal, the grants and the resolved target.
//!
//! The grants are loaded once per request (one query, `Grants::load`) and kept in the request's
//! extensions under a type private to this module, so a second `Require` in the same handler
//! reuses them and nothing outside this module can plant them. Nothing is kept across requests.

use std::{collections::BTreeSet, future::Future, marker::PhantomData, sync::Arc, sync::Mutex};

use axum::{
    extract::{FromRequestParts, RawPathParams},
    http::{StatusCode, request::Parts},
    response::{IntoResponse, Response},
};
use stepclimb_core::{
    feature::Feature,
    grants::{self, Denied, Grants, RequireError},
    id::Uuid,
    perm::Permission,
    requirement::{FeatureKind, PermissionKind, Requirement},
};

use crate::{
    AppState, entitlements,
    error::ApiError,
    request_org,
    session::{self, Authenticated},
};

/// The codes this module produces.
pub mod code {
    /// The target is visible but the permission is not effective at a scope containing it (403).
    pub const PERMISSION_DENIED: &str = "permission.denied";
    /// The account does not exist in the organization or is not visible to the caller (404).
    pub const ACCOUNT_NOT_FOUND: &str = "account.not_found";
}

/// The path parameter [`AccountTarget`] and [`AccountParentTarget`] read.
pub const ACCOUNT_ID_PARAM: &str = "account_id";

/// A request whose principal holds the permission `R` at the target `T` resolves to (B17), or
/// whose organization's plan includes the feature `R` (spec 10 B9).
///
/// `T` defaults to [`OrgTarget`], the only target a feature takes. The handler reads who is
/// asking from [`Self::principal`], the grants (for filtering a list or checking a body target)
/// from [`Self::grants`], and the resolved target from [`Self::target`]. A feature's `Require`
/// resolves no target: its `target` is [`OrgTarget`] without a permission check, and its grants
/// are the request's, loaded once for every `Require`.
pub struct Require<R, T = OrgTarget> {
    principal: Authenticated,
    grants: Arc<Grants>,
    target: T,
    requirement: PhantomData<fn() -> R>,
}

impl<R, T> Require<R, T> {
    /// The authenticated principal; its scope is the organization every repository call of the
    /// request acts in.
    pub fn principal(&self) -> Authenticated {
        self.principal
    }

    /// The principal's grants for this request.
    pub fn grants(&self) -> &Grants {
        &self.grants
    }

    /// The resolved target.
    pub fn target(&self) -> &T {
        &self.target
    }
}

/// How a [`Require`] finds its target and decides B17 steps 3 to 5 for `P`.
pub trait Target: Sized + Send {
    /// The resolved target, or the 400, 404, 403 or 500 that refuses the request.
    fn resolve<P: Permission>(
        parts: &mut Parts,
        state: &AppState,
        grants: &Grants,
    ) -> impl Future<Output = Result<Self, ApiError>> + Send;
}

/// The principal's own organization. Steps 3 and 4 always pass: the organization is visible to
/// its members. `P` must be effective at organization scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OrgTarget;

impl Target for OrgTarget {
    async fn resolve<P: Permission>(
        _parts: &mut Parts,
        _state: &AppState,
        grants: &Grants,
    ) -> Result<Self, ApiError> {
        grants.check::<P>(&grants::Target::Organization)?;
        Ok(Self)
    }
}

/// The account named by the path parameter `account_id`, which `P` must be effective over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountTarget {
    pub id: Uuid,
    /// The ids on the account's path: the root first, the account itself last.
    pub path: Vec<Uuid>,
}

impl Target for AccountTarget {
    async fn resolve<P: Permission>(
        parts: &mut Parts,
        state: &AppState,
        grants: &Grants,
    ) -> Result<Self, ApiError> {
        let id = account_id(parts, state).await?;
        let path = ancestors_path(state, grants, id).await?;
        grants.check::<P>(&grants::Target::Account(path.clone()))?;
        Ok(Self { id, path })
    }
}

/// The account named by `account_id`, checked at its parent (for move and delete, spec 02
/// B23): `P` must be effective over the parent, so a holder at account A reaches only A's
/// strict descendants. The root has no parent: only a principal holding `P` at organization
/// scope passes, and any other gets 404.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountParentTarget {
    pub id: Uuid,
    /// `None` for the root.
    pub parent_id: Option<Uuid>,
}

impl Target for AccountParentTarget {
    async fn resolve<P: Permission>(
        parts: &mut Parts,
        state: &AppState,
        grants: &Grants,
    ) -> Result<Self, ApiError> {
        let id = account_id(parts, state).await?;
        let path = ancestors_path(state, grants, id).await?;
        let parent_id = check_at_parent::<P>(grants, path)?;
        Ok(Self { id, parent_id })
    }
}

/// [`AccountParentTarget`]'s check for the account whose path is `path` (the root first, the
/// account itself last): `P` must be effective over the account's parent. Returns the parent's
/// id, or `None` for the root, which passes only for a holder of `P` at organization scope; any
/// other caller gets [`Denied::AccountNotFound`] for it.
///
/// Public so a handler can repeat the check on a path it read again under the account tree's
/// lock (spec 02 B22, B23).
pub fn check_at_parent<P: Permission>(
    grants: &Grants,
    mut path: Vec<Uuid>,
) -> Result<Option<Uuid>, Denied> {
    path.pop();
    let Some(&parent_id) = path.last() else {
        if grants.holds::<P>(&grants::Target::Organization) {
            return Ok(None);
        }
        return Err(Denied::AccountNotFound);
    };
    grants.check::<P>(&grants::Target::Account(path))?;
    Ok(Some(parent_id))
}

/// For list routes and routes whose target is in the body: passes when `P` is effective at any
/// scope, otherwise 403. The handler then filters by `grants().scopes_with::<P>()`, or checks a
/// body target with `grants().require_at::<P>(..)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnyScope;

impl Target for AnyScope {
    async fn resolve<P: Permission>(
        _parts: &mut Parts,
        _state: &AppState,
        grants: &Grants,
    ) -> Result<Self, ApiError> {
        if grants.holds_anywhere::<P>() {
            Ok(Self)
        } else {
            Err(Denied::Forbidden.into())
        }
    }
}

/// The `account_id` path parameter: 400 when it is not a UUID, 500 when the route has none.
async fn account_id(parts: &mut Parts, state: &AppState) -> Result<Uuid, ApiError> {
    let params = RawPathParams::from_request_parts(parts, state)
        .await
        .map_err(|rejection| {
            if rejection.status().is_server_error() {
                ApiError::internal(&rejection)
            } else {
                ApiError::malformed("A path parameter is not valid for this operation.")
            }
        })?;
    let value = params
        .iter()
        .find_map(|(key, value)| (key == ACCOUNT_ID_PARAM).then_some(value))
        .ok_or_else(|| ApiError::internal(&MissingAccountIdParam))?;
    Uuid::try_parse(value)
        .map_err(|_| ApiError::malformed("A path parameter is not valid for this operation."))
}

/// A route took an account target without an `{account_id}` segment: a bug in the router.
#[derive(Debug, thiserror::Error)]
#[error("the route has no {ACCOUNT_ID_PARAM} path parameter for its account target")]
struct MissingAccountIdParam;

/// The path of the account `id` in the grants' organization, or 404 when it has none.
async fn ancestors_path(
    state: &AppState,
    grants: &Grants,
    id: Uuid,
) -> Result<Vec<Uuid>, ApiError> {
    stepclimb_core::accounts::ancestors_path(&state.db, grants.scope(), id)
        .await
        .map_err(|error| ApiError::internal(&error))?
        .ok_or_else(|| Denied::AccountNotFound.into())
}

impl From<Denied> for ApiError {
    fn from(denied: Denied) -> Self {
        match denied {
            Denied::AccountNotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                code::ACCOUNT_NOT_FOUND,
                "No account with this id exists, or you cannot see it.",
            ),
            Denied::OrganizationNotFound => ApiError::new(
                StatusCode::NOT_FOUND,
                request_org::code::ORG_NOT_FOUND,
                "No organization with this id exists, or you cannot see it.",
            ),
            Denied::Forbidden => ApiError::new(
                StatusCode::FORBIDDEN,
                code::PERMISSION_DENIED,
                "You do not have the permission this operation needs here.",
            ),
        }
    }
}

impl From<RequireError> for ApiError {
    fn from(error: RequireError) -> Self {
        match error {
            RequireError::Denied(denied) => denied.into(),
            RequireError::Account(error) => ApiError::internal(&error),
        }
    }
}

/// Why a [`Require`] refused the request.
#[derive(Debug)]
pub enum Rejection {
    /// No authenticated principal (step 1), answered as the session extractor answers it,
    /// clearing the cookie where it does.
    Unauthenticated(session::Rejection),
    /// A feature the plan lacks (step 2), steps 3 to 5, or a failure loading the grants or the
    /// entitlements.
    Refused(ApiError),
}

impl From<ApiError> for Rejection {
    fn from(error: ApiError) -> Self {
        Self::Refused(error)
    }
}

impl IntoResponse for Rejection {
    fn into_response(self) -> Response {
        match self {
            Self::Unauthenticated(rejection) => rejection.into_response(),
            Self::Refused(error) => error.into_response(),
        }
    }
}

/// The principal and grants of the request, kept in its extensions after the first `Require`.
#[derive(Clone)]
struct Evaluated {
    principal: Authenticated,
    grants: Arc<Grants>,
}

/// Kept in the request's extensions once a permission `Require` has started, so a feature
/// `Require` after it can tell it runs out of order.
#[derive(Clone, Copy)]
struct PermissionStarted;

/// The check a [`Require<R, T>`] runs, implemented on `R`'s [`Requirement::Kind`]: for
/// [`PermissionKind`] with any [`Target`], for [`FeatureKind`] with [`OrgTarget`] only. Public
/// because it bounds the extractor; nothing outside this module implements it.
pub trait Check<R, T>: Sized {
    /// The [`Require`], or the response that refuses the request.
    fn check(
        parts: &mut Parts,
        state: &AppState,
    ) -> impl Future<Output = Result<Require<R, T>, Rejection>> + Send;
}

impl<P, T> Check<P, T> for PermissionKind
where
    P: Permission,
    T: Target,
{
    async fn check(parts: &mut Parts, state: &AppState) -> Result<Require<P, T>, Rejection> {
        let Evaluated { principal, grants } = evaluate(parts, state).await?;
        parts.extensions.insert(PermissionStarted);
        let target = T::resolve::<P>(parts, state, &grants).await?;
        Ok(Require {
            principal,
            grants,
            target,
            requirement: PhantomData,
        })
    }
}

impl<F: Feature> Check<F, OrgTarget> for FeatureKind {
    async fn check(parts: &mut Parts, state: &AppState) -> Result<Require<F>, Rejection> {
        let Evaluated { principal, grants } = evaluate(parts, state).await?;
        if parts.extensions.get::<PermissionStarted>().is_some() {
            return Err(ApiError::internal(&FeatureAfterPermission(F::KEY)).into());
        }
        entitlements::of_request(parts, state).await?.check::<F>()?;
        Ok(Require {
            principal,
            grants,
            target: OrgTarget,
            requirement: PhantomData,
        })
    }
}

/// A handler lists `Require<feature::X>` after a permission `Require`: a bug in the handler.
#[derive(Debug, thiserror::Error)]
#[error("Require<{0}> runs after a permission Require; list feature checks first")]
struct FeatureAfterPermission(&'static str);

impl<R, T> FromRequestParts<AppState> for Require<R, T>
where
    R: Requirement,
    R::Kind: Check<R, T>,
    T: Send,
{
    type Rejection = Rejection;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        R::Kind::check(parts, state).await
    }
}

/// The request's authenticated principal, as [`Require`] finds it: loading the request's grants
/// with it on first use, so a later `Require` reuses both. No principal answers the session
/// extractor's 401.
pub(crate) async fn principal(
    parts: &mut Parts,
    state: &AppState,
) -> Result<Authenticated, Rejection> {
    Ok(evaluate(parts, state).await?.principal)
}

/// The request's principal and grants: from the extensions when an earlier `Require` of the
/// request loaded them, otherwise authenticated and loaded now.
async fn evaluate(parts: &mut Parts, state: &AppState) -> Result<Evaluated, Rejection> {
    if let Some(evaluated) = parts.extensions.get::<Evaluated>() {
        return Ok(evaluated.clone());
    }
    let principal = Authenticated::from_request_parts(parts, state)
        .await
        .map_err(Rejection::Unauthenticated)?;
    let grants = Grants::load(&state.db, principal.scope(), principal.user_id)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    for key in grants.ignored_keys() {
        warn_unknown_key_once(key);
    }
    let evaluated = Evaluated {
        principal,
        grants: Arc::new(grants),
    };
    parts.extensions.insert(evaluated.clone());
    Ok(evaluated)
}

/// The unknown permission keys this process has already logged.
static WARNED_KEYS: Mutex<BTreeSet<String>> = Mutex::new(BTreeSet::new());

/// Log a stored permission key the catalog does not know, the first time this process meets
/// it (B9). The key grants nothing either way.
fn warn_unknown_key_once(key: &str) {
    if first_sighting(&WARNED_KEYS, key) {
        tracing::warn!(
            key,
            "a role holds a permission key this release does not define; it grants nothing"
        );
    }
}

/// Whether `key` is new to `seen`, recording it. A poisoned lock still holds a valid set: the
/// only writer inserts one string.
fn first_sighting(seen: &Mutex<BTreeSet<String>>, key: &str) -> bool {
    let mut seen = seen
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    !seen.contains(key) && seen.insert(key.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_key_is_reported_once() {
        let seen = Mutex::new(BTreeSet::new());
        assert!(first_sighting(&seen, "course.retired"));
        assert!(!first_sighting(&seen, "course.retired"));
        assert!(first_sighting(&seen, "grade.retired"));
    }

    #[test]
    fn denials_map_to_their_status_and_code() {
        let not_found = ApiError::from(Denied::AccountNotFound);
        assert_eq!(not_found.status(), StatusCode::NOT_FOUND);
        assert_eq!(not_found.code(), "account.not_found");
        let other_org = ApiError::from(Denied::OrganizationNotFound);
        assert_eq!(other_org.status(), StatusCode::NOT_FOUND);
        assert_eq!(other_org.code(), "org.not_found");
        let forbidden = ApiError::from(RequireError::Denied(Denied::Forbidden));
        assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);
        assert_eq!(forbidden.code(), "permission.denied");
    }
}
