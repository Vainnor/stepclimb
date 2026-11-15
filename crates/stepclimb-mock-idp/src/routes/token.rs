// SPDX-License-Identifier: AGPL-3.0-or-later
//! The token endpoints (RFC 6749 4.1.3 and 5, RFC 7636 4.6, OIDC Core 3.1.3): client
//! authentication, a single-use code, the redirect URI and the PKCE verifier, then the tokens.

use std::time::UNIX_EPOCH;

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

use super::{json_response, oauth_error, random_token};
use crate::keys::SignError;
use crate::state::{Flavor, Grant, Shared, Signing, State, apply};
use crate::types::{ClientAuth, Endpoint, IssuedTokens, RecordedRequest};
use crate::{ID_TOKEN_LIFETIME, jwt};

/// The `expires_in` of an access token, in seconds.
const ACCESS_TOKEN_LIFETIME: u64 = 3600;

pub(super) fn answer(shared: &Shared, request: &RecordedRequest, flavor: Flavor) -> Response {
    let form_typed = request
        .header("content-type")
        .and_then(|t| t.split(';').next())
        .is_some_and(|t| t.trim() == "application/x-www-form-urlencoded");
    if !form_typed {
        return invalid("invalid_request", "the body must be form-encoded");
    }
    let mut state = shared.lock();
    let client_id = match authenticate(&state, request) {
        Ok(client_id) => client_id,
        Err(response) => return *response,
    };
    // Once the client is authenticated, presenting a code spends it, whether the rest of the
    // request passes or not (RFC 6749 4.1.2: single use).
    let grant = request
        .form_param("code")
        .and_then(|code| state.grants.remove(&code));
    if request.form_param("grant_type").as_deref() != Some("authorization_code") {
        return invalid("unsupported_grant_type", "only authorization_code");
    }
    let Some(grant) = grant else {
        return invalid("invalid_grant", "unknown or used code");
    };
    if let Err(description) = check_grant(&grant, flavor, &client_id, request) {
        return invalid("invalid_grant", description);
    }
    issue(&mut state, grant, flavor)
}

fn invalid(error: &str, description: &str) -> Response {
    oauth_error(StatusCode::BAD_REQUEST, error, description)
}

/// The authenticated `client_id`, or the 400 or 401 error response.
fn authenticate(state: &State, request: &RecordedRequest) -> Result<String, Box<Response>> {
    let basic = request
        .header("authorization")
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("basic"))
        .map(|(_, credentials)| credentials.trim().to_owned());
    let post_secret = request.form_param("client_secret");
    let (method, id, secret) = match (basic, post_secret) {
        (Some(_), Some(_)) => {
            return Err(Box::new(invalid(
                "invalid_request",
                "two client auth methods",
            )));
        }
        (Some(credentials), None) => {
            let (id, secret) =
                decode_basic(&credentials).ok_or_else(|| Box::new(unauthorized(true)))?;
            if request
                .form_param("client_id")
                .is_some_and(|form_id| form_id != id)
            {
                return Err(Box::new(invalid(
                    "invalid_request",
                    "client_id differs from Basic",
                )));
            }
            (ClientAuth::Basic, id, secret)
        }
        (None, Some(secret)) => {
            let id = request.form_param("client_id").unwrap_or_default();
            (ClientAuth::Post, id, secret)
        }
        (None, None) => return Err(Box::new(unauthorized(false))),
    };
    let allowed = state.auth_methods.contains(&method);
    if !allowed || id != state.client_id || secret != state.client_secret {
        return Err(Box::new(unauthorized(method == ClientAuth::Basic)));
    }
    Ok(id)
}

/// The id and secret of `client_secret_basic` credentials: base64 of the form-encoded id, a
/// colon and the form-encoded secret (RFC 6749 2.3.1).
fn decode_basic(credentials: &str) -> Option<(String, String)> {
    let decoded = String::from_utf8(STANDARD.decode(credentials).ok()?).ok()?;
    let (id, secret) = decoded.split_once(':')?;
    Some((form_decode(id), form_decode(secret)))
}

