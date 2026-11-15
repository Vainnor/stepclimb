// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OAuth 2.1 token and revocation endpoints for Stepclimb's own apps: `POST /oauth/token`
//! and `POST /oauth/revoke` (docs/specs/m0/08-sessions-and-csrf.md B30, B32 to B37, B41, Q11 to
//! Q14).
//!
//! The app calls both with an `application/x-www-form-urlencoded` body and no credential beyond
//! what the body carries: the clients are public. Neither endpoint reads a cookie or an
//! `Authorization` header, so the CSRF layer lets their posts through (B16), and they send no
//! CORS headers: the app calls them from its native HTTP client, not from a web page. Each
//! counts the request against the client address's token budget first (`RateLimited<Token>`,
//! B41), then resolves the request's organization, which every lookup is scoped to. Their
//! answers carry `Cache-Control: no-store` and `Pragma: no-cache` (RFC 6749 section 5.1), and
//! their refusals are RFC 6749 section 5.2 JSON, `error` and a fixed `error_description`, status
//! 400 (Q13 (a)).
//!
//! A code exchange (`grant_type=authorization_code`, B32) runs in one transaction. It locks the
//! code's row, so a second exchange of the same code waits for the first. A code that was already
//! used is reuse: the family its first exchange started is revoked and audited. Otherwise the code
//! must be unexpired by the server's clock, issued to the same `client_id` for the same
//! `redirect_uri` (both compared exactly), its challenge must be the S256 of `code_verifier`
//! (compared in constant time), and its user must be active. Then a new refresh family starts
//! with the first refresh token, the code is marked used with that family, and the answer
//! carries an access token for 15 minutes and the refresh token. A code that fails a check is
//! left unused, so a request without the verifier cannot spend the app's code.
//!
//! A refresh (`grant_type=refresh_token`, B33, B34) also runs in one transaction: it locks the
//! presented token, which must be the same client's. One already rotated is reuse: its family is
//! revoked and audited (Q11: strict, no grace window). Otherwise it must be unexpired and its
//! user active (B11), and a new token rotated from it joins the family; if a concurrent refresh
//! rotated it first, that is reuse too. Every refusal of a code or refresh token is the same
//! `invalid_grant`.
//!
//! A revocation (B35, RFC 7009) deletes the family of the presented refresh token, spent or not,
//! or of the presented access token when it verifies, as long as the token was issued to the
//! request's `client_id` in the request's organization, and answers 200 whatever it found.
//!
//! No code, verifier or token reaches a log line: the request span leaves bodies out, and the
//! errors logged here name no request value.

use std::net::IpAddr;
use std::time::SystemTime;

