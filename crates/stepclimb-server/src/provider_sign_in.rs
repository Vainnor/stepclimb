// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sign-in with an outside identity provider: `GET /auth/{provider}/start`
//! (docs/specs/m0/05-oidc.md B3 to B6, B16).
//!
//! The browser opens the start route; the server answers 303 to the provider's authorization
//! endpoint and sets the flow cookie, and the provider later sends the browser back to
//! [`redirect_uri`] (the callback, spec 05 I3). The route needs no credential: it runs before
//! sign-in, in the organization `RequestOrg` resolves.
//!
//! In order, the route:
//!
//! 1. counts the request against the client address's sign-in budget (B16, spec 08 B22). Over
//!    it, the browser is sent to the sign-in page with `error=auth.rate_limited` and
//!    `Retry-After`, and nothing else runs;
//! 2. finds the provider by its key in the request's organization. No provider, a disabled one,
//!    or the local row is `auth.provider_not_found` (B3). Another organization's key is no
//!    provider: the lookup is scoped;
//! 3. reads its stored config and its discovery document, through the client for the
//!    instance's organization count ([`crate::providers`]) and the cache of Q5. A config that
//!    does not read, a refused, failed or mismatched discovery (B4), or a provider of a kind
//!    this route does not sign in with yet (`oauth2`, spec 06) is `auth.provider_unavailable`;
//! 4. generates the flow's `state`, `nonce` and PKCE verifier, stores them sealed in a sign-in
//!    flow of kind `oidc` that names the provider (spec 08 B17, B20), with the sanitized
//!    `return_to` (B19), and answers 303 to the authorization URL (B5) with the flow cookie.
//!
//! A failure a user can cause or see is a 303 to the web app's [`SIGN_IN_PAGE`] with an `error`
//! query parameter, since the route is a browser navigation; a server fault is 500 problem
//! details. No answer carries why a provider is unavailable: the server logs it, at warn, with
//! the provider's key and no URL or secret. The flow's values are in the `Location` and the
//! cookie only, never in a log line.

use axum::{
    extract::{FromRequestParts, State},
    http::{HeaderValue, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use stepclimb_auth::{
    oidc::{FlowSecrets, authorization_url},
    oidc_config::OidcConfig,
};
use stepclimb_core::{
    OrgScope,
    auth_flows::Kind as FlowKind,
    identity_providers::{self, IdentityProvider, Kind},
    organizations,
};
use url::Url;
use utoipa::IntoParams;

use crate::{
    AppState,
    error::ApiError,
    extract::{ApiPath, ApiQuery},
    flow::{self, FlowStart},
    rate_limit::{self, SignIn},
    request_org::RequestOrg,
    web_origin::WebOrigin,
};

/// The start route's path.
pub const START_PATH: &str = "/auth/{provider}/start";

/// The web app's sign-in page, where every refusal of the route lands with `?error=<code>`.
pub const SIGN_IN_PAGE: &str = "/sign-in";

/// The codes the route redirects with.
pub mod code {
    /// No enabled provider with this key in the request's organization (B3).
    pub const AUTH_PROVIDER_NOT_FOUND: &str = "auth.provider_not_found";
    /// The provider's discovery failed or was refused, or its config does not read (B4).
    pub const AUTH_PROVIDER_UNAVAILABLE: &str = "auth.provider_unavailable";
    /// The client address's sign-in budget is spent (B16).
    pub const AUTH_RATE_LIMITED: &str = crate::rate_limit::code::AUTH_RATE_LIMITED;
}

/// The path parameter of the start route.
#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Path)]
pub struct ProviderParam {
    /// The provider's key in the organization (`identity_providers.key`).
    pub provider: String,
}

/// The query of the start route.
#[derive(Clone, Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct StartQuery {
    /// The path on this site to land on after sign-in. Anything but a safe path on this host
    /// becomes `/`.
    pub return_to: Option<String>,
}

/// `<public URL>/auth/<key>/callback`, the URI the provider sends the browser back to and the
/// admin registers at the provider (B6). It is built from the configured public URL, never
/// from the request's `Host`, and the key is percent-encoded as one path segment.
pub fn redirect_uri(public_url: &WebOrigin, key: &str) -> String {
    // A `WebOrigin` is an `http` or `https` origin that parsed as a URL, and such a URL has
    // path segments, so neither `expect` can fail.
    let mut url = Url::parse(public_url.as_str()).expect("a web origin is an absolute URL");
    url.path_segments_mut()
        .expect("an http(s) URL has a path")
        .pop_if_empty()
        .extend(["auth", key, "callback"]);
    url.into()
}

/// The 303 to the sign-in page with `error=<code>`.
fn to_sign_in(error: &'static str) -> Response {
    let location = format!("{SIGN_IN_PAGE}?error={error}");
    redirect(&location)
}

/// A 303 to `location` that no cache keeps: the authorization URL carries the flow's state.
fn redirect(location: &str) -> Response {
    let Ok(value) = HeaderValue::from_str(location) else {
        // A URL the url crate serialized is ASCII without control characters.
        return ApiError::internal(&InvalidLocation).into_response();
    };
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, value),
            (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        ],
    )
        .into_response()
}

