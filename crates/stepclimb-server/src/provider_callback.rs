// SPDX-License-Identifier: AGPL-3.0-or-later
//! The provider's redirect back: `GET /auth/{provider}/callback` (docs/specs/m0/05-oidc.md B7
//! to B11, B16; `06-oauth2-and-vatsim-preset.md` B7 to B10).
//!
//! The browser arrives here from the provider's authorization endpoint with `code` and `state`,
//! or with `error` and `state`. The route needs no credential: it runs before sign-in, in the
//! organization `RequestOrg` resolves. One route serves both provider kinds and picks the flow
//! by the provider's kind (spec 05, API); steps 1 to 5 are the same for both. In order, it:
//!
//! 1. counts the request against the client address's sign-in budget (B16), as the start route
//!    does;
//! 2. finds the provider by its key in the request's organization (B3). No provider is
//!    `auth.provider_not_found` and the flow is left alone;
//! 3. takes the sign-in flow of the provider's kind the `__Host-sc_flow` cookie names (spec 08
//!    B18). Taking it
//!    deletes it, so whatever follows, the flow is used once. No cookie, an unknown or used flow
//!    is `auth.flow_invalid`, an expired one `auth.flow_expired`. A flow started for another
//!    provider, or a `state` that is missing or not the flow's (compared in constant time), is
//!    `auth.flow_invalid` too (B7);
//! 4. answers a provider's `error` (B8): `access_denied` is `auth.provider_denied`, anything
//!    else `auth.provider_error`. The `error_description` is logged, cut to 200 characters,
//!    and never put in the redirect;
//! 5. refuses a callback with neither `error` nor `code` as `auth.provider_error`;
//! 6. for `oidc`, redeems the `code` and verifies the result
//!    ([`stepclimb_auth::oidc::redeem_code`]): a failed exchange, an unreadable key set or a
//!    userinfo answer that cannot be used is `auth.provider_error` (B9, B11); a refused ID token
//!    `auth.id_token_invalid` (B10), except a `sub` no identity can hold, which is
//!    `auth.subject_missing`. The verified claims are mapped by the provider's JSON Pointers
//!    ([`ProviderSignIn::map`], B12, B13), the email dropped unless the provider vouched for it.
//!    For `oauth2`, [`crate::oauth2_sign_in::finish`] redeems the code, calls userinfo and maps
//!    the body (spec 06 B8 to B10);
//! 7. hands the mapped sign-in, of either kind, to [`complete_sign_in`]: admission
//!    (`07-admission.md` B1) and, for an admitted user, the session in one transaction, then a
//!    303 to the flow's `return_to` with `__Host-sc_session` (B14). An admission refusal is its
//!    303 to the sign-in page with its code, and no session.
//!
//! The provider's tokens never leave [`stepclimb_auth`]: the callback gets the claims, and the
//! tokens are dropped when the code is redeemed (B15).
//!
//! A refusal is a 303 to the sign-in page with the code, logged at warn with the provider's key
//! and never with a URL, a token, the `code` or a secret. A server fault (the flow's payload or
//! the client secret does not open) is 500 problem details. Every answer from step 3 on clears
//! the flow cookie, a 500 included.
//!
//! The flow is taken before any request to the provider, in its own statement, rather than in
//! the transaction that later creates the session: no database connection is held while the
//! provider answers, and the provider's code is single use anyway, so a flow kept back after a
//! failure could not be finished.