use axum::{
    body::{Body, to_bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use stepclimb_auth::{
    oauth_server::{
        access_token::{self, Subject},
        clients::Client,
        token_request::{
            self, ErrorCode, INVALID_GRANT, MAX_BODY_LEN, Refusal, RevokeRequest, TokenRequest,
        },
    },
    token::{Token, TokenHash},
};
use stepclimb_core::{
    OrgScope,
    id::{Uuid, new_id},
    oauth_codes,
    oauth_refresh_tokens::{self as refresh, NewToken, Reuse, Reused, Rotation},
    users::Status,
};
use utoipa::ToSchema;

use crate::{
    AppState,
    error::ApiError,
    oauth_authorize::issuer,
    rate_limit::{RateLimited, Token as TokenRoute},
    request_org::RequestOrg,
};

/// The token endpoint's path.
pub const TOKEN_PATH: &str = "/oauth/token";

/// The revocation endpoint's path.
pub const REVOKE_PATH: &str = "/oauth/revoke";

/// The one media type both endpoints read.
const FORM: &str = "application/x-www-form-urlencoded";

const NOT_A_FORM: Refusal = Refusal {
    error: ErrorCode::InvalidRequest,
    description: "The body must be application/x-www-form-urlencoded, at most 4096 bytes.",
};

/// The answer to a granted token request (RFC 6749 section 5.1).
#[derive(Debug, Serialize, ToSchema)]
pub struct TokenResponse {
    /// The access token: a bearer credential for `/api/v1`, valid `expires_in` seconds.
    pub access_token: String,
    /// Always `Bearer`.
    #[schema(example = "Bearer")]
    pub token_type: &'static str,
    /// Seconds the access token is valid: 900.
    #[schema(example = 900)]
    pub expires_in: u64,
    /// The refresh token: single use, valid 30 days, and never past 90 days from sign-in.
    pub refresh_token: String,
}

/// A refused token or revocation request (RFC 6749 section 5.2).
#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthError {
    /// `invalid_request`, `invalid_client`, `invalid_grant` or `unsupported_grant_type`.
    pub error: &'static str,
    /// A fixed sentence for the app's developer.
    pub error_description: &'static str,
}

/// The token request's form, for the document; the handler reads the body itself.
#[derive(ToSchema)]
pub struct TokenForm {
    /// `authorization_code` or `refresh_token`.
    pub grant_type: String,
    /// With `authorization_code`: the code from the redirect URI.
    pub code: Option<String>,
    /// With `authorization_code`: the redirect URI the authorization request named.
    pub redirect_uri: Option<String>,
    /// With `authorization_code`: the PKCE verifier, 43 to 128 characters.
    pub code_verifier: Option<String>,
    /// With `refresh_token`: the current refresh token.
    pub refresh_token: Option<String>,
    /// The app's client id, `stepclimb-app`.
    pub client_id: String,
}

/// The revocation request's form, for the document; the handler reads the body itself.
#[derive(ToSchema)]
pub struct RevokeForm {
    /// A refresh token or an access token.
    pub token: String,
    /// `refresh_token` or `access_token`; the server tries both whatever it says.
    pub token_type_hint: Option<String>,
    /// The app's client id, `stepclimb-app`.
    pub client_id: String,
}

/// `body` with the headers every answer of these endpoints carries.
fn answer(status: StatusCode, body: Option<String>) -> Response {
    let mut response = match body {
        Some(json) => (
            status,
            [(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )],
            json,
        )
            .into_response(),
        None => status.into_response(),
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    response
}

fn json<T: Serialize>(status: StatusCode, value: &T) -> Response {
    // The response types are strings and integers, which always serialize.
    let text = serde_json::to_string(value).expect("a token answer serializes");
    answer(status, Some(text))
}

/// The RFC 6749 section 5.2 answer to `refusal`.
fn refused(refusal: Refusal) -> Response {
    let body = OAuthError {
        error: refusal.error.as_str(),
        error_description: refusal.description,
    };
    json(StatusCode::BAD_REQUEST, &body)
}

/// The body, when the request is a form of at most [`MAX_BODY_LEN`] bytes.
async fn form_body(headers: &HeaderMap, body: Body) -> Option<axum::body::Bytes> {
    let media_type = headers
        .get(header::CONTENT_TYPE)?
        .to_str()
        .ok()?
        .split(';')
        .next()?
        .trim();
    if !media_type.eq_ignore_ascii_case(FORM) {
        return None;
    }
    to_bytes(body, MAX_BODY_LEN).await.ok()
}

/// Exchange an authorization code, or refresh, for an access token and a refresh token.
#[utoipa::path(
    post,
    path = "/oauth/token",
    operation_id = "requestAppToken",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    description = "The token endpoint of the OAuth 2.1 server for Stepclimb's own apps (public \
clients, no client secret). `grant_type=authorization_code` with `code`, `redirect_uri`, \
`client_id` and `code_verifier` exchanges a code from `/oauth/authorize` once; \
`grant_type=refresh_token` with `refresh_token` and `client_id` rotates a refresh token: the \
presented one stops working, and presenting a used code or a rotated refresh token again revokes \
every token of its family. Refusals are RFC 6749 JSON with status 400: `invalid_request`, \
`invalid_client`, `invalid_grant` (any code or refresh token that does not pass, and a suspended \
user's) or `unsupported_grant_type`. Every answer carries `Cache-Control: no-store`.",
    request_body(content = TokenForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (
            status = 200,
            description = "The access token (15 minutes) and the next refresh token",
            body = TokenResponse,
            headers(("Cache-Control" = String, description = "`no-store`"))
        ),
        (
            status = 400,
            description = "An RFC 6749 section 5.2 error",
            body = OAuthError,
            headers(("Cache-Control" = String, description = "`no-store`"))
        ),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn request_app_token(
    limited: RateLimited<TokenRoute>,
    State(state): State<AppState>,
    org: RequestOrg,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let Some(body) = form_body(&headers, body).await else {
        return Ok(refused(NOT_A_FORM));
    };
    let request = match token_request::parse_token(&body) {
        Ok(request) => request,
        Err(refusal) => return Ok(refused(refusal)),
    };
    let grant = Grant {
        state: &state,
        scope: org.scope(),
        client_address: limited.client,
        now: state.clock.now(),
    };
    let issued = match request {
        TokenRequest::AuthorizationCode {
            code,
            redirect_uri,
            client,
            code_verifier,
        } => {
            let exchange = Exchange {
                code: &code,
                redirect_uri: &redirect_uri,
                client,
                code_verifier: &code_verifier,
            };
            grant.exchange(&exchange).await?
        }
        TokenRequest::RefreshToken {
            refresh_token,
            client,
        } => grant.refresh(&refresh_token, client).await?,
    };
    Ok(match issued {
        Ok(issued) => grant.answer(&issued)?,
        Err(refusal) => refused(refusal),
    })
}

/// One token request's context.
struct Grant<'s> {
    state: &'s AppState,
    scope: OrgScope,
    client_address: IpAddr,
    now: SystemTime,
}

/// A code exchange's checked parameters.
struct Exchange<'r> {
    code: &'r str,
    redirect_uri: &'r str,
    client: &'static Client,
    code_verifier: &'r str,
}

/// What a granted request hands the app.
struct Issued {
    user_id: Uuid,
    client: &'static Client,
    family_id: Uuid,
    refresh_token: Token,
}

fn internal(error: &dyn std::error::Error) -> ApiError {
    ApiError::internal(error)
}

/// The code's row could not be marked used under its own lock: a broken invariant.
#[derive(Debug, thiserror::Error)]
#[error("a locked, unused authorization code could not be marked used")]
struct CodeNotMarked;

impl Grant<'_> {
    /// B32: exchange a code once (the module docs list the checks).
    async fn exchange(&self, request: &Exchange<'_>) -> Result<Result<Issued, Refusal>, ApiError> {
        let hash = TokenHash::of(request.code);
        let mut tx = self.state.db.begin().await.map_err(|e| internal(&e))?;
        let Some(code) = oauth_codes::lock(&mut tx, self.scope, hash.as_bytes())
            .await
            .map_err(|e| internal(&e))?
        else {
            return Ok(Err(INVALID_GRANT));
        };
        if code.used_at.is_some() {
            if let Some(family_id) = code.family_id {
                let reuse = Reuse {
                    family_id,
                    user_id: code.user_id,
                    client_id: &code.client_id,
                    presented: Reused::AuthorizationCode,
                    client_address: self.client_address,
                };
                self.revoke_reused(&mut tx, &reuse).await?;
            }
            tx.commit().await.map_err(|e| internal(&e))?;
            return Ok(Err(INVALID_GRANT));
        }
        let passes = self.now < code.expires_at
            && code.client_id == request.client.id
            && code.redirect_uri == request.redirect_uri
            && token_request::verifier_matches(request.code_verifier, &code.code_challenge)
            && code.user_status == Status::Active;
        if !passes {
            return Ok(Err(INVALID_GRANT));
        }
        let refresh_token = Token::generate().map_err(|e| internal(&e))?;
        let refresh_hash = refresh_token.hash();
        let family_id = new_id();
        let first = NewToken {
            id: family_id,
            user_id: code.user_id,
            client_id: request.client.id,
            token_hash: refresh_hash.as_bytes(),
        };
        refresh::start_family(&mut tx, self.scope, &first, self.now)
            .await
            .map_err(|e| internal(&e))?;
        let marked = oauth_codes::mark_used(&mut tx, self.scope, code.id, family_id, self.now)
            .await
            .map_err(|e| internal(&e))?;
        if !marked {
            return Err(internal(&CodeNotMarked));
        }
        tx.commit().await.map_err(|e| internal(&e))?;
        Ok(Ok(Issued {
            user_id: code.user_id,
            client: request.client,
            family_id,
            refresh_token,
        }))
    }

    /// B33, B34: rotate a refresh token (the module docs list the checks).
    async fn refresh(
        &self,
        presented: &str,
        client: &'static Client,
    ) -> Result<Result<Issued, Refusal>, ApiError> {
        let hash = TokenHash::of(presented);
        let mut tx = self.state.db.begin().await.map_err(|e| internal(&e))?;
        let Some(token) = refresh::lock(&mut tx, self.scope, hash.as_bytes())
            .await
            .map_err(|e| internal(&e))?
        else {
            return Ok(Err(INVALID_GRANT));
        };
        if token.client_id != client.id {
            return Ok(Err(INVALID_GRANT));
        }
        let reuse = Reuse {
            family_id: token.family_id,
            user_id: token.user_id,
            client_id: client.id,
            presented: Reused::RefreshToken,
            client_address: self.client_address,
        };
        let rotated = refresh::is_rotated(&mut tx, self.scope, token.id)
            .await
            .map_err(|e| internal(&e))?;
        if rotated {
            self.revoke_reused(&mut tx, &reuse).await?;
            tx.commit().await.map_err(|e| internal(&e))?;
            return Ok(Err(INVALID_GRANT));
        }
        if self.now >= token.expires_at || token.user_status != Status::Active {
            return Ok(Err(INVALID_GRANT));
        }
        let refresh_token = Token::generate().map_err(|e| internal(&e))?;
        let refresh_hash = refresh_token.hash();
        let next = NewToken {
            id: new_id(),
            user_id: token.user_id,
            client_id: client.id,
            token_hash: refresh_hash.as_bytes(),
        };
        let rotation = refresh::rotate(&mut tx, self.scope, &token, &next, self.now)
            .await
            .map_err(|e| internal(&e))?;
        match rotation {
            Rotation::Rotated(_) => {}
            Rotation::FamilyExpired => return Ok(Err(INVALID_GRANT)),
            Rotation::AlreadyRotated => {
                self.revoke_reused(&mut tx, &reuse).await?;
                tx.commit().await.map_err(|e| internal(&e))?;
                return Ok(Err(INVALID_GRANT));
            }
        }
        tx.commit().await.map_err(|e| internal(&e))?;
        Ok(Ok(Issued {
            user_id: token.user_id,
            client,
            family_id: token.family_id,
            refresh_token,
        }))
    }

    async fn revoke_reused(
        &self,
        tx: &mut sqlx::PgConnection,
        reuse: &Reuse<'_>,
    ) -> Result<(), ApiError> {
        refresh::revoke_reused(tx, self.scope, reuse, self.now)
            .await
            .map_err(|e| internal(&e))?;
        Ok(())
    }

    /// The 200 answer with a new access token for `issued` and its refresh token.
    fn answer(&self, issued: &Issued) -> Result<Response, ApiError> {
        let issuer = issuer(self.state);
        let subject = Subject {
            issuer: &issuer,
            user_id: issued.user_id,
            org_id: self.scope.org_id(),
            client_id: issued.client.id,
            family_id: issued.family_id,
        };
        let access =
            access_token::issue(&self.state.keys, &subject, self.now).map_err(|e| internal(&e))?;
        let body = TokenResponse {
            access_token: access.expose_secret().to_owned(),
            token_type: "Bearer",
            expires_in: access_token::LIFETIME.as_secs(),
            refresh_token: issued.refresh_token.expose_secret().to_owned(),
        };
        Ok(json(StatusCode::OK, &body))
    }
}