fn form_decode(value: &str) -> String {
    // A form-encoded value never holds a raw `=` or `&`, so parsing it as one key reads it whole.
    url::form_urlencoded::parse(value.as_bytes())
        .next()
        .map(|(key, _)| key.into_owned())
        .unwrap_or_default()
}

fn unauthorized(basic: bool) -> Response {
    let mut response = oauth_error(
        StatusCode::UNAUTHORIZED,
        "invalid_client",
        "client authentication failed",
    );
    if basic && let Ok(value) = "Basic realm=\"mock-idp\"".parse() {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}

/// The code was issued by this provider to this client for this redirect URI, and the PKCE
/// verifier matches its challenge.
fn check_grant(
    grant: &Grant,
    flavor: Flavor,
    client_id: &str,
    request: &RecordedRequest,
) -> Result<(), &'static str> {
    if grant.flavor != flavor || grant.client_id != client_id {
        return Err("the code was not issued here to this client");
    }
    if request.form_param("redirect_uri").as_deref() != Some(grant.redirect_uri.as_str()) {
        return Err("redirect_uri differs from the authorize request");
    }
    let verifier = request.form_param("code_verifier");
    match (&grant.code_challenge, verifier) {
        (Some(challenge), Some(verifier)) => {
            if jwt::verifier_well_formed(&verifier) && jwt::s256(&verifier) == *challenge {
                Ok(())
            } else {
                Err("code_verifier does not match the code_challenge")
            }
        }
        (Some(_), None) => Err("code_verifier is missing"),
        (None, Some(_)) => Err("code_verifier without a code_challenge"),
        (None, None) => Ok(()),
    }
}

fn issue(state: &mut State, grant: Grant, flavor: Flavor) -> Response {
    let (Some(access_token), Some(refresh_token)) = (random_token(), random_token()) else {
        return oauth_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
            "no randomness",
        );
    };
    let mut body = Map::new();
    body.insert("access_token".to_owned(), json!(access_token));
    body.insert("token_type".to_owned(), json!("Bearer"));
    body.insert("expires_in".to_owned(), json!(ACCESS_TOKEN_LIFETIME));
    body.insert("refresh_token".to_owned(), json!(refresh_token));
    let id_token = match flavor {
        Flavor::Oidc => {
            let Ok(token) = id_token(state, &grant, &access_token) else {
                return oauth_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "server_error",
                    "signing failed",
                );
            };
            body.insert("id_token".to_owned(), json!(token));
            Some(token)
        }
        Flavor::OAuth2 => None,
    };
    apply(&mut body, &state.token_overrides);
    state
        .access_tokens
        .insert(access_token.clone(), (flavor, grant.user));
    state.issued.push(IssuedTokens {
        endpoint: match flavor {
            Flavor::Oidc => Endpoint::Token,
            Flavor::OAuth2 => Endpoint::OAuth2Token,
        },
        access_token,
        refresh_token,
        id_token,
    });
    let mut response = json_response(StatusCode::OK, &Value::Object(body));
    if let Ok(value) = "no-cache".parse() {
        response.headers_mut().insert(header::PRAGMA, value);
    }
    response.into_response()
}

/// The ID token for `grant`: the user's claims, then the registered claims, then the test's
/// overrides, signed as the test chose.
fn id_token(state: &State, grant: &Grant, access_token: &str) -> Result<String, SignError> {
    let iat = (state.now)()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut claims = grant.user.claims.clone();
    claims.insert("iss".to_owned(), json!(state.issuer()));
    claims.insert("sub".to_owned(), json!(grant.user.subject));
    claims.insert("aud".to_owned(), json!(grant.client_id));
    claims.insert("iat".to_owned(), json!(iat));
    claims.insert("exp".to_owned(), json!(iat + ID_TOKEN_LIFETIME.as_secs()));
    if let Some(nonce) = &grant.nonce {
        claims.insert("nonce".to_owned(), json!(nonce));
    }
    claims.insert("at_hash".to_owned(), json!(jwt::at_hash(access_token)));
    apply(&mut claims, &state.id_token_overrides);
    let signer = match &state.signing {
        Signing::Current => Some(&state.keys.current),
        Signing::AlgNone => None,
        Signing::Unpublished(key) => Some(key),
    };
    jwt::encode(&claims, signer.map(AsRef::as_ref))
}