use axum::{
    extract::State,
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use stepclimb_auth::{
    claims::MappedSignIn,
    oauth2_flow,
    oidc::{self, IdTokenError, ProviderSignIn},
    oidc_config::{OidcConfig, ProviderRow},
};
use stepclimb_core::{
    OrgScope,
    auth_flows::{Kind as FlowKind, ReturnTo},
    identity_providers::{IdentityProvider, Kind},
    organizations,
};
use utoipa::IntoParams;

use crate::{
    AppState,
    admission::{self, Completed, Completion, Context, Profile, Verified},
    audit::ClientAddress,
    error::ApiError,
    extract::{ApiPath, ApiQuery},
    flow::{self, Finished},
    oauth2_sign_in,
    provider_sign_in::{
        NavigationLimited, ProviderParam, redirect, redirect_uri, sign_in_provider, to_sign_in,
    },
};

/// The callback route's path, the `redirect_uri` of [`redirect_uri`].
pub const CALLBACK_PATH: &str = "/auth/{provider}/callback";

/// How much of a provider's `error` and `error_description` is logged (B8).
const LOGGED_CHARS: usize = 200;

/// The codes the route redirects with, beside the start route's.
pub mod code {
    pub use crate::flow::code::{AUTH_FLOW_EXPIRED, AUTH_FLOW_INVALID};
    pub use crate::provider_sign_in::code::{AUTH_PROVIDER_NOT_FOUND, AUTH_RATE_LIMITED};
    /// The person declined at the provider (`error=access_denied`, B8).
    pub const AUTH_PROVIDER_DENIED: &str = "auth.provider_denied";
    /// The provider answered with another error, or the token, key set or userinfo request
    /// failed (B8, B9, B11).
    pub const AUTH_PROVIDER_ERROR: &str = "auth.provider_error";
    /// The ID token failed a check of B10.
    pub const AUTH_ID_TOKEN_INVALID: &str = "auth.id_token_invalid";
    /// The ID token's `sub`, or the OAuth 2 userinfo body's subject, is not one an identity can
    /// hold (B12; spec 06 B6).
    pub const AUTH_SUBJECT_MISSING: &str = stepclimb_auth::claims::SUBJECT_MISSING;
}

/// The query the provider sends the browser back with (RFC 6749 4.1.2, 4.1.2.1). Other
/// parameters a provider adds (`iss`, `session_state`, `scope`) are ignored.
#[derive(Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct CallbackQuery {
    /// The authorization code, on success.
    pub code: Option<String>,
    /// The `state` the start route sent.
    pub state: Option<String>,
    /// The provider's error code, on failure.
    pub error: Option<String>,
    /// The provider's description of the error. Logged, never shown.
    pub error_description: Option<String>,
}

