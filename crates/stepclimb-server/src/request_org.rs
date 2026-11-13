// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization a request belongs to before any credential is read (spec 02 B11, spec 08
//! B5, Q1).
//!
//! M0 is single-organization (spec 08 Q1): with exactly one organization, every request belongs
//! to it whatever its `Host`. With none, the request answers 503 `org.none`; with two or more,
//! 404 `org.not_found`, so no request is ever matched to the wrong organization until
//! multi-organization resolution is built. Building that replaces [`resolve`] only.
//!
//! `clippy.toml` lists [`resolve`] under `disallowed-methods`: its callers are the session
//! extractor and the resolver of unauthenticated routes (spec 02 B11), each through a `fn` with a
//! reasoned `#[expect(clippy::disallowed_methods)]`. A handler takes its scope from the
//! credential instead.

use axum::http::StatusCode;
use sqlx::PgExecutor;
use stepclimb_core::{
    OrgScope,
    id::Uuid,
    organizations::{self, Tenancy},
};

use crate::error::ApiError;

/// The codes this module produces.
pub mod code {
    /// The instance has no organization yet (503).
    pub const ORG_NONE: &str = "org.none";
    /// The request's organization cannot be determined (404).
    pub const ORG_NOT_FOUND: &str = "org.not_found";
}

/// The request's organization, as an instance scope.
pub async fn resolve(db: impl PgExecutor<'_>) -> Result<OrgScope, ApiError> {
    let tenancy = organizations::tenancy(db)
        .await
        .map_err(|error| ApiError::internal(&error))?;
    match tenancy {
        Tenancy::Single(org_id) => Ok(request_scope(org_id)),
        Tenancy::Empty => Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            code::ORG_NONE,
            "The instance has no organization yet. Create one with `stepclimb admin create`.",
        )),
        Tenancy::Multiple => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            code::ORG_NOT_FOUND,
            "The request's organization cannot be determined.",
        )),
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in the request-organization resolver"
)]
fn request_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}