#[derive(Debug, thiserror::Error)]
#[error("the redirect target is not a valid header value")]
struct InvalidLocation;

/// The sign-in budget, counted first (B16). Refused, the browser goes to the sign-in page with
/// `error=auth.rate_limited` and `Retry-After`.
#[derive(Debug, Clone, Copy)]
pub struct NavigationLimited;

impl FromRequestParts<AppState> for NavigationLimited {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Response> {
        match rate_limit::count::<SignIn>(parts, state) {
            Ok(Ok(_client)) => Ok(Self),
            Ok(Err(limited)) => {
                let mut response = to_sign_in(code::AUTH_RATE_LIMITED);
                response.headers_mut().insert(
                    header::RETRY_AFTER,
                    HeaderValue::from(limited.retry_after()),
                );
                Err(response)
            }
            Err(fault) => Err(fault.into_response()),
        }
    }
}

/// Start a sign-in with an outside identity provider.
#[utoipa::path(
    get,
    path = "/auth/{provider}/start",
    operation_id = "startProviderSignIn",
    tag = "auth",
    description = "A browser navigation. Sends the browser to the provider's authorization \
endpoint with `state`, `nonce` and a PKCE S256 challenge, bound to the sign-in flow whose \
`__Host-sc_flow` cookie this answer sets. Every refusal is a 303 to `/sign-in?error=<code>`: \
`auth.provider_not_found` (no enabled provider with this key), `auth.provider_unavailable` (the \
provider's discovery document could not be read or names another issuer) or \
`auth.rate_limited` (with `Retry-After`).",
    params(ProviderParam, StartQuery),
    responses(
        (
            status = 303,
            description = "To the provider's authorization endpoint, or to `/sign-in?error=<code>`",
            headers(
                ("Location" = String, description = "The authorization URL, or the sign-in page with the error code"),
                ("Set-Cookie" = String, description = "To the provider: the `__Host-sc_flow` cookie of the sign-in flow"),
                ("Retry-After" = i64, description = "With `auth.rate_limited`: seconds to wait, at least 1"),
                ("Cache-Control" = String, description = "`no-store`")
            )
        ),
        (status = 400, response = crate::openapi::BadRequest),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn start_provider_sign_in(
    _limited: NavigationLimited,
    org: RequestOrg,
    State(state): State<AppState>,
    ApiPath(ProviderParam { provider: key }): ApiPath<ProviderParam>,
    ApiQuery(query): ApiQuery<StartQuery>,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    let Some(provider) = sign_in_provider(&state, scope, &key).await? else {
        return Ok(to_sign_in(code::AUTH_PROVIDER_NOT_FOUND));
    };
    let Some(authorization) = authorize(&state, &provider, &key).await? else {
        return Ok(to_sign_in(code::AUTH_PROVIDER_UNAVAILABLE));
    };
    let start = FlowStart {
        kind: FlowKind::Oidc,
        user_id: None,
        provider_id: Some(provider.id),
        payload: &authorization.secrets.to_payload(),
        return_to: query.return_to.as_deref(),
    };
    let started = flow::start(&state.db, &state.keys, scope, start, state.clock.now())
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let mut response = redirect(authorization.url.as_str());
    response
        .headers_mut()
        .append(header::SET_COOKIE, started.set_cookie);
    Ok(response)
}

/// The enabled outside provider with `key` in the scope's organization, or `None` (B3). The
/// local row is not one: its methods have their own routes.
async fn sign_in_provider(
    state: &AppState,
    scope: OrgScope,
    key: &str,
) -> Result<Option<IdentityProvider>, ApiError> {
    let provider = match identity_providers::get_by_key(&state.db, scope, key).await {
        Ok(provider) => provider,
        // A key holding NUL names no provider.
        Err(identity_providers::Error::ContainsNul { .. }) => None,
        Err(error) => return Err(ApiError::internal(&error)),
    };
    Ok(provider.filter(|p| p.enabled && p.kind != Kind::Local))
}

/// What the redirect to the provider needs.
struct Authorization {
    url: Url,
    secrets: FlowSecrets,
}

/// The authorization URL for `provider` with new flow secrets, or `None` when the provider is
/// unavailable, logged with its key (B4).
async fn authorize(
    state: &AppState,
    provider: &IdentityProvider,
    key: &str,
) -> Result<Option<Authorization>, ApiError> {
    if provider.kind != Kind::Oidc {
        tracing::warn!(
            provider = key,
            kind = provider.kind.as_str(),
            "this release signs in only with oidc providers"
        );
        return Ok(None);
    }
    let config = match OidcConfig::from_stored(&provider.config) {
        Ok(config) => config,
        Err(error) => {
            tracing::error!(provider = key, %error, "the provider's stored config does not read");
            return Ok(None);
        }
    };
    let tenancy = organizations::tenancy(&state.db)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let client = state.providers.client(tenancy);
    let issuer = &config.settings.issuer;
    let now = state.clock.now();
    let metadata = match state
        .providers
        .discovery()
        .get(client, provider.id, issuer, now)
        .await
    {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(provider = key, %error, "the provider's discovery document was not accepted");
            return Ok(None);
        }
    };
    let secrets = FlowSecrets::generate().map_err(|e| ApiError::internal(&e))?;
    let redirect_uri = redirect_uri(&state.public_url, key);
    let url = authorization_url(&metadata, &config.settings, &redirect_uri, &secrets)
        .map_err(|e| ApiError::internal(&e))?;
    Ok(Some(Authorization { url, secrets }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_redirect_uri_is_the_public_url_and_the_encoded_key() {
        let public = WebOrigin::parse("https://lms.example.org").unwrap();
        assert_eq!(
            redirect_uri(&public, "acme"),
            "https://lms.example.org/auth/acme/callback"
        );
        assert_eq!(
            redirect_uri(&public, "a/b?c"),
            "https://lms.example.org/auth/a%2Fb%3Fc/callback"
        );
        let local = WebOrigin::parse("http://localhost:8080/").unwrap();
        assert_eq!(
            redirect_uri(&local, "idp"),
            "http://localhost:8080/auth/idp/callback"
        );
    }

    #[test]
    fn a_refusal_lands_on_the_sign_in_page_with_its_code() {
        let response = to_sign_in(code::AUTH_PROVIDER_NOT_FOUND);
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            response.headers()[header::LOCATION],
            "/sign-in?error=auth.provider_not_found"
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