impl std::fmt::Debug for CallbackQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CallbackQuery")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// Finish a sign-in with an outside identity provider.
#[utoipa::path(
    get,
    path = "/auth/{provider}/callback",
    operation_id = "finishProviderSignIn",
    tag = "auth",
    description = "A browser navigation: the provider's redirect back after sign-in, with \
`code` and `state` or with `error` and `state`. It needs the `__Host-sc_flow` cookie of the \
flow the start route began, and clears it. Every refusal is a 303 to `/sign-in?error=<code>`: \
`auth.provider_not_found` (no enabled provider with this key), `auth.flow_invalid` (no flow \
cookie, a used or unknown flow, a flow for another provider, or another `state`), \
`auth.flow_expired` (the flow is older than 10 minutes), `auth.provider_denied` (the person \
declined at the provider), `auth.provider_error` (the provider reported an error, or its token, \
key set or userinfo answer could not be used), `auth.id_token_invalid` (the ID token failed a \
check), `auth.subject_missing` (the ID token's `sub` or the OAuth 2 userinfo subject is \
missing, empty, holds NUL or is longer than 255 characters), the admission refusals \
`auth.suspended`, `auth.admission_refused`, `auth.signup_closed` and \
`entitlement.limit_reached`, or `auth.rate_limited` \
(with `Retry-After`). An admitted sign-in sets `__Host-sc_session` and is a 303 to the flow's \
`return_to`.",
    params(ProviderParam, CallbackQuery),
    responses(
        (
            status = 303,
            description = "To the flow's `return_to`, signed in, or to `/sign-in?error=<code>`",
            headers(
                ("Location" = String, description = "The `return_to` path, or the sign-in page with the error code"),
                ("Set-Cookie" = String, description = "Clears the `__Host-sc_flow` cookie; signed in, also sets `__Host-sc_session`"),
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
pub async fn finish_provider_sign_in(
    _limited: NavigationLimited,
    org: crate::request_org::RequestOrg,
    State(state): State<AppState>,
    ApiPath(ProviderParam { provider: key }): ApiPath<ProviderParam>,
    ApiQuery(query): ApiQuery<CallbackQuery>,
    client: ClientAddress,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let scope = org.scope();
    let Some(provider) = sign_in_provider(&state, scope, &key).await? else {
        return Ok(to_sign_in(code::AUTH_PROVIDER_NOT_FOUND));
    };
    let flow_kind = match provider.kind {
        Kind::Oidc => FlowKind::Oidc,
        Kind::Oauth2 => FlowKind::Oauth2,
        // `sign_in_provider` never returns the local row.
        Kind::Local => return Ok(to_sign_in(code::AUTH_PROVIDER_NOT_FOUND)),
    };
    let now = state.clock.now();
    let taken = flow::finish(&state.db, &state.keys, scope, &headers, flow_kind, now).await;
    let request = Request {
        scope,
        client,
        headers: &headers,
    };
    // From here on every answer clears the flow cookie, a server fault's 500 included: the flow
    // is taken, or its row is unusable.
    let response = match taken {
        Ok(flow) => after_flow(&state, &request, &provider, &key, flow, &query)
            .await
            .unwrap_or_else(IntoResponse::into_response),
        Err(error) => match error.code() {
            Some(code) => to_sign_in(code),
            None => ApiError::internal(&error).into_response(),
        },
    };
    Ok(clearing_flow_cookie(response))
}

fn clearing_flow_cookie(mut response: Response) -> Response {
    response
        .headers_mut()
        .append(header::SET_COOKIE, flow::clear_cookie());
    response
}

/// What the callback knows of its request.
#[derive(Clone, Copy)]
pub(crate) struct Request<'a> {
    pub(crate) scope: OrgScope,
    /// The client address, for the audit entry of a just-in-time identity.
    pub(crate) client: ClientAddress,
    /// The request's headers, whose session cookies the new session ends (spec 08 B7).
    pub(crate) headers: &'a HeaderMap,
}

/// A taken flow's secrets, by the provider's kind. Each kind's type lives in its own module.
enum Secrets {
    Oidc(oidc::FlowSecrets),
    Oauth2(oauth2_flow::FlowSecrets),
}

impl Secrets {
    /// The secrets of `flow`, a flow of `provider`'s kind; a payload that does not read is a
    /// fault.
    fn open(provider: &IdentityProvider, flow: &Finished) -> Result<Self, ApiError> {
        let internal = |e: &dyn std::error::Error| ApiError::internal(e);
        match provider.kind {
            Kind::Oauth2 => oauth2_flow::FlowSecrets::from_payload(&flow.payload)
                .map(Self::Oauth2)
                .map_err(|e| internal(&e)),
            Kind::Oidc | Kind::Local => oidc::FlowSecrets::from_payload(&flow.payload)
                .map(Self::Oidc)
                .map_err(|e| internal(&e)),
        }
    }

    /// Whether `presented` is the flow's `state`, in constant time (B7).
    fn state_matches(&self, presented: &str) -> bool {
        match self {
            Self::Oidc(secrets) => secrets.state_matches(presented),
            Self::Oauth2(secrets) => secrets.state_matches(presented),
        }
    }
}

/// Steps 3 (the flow's provider and `state`) to 7, on a flow already taken, in the same order
/// for both kinds: the provider, the `state`, the provider's `error`, then the `code`.
async fn after_flow(
    state: &AppState,
    request: &Request<'_>,
    provider: &IdentityProvider,
    key: &str,
    flow: Finished,
    query: &CallbackQuery,
) -> Result<Response, ApiError> {
    if flow.provider_id != Some(provider.id) {
        tracing::warn!(
            provider = key,
            "the sign-in flow was started for another provider"
        );
        return Ok(to_sign_in(code::AUTH_FLOW_INVALID));
    }
    let secrets = Secrets::open(provider, &flow)?;
    if !query
        .state
        .as_deref()
        .is_some_and(|presented| secrets.state_matches(presented))
    {
        tracing::warn!(provider = key, "the callback's state is not the flow's");
        return Ok(to_sign_in(code::AUTH_FLOW_INVALID));
    }
    if let Some(error) = &query.error {
        return Ok(provider_refused(
            key,
            error,
            query.error_description.as_deref(),
        ));
    }
    let Some(code) = &query.code else {
        tracing::warn!(
            provider = key,
            "the callback has neither a code nor an error"
        );
        return Ok(to_sign_in(code::AUTH_PROVIDER_ERROR));
    };
    let scope = request.scope;
    match secrets {
        Secrets::Oidc(secrets) => {
            match redeem(state, scope, provider, key, &secrets, code).await? {
                Ok(sign_in) => {
                    complete_sign_in(state, request, provider, &flow.return_to, sign_in).await
                }
                Err(refusal) => Ok(to_sign_in(refusal)),
            }
        }
        Secrets::Oauth2(secrets) => {
            oauth2_sign_in::finish(
                state,
                request,
                provider,
                key,
                &secrets,
                code,
                &flow.return_to,
            )
            .await
        }
    }
}

/// The provider's `error` (B8): logged with its description, both cut to [`LOGGED_CHARS`], and
/// answered without either.
pub(crate) fn provider_refused(key: &str, error: &str, description: Option<&str>) -> Response {
    let cut = |text: &str| text.chars().take(LOGGED_CHARS).collect::<String>();
    tracing::warn!(
        provider = key,
        error = ?cut(error),
        description = ?description.map(cut),
        "the provider answered the sign-in with an error"
    );
    if error == "access_denied" {
        to_sign_in(code::AUTH_PROVIDER_DENIED)
    } else {
        to_sign_in(code::AUTH_PROVIDER_ERROR)
    }
}

/// Redeem `code` for the `oidc` `provider` (B9 to B11) and map the verified claims (B12, B13):
/// the mapped sign-in, or the code to refuse with.
async fn redeem(
    state: &AppState,
    scope: OrgScope,
    provider: &IdentityProvider,
    key: &str,
    secrets: &oidc::FlowSecrets,
    code: &str,
) -> Result<Result<MappedSignIn, &'static str>, ApiError> {
    let config = match OidcConfig::from_stored(&provider.config) {
        Ok(config) => config,
        Err(error) => {
            tracing::warn!(provider = key, %error, "the provider's stored config does not read");
            return Ok(Err(code::AUTH_PROVIDER_ERROR));
        }
    };
    let tenancy = organizations::tenancy(&state.db)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let client = state.providers.client(tenancy);
    let now = state.clock.now();
    let discovery = state.providers.discovery();
    let metadata = match discovery
        .get(client, provider.id, &config.settings.issuer, now)
        .await
    {
        Ok(metadata) => metadata,
        Err(error) => {
            tracing::warn!(provider = key, %error, "the provider's discovery document was not accepted");
            return Ok(Err(code::AUTH_PROVIDER_ERROR));
        }
    };
    let row = ProviderRow {
        org_id: scope.org_id(),
        provider_id: provider.id,
    };
    let client_secret = config
        .client_secret(&state.keys, row, state.providers.config_secrets())
        .map_err(|e| ApiError::internal(&e))?;
    let redirect_uri = redirect_uri(&state.public_url, key);
    let callback = oidc::CodeCallback {
        client,
        keys: state.providers.jwks(),
        provider_id: provider.id,
        metadata: &metadata,
        settings: &config.settings,
        client_secret: &client_secret,
        redirect_uri: &redirect_uri,
        secrets,
        code,
        now,
    };
    Ok(oidc::redeem_code(&callback)
        .await
        .map(|sign_in: ProviderSignIn| sign_in.map(&config.settings.claims))
        .map_err(|error| {
            tracing::warn!(provider = key, %error, "the provider sign-in was refused");
            refusal_code(&error)
        }))
}

/// The redirect code for a callback that could not be verified.
fn refusal_code(error: &oidc::CallbackError) -> &'static str {
    use oidc::CallbackError;
    match error {
        CallbackError::IdToken(IdTokenError::Subject) => code::AUTH_SUBJECT_MISSING,
        CallbackError::IdToken(_) => code::AUTH_ID_TOKEN_INVALID,
        CallbackError::Exchange(_) | CallbackError::Keys(_) | CallbackError::Userinfo(_) => {
            code::AUTH_PROVIDER_ERROR
        }
    }
}