/// Revoke an app's refresh token, or the family of its access token.
#[utoipa::path(
    post,
    path = "/oauth/revoke",
    operation_id = "revokeAppToken",
    tag = "auth",
    extensions(("x-stepclimb-mutates" = json!(true))),
    description = "RFC 7009 token revocation for Stepclimb's own apps: `token` (a refresh token or \
an access token), `token_type_hint` (optional, either kind) and `client_id`. Revoking either kind \
deletes every refresh token of its family, so the family's access tokens stop working too. \
Answers 200 whether or not the token was known. Refusals are RFC 6749 JSON with status 400: \
`invalid_request` or `invalid_client`.",
    request_body(content = RevokeForm, content_type = "application/x-www-form-urlencoded"),
    responses(
        (
            status = 200,
            description = "The token's family is revoked, or the token was not one of this client's",
            headers(("Cache-Control" = String, description = "`no-store`"))
        ),
        (
            status = 400,
            description = "An RFC 6749 section 5.2 error",
            body = OAuthError,
            headers(("Cache-Control" = String, description = "`no-store`"))
        ),
        (status = 404, response = crate::openapi::OrganizationNotFound),
        (status = 429, response = crate::openapi::RateLimited),
        (status = 500, response = crate::openapi::InternalError),
        (status = 503, response = crate::openapi::NoOrganization),
    )
)]
pub async fn revoke_app_token(
    _limited: RateLimited<TokenRoute>,
    State(state): State<AppState>,
    org: RequestOrg,
    headers: HeaderMap,
    body: Body,
) -> Result<Response, ApiError> {
    let Some(body) = form_body(&headers, body).await else {
        return Ok(refused(NOT_A_FORM));
    };
    let request = match token_request::parse_revoke(&body) {
        Ok(request) => request,
        Err(refusal) => return Ok(refused(refusal)),
    };
    revoke(&state, org.scope(), &request).await?;
    Ok(answer(StatusCode::OK, None))
}

/// Delete the family `request.token` belongs to, when it is a refresh token or a valid access
/// token of `request.client` in the scope's organization.
async fn revoke(
    state: &AppState,
    scope: OrgScope,
    request: &RevokeRequest,
) -> Result<(), ApiError> {
    let hash = TokenHash::of(&request.token);
    let client_id = request.client.id;
    let revoked = refresh::revoke_by_token_hash(&state.db, scope, hash.as_bytes(), client_id)
        .await
        .map_err(|e| internal(&e))?;
    if revoked {
        return Ok(());
    }
    let verified = access_token::verify(
        &state.keys,
        &request.token,
        &issuer(state),
        state.clock.now(),
    );
    if let Ok(claims) = verified
        && claims.org == scope.org_id()
        && claims.client_id == client_id
    {
        refresh::revoke_family(&state.db, scope, claims.fid)
            .await
            .map_err(|e| internal(&e))?;
    }
    Ok(())
}
