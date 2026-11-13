// SPDX-License-Identifier: AGPL-3.0-or-later
//! The organization a request belongs to before any credential is read (spec 02 B11, spec 08
//! B5, Q1).
//!
//! M0 is single-organization (spec 08 Q1): with exactly one organization, every request belongs
//! to it whatever its `Host`. With none, the request answers 503 `org.none`; with two or more,
//! 404 `org.not_found`, so no request is ever matched to the wrong organization until
//! multi-organization resolution is built. Building that replaces [`resolve`] only.
//!
//! A build with the `test-support` cargo feature also answers the `x-stepclimb-test-org` header
//! (`TEST_ORG_HEADER`, defined only in that build) naming an organization, so the server's tests
//! can run several organizations in one database (spec 09 B20). Only the server's own tests
//! enable the feature, through its dev-dependency on itself; a build without dev-dependencies or
//! `--all-features` (`cargo build -p stepclimb-server`, the release image) never has it, and
//! there the header is ignored.
//!
//! `clippy.toml` lists [`resolve`] under `disallowed-methods`: its callers are the session module
//! (its extractor and sign-out) and the resolver of unauthenticated routes (spec 02 B11), each
//! through a `fn` with a reasoned `#[expect(clippy::disallowed_methods)]`. A handler takes its scope from the
//! credential instead.

use axum::http::{HeaderMap, StatusCode};
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

/// The header a `test-support` build reads to pick the request's organization: its id.
#[cfg(feature = "test-support")]
pub const TEST_ORG_HEADER: &str = "x-stepclimb-test-org";

/// The request's organization, as an instance scope. `headers` are read only by a
/// `test-support` build.
pub async fn resolve(db: impl PgExecutor<'_>, headers: &HeaderMap) -> Result<OrgScope, ApiError> {
    #[cfg(feature = "test-support")]
    if let Some(value) = headers.get(TEST_ORG_HEADER) {
        return named(db, value).await;
    }
    #[cfg(not(feature = "test-support"))]
    let _ = headers;
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
        Tenancy::Multiple => Err(not_found()),
    }
}

fn not_found() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        code::ORG_NOT_FOUND,
        "The request's organization cannot be determined.",
    )
}

/// The organization [`TEST_ORG_HEADER`] names: 404 `org.not_found` when the value is not the id
/// of an organization.
#[cfg(feature = "test-support")]
async fn named(
    db: impl PgExecutor<'_>,
    value: &axum::http::HeaderValue,
) -> Result<OrgScope, ApiError> {
    let org_id = value
        .to_str()
        .ok()
        .and_then(|value| Uuid::try_parse(value).ok())
        .ok_or_else(not_found)?;
    let scope = request_scope(org_id);
    match organizations::get(db, scope).await {
        Ok(_) => Ok(scope),
        Err(organizations::Error::NotFound) => Err(not_found()),
        Err(error) => Err(ApiError::internal(&error)),
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in the request-organization resolver"
)]
fn request_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}