/// Finish a mapped provider sign-in of either kind (B14; spec 06 B10): admission
/// (`07-admission.md` B1) and, for an admitted user, the session (spec 08 B7) in one
/// transaction, then a 303 to the flow's `return_to` with the session cookie. A refusal is
/// admission's 303 to the sign-in page with its code and no session; a fault rolls the
/// transaction back and is 500.
///
/// The flow was deleted when the route took it, before the provider was called, so it is not
/// in this transaction: a provider's code is single use, and a flow kept after a failure here
/// could not be finished anyway. `return_to` was sanitized again when the flow was read
/// (`stepclimb_core::auth_flows::ReturnTo`), so it is a path on this host.
pub(crate) async fn complete_sign_in(
    state: &AppState,
    request: &Request<'_>,
    provider: &IdentityProvider,
    return_to: &ReturnTo,
    sign_in: MappedSignIn,
) -> Result<Response, ApiError> {
    let MappedSignIn { subject, profile } = sign_in;
    let verified = Verified {
        provider,
        subject: &subject,
        profile: Profile {
            given_name: profile.given_name,
            family_name: profile.family_name,
            email: profile.email,
        },
        attributes: profile.attributes,
    };
    let context = Context {
        scope: request.scope,
        client: request.client,
        now: state.clock.now(),
    };
    let mut tx = state.db.begin().await.map_err(|e| ApiError::internal(&e))?;
    match admission::complete(&mut tx, &verified, &context, request.headers).await {
        Ok(Completed { session, .. }) => {
            tx.commit().await.map_err(|e| ApiError::internal(&e))?;
            let mut response = redirect(return_to.as_str());
            response
                .headers_mut()
                .append(header::SET_COOKIE, session.set_cookie);
            Ok(response)
        }
        // Admission rolled its own writes back; the transaction holds only the refusal marker
        // a refusal by the rules records (spec 07 I3), which must be kept.
        Err(Completion::Refused(refusal)) => {
            tx.commit().await.map_err(|e| ApiError::internal(&e))?;
            Ok(refusal.redirect())
        }
        Err(Completion::Failed(error)) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use stepclimb_auth::oidc::{CallbackError, ExchangeError, UserinfoError};

    use super::*;

    #[test]
    fn a_refused_sub_is_subject_missing_and_other_token_refusals_id_token_invalid() {
        assert_eq!(
            refusal_code(&CallbackError::IdToken(IdTokenError::Subject)),
            "auth.subject_missing"
        );
        assert_eq!(
            refusal_code(&CallbackError::IdToken(IdTokenError::Nonce)),
            "auth.id_token_invalid"
        );
        assert_eq!(
            refusal_code(&CallbackError::Exchange(ExchangeError::NoIdToken)),
            "auth.provider_error"
        );
        assert_eq!(
            refusal_code(&CallbackError::Userinfo(UserinfoError::SubjectMismatch)),
            "auth.provider_error"
        );
    }

    #[test]
    fn access_denied_is_provider_denied_and_any_other_error_provider_error() {
        let location = |error| {
            provider_refused("k", error, Some("why"))
                .headers()
                .get(header::LOCATION)
                .cloned()
        };
        assert_eq!(
            location("access_denied").unwrap(),
            "/sign-in?error=auth.provider_denied"
        );
        for other in [
            "server_error",
            "Access_Denied",
            "",
            "temporarily_unavailable",
        ] {
            assert_eq!(
                location(other).unwrap(),
                "/sign-in?error=auth.provider_error",
                "{other}"
            );
        }
    }

    #[test]
    fn debug_output_of_the_query_leaves_the_code_out() {
        let query = CallbackQuery {
            code: Some("code-zq".into()),
            state: Some("state-zq".into()),
            error: None,
            error_description: None,
        };
        assert!(!format!("{query:?}").contains("zq"));
    }

    #[test]
    fn every_answer_after_the_flow_clears_its_cookie() {
        let response = clearing_flow_cookie(to_sign_in(code::AUTH_FLOW_INVALID));
        assert_eq!(
            response.headers()[header::SET_COOKIE],
            HeaderValue::from_static(
                "__Host-sc_flow=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax"
            )
        );
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    }
}
