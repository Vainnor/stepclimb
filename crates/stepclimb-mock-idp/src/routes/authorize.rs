// SPDX-License-Identifier: AGPL-3.0-or-later
//! The authorize endpoints (RFC 6749 4.1.1, OIDC Core 3.1.2): no login page; they approve at once
//! as the selected user and redirect back with a code, or with the error the test set.

use axum::http::{StatusCode, header};
use axum::response::{IntoResponse as _, Response};
use url::Url;

use super::random_token;
use crate::jwt::verifier_well_formed;
use crate::state::{Flavor, Grant, Shared, State};
use crate::types::RecordedRequest;

pub(super) fn answer(shared: &Shared, request: &RecordedRequest, flavor: Flavor) -> Response {
    let mut state = shared.lock();
    let back = match client_and_redirect(&state, request) {
        Ok(back) => back,
        Err(refusal) => return (StatusCode::BAD_REQUEST, refusal).into_response(),
    };
    let code_challenge = match check_request(request, flavor, state.require_pkce) {
        Ok(challenge) => challenge,
        Err((error, description)) => return back.error(error, description),
    };
    if let Some(error) = &state.authorize_error {
        return back.redirect(&[
            Some(("error", error.error.clone())),
            error
                .description
                .clone()
                .map(|description| ("error_description", description)),
        ]);
    }
    let Some(code) = random_token() else {
        return back.error("server_error", "no randomness");
    };
    let grant = Grant {
        flavor,
        client_id: state.client_id.clone(),
        redirect_uri: back.raw_redirect_uri.clone(),
        code_challenge,
        nonce: request.query_param("nonce").map(str::to_owned),
        user: state.user.clone(),
    };
    state.grants.insert(code.clone(), grant);
    back.redirect(&[Some(("code", code))])
}

/// Where to send the browser back to, or why there is nowhere safe to send an error, in which
/// case the mock answers 400 itself (RFC 6749 4.1.2.1): an unknown client, or a redirect URI
/// that is missing, not an absolute `http` or `https` URL without a fragment, or not in the
/// allowlist.
fn client_and_redirect(state: &State, request: &RecordedRequest) -> Result<Back, &'static str> {
    if request.query_param("client_id") != Some(state.client_id.as_str()) {
        return Err("unknown client_id");
    }
    let raw = request
        .query_param("redirect_uri")
        .ok_or("missing redirect_uri")?;
    let parsed = Url::parse(raw)
        .ok()
        .filter(|uri| matches!(uri.scheme(), "http" | "https") && uri.fragment().is_none())
        .ok_or("invalid redirect_uri")?;
    if let Some(allowed) = &state.redirect_uris
        && !allowed.iter().any(|uri| uri == raw)
    {
        return Err("redirect_uri is not registered");
    }
    Ok(Back {
        redirect_uri: parsed,
        raw_redirect_uri: raw.to_owned(),
        state: request.query_param("state").map(str::to_owned),
    })
}

/// The PKCE challenge to bind to the code, or the error to redirect with.
fn check_request(
    request: &RecordedRequest,
    flavor: Flavor,
    require_pkce: bool,
) -> Result<Option<String>, (&'static str, &'static str)> {
    if request.query_param("response_type") != Some("code") {
        return Err(("unsupported_response_type", "response_type must be code"));
    }
    let scopes = request.query_param("scope").unwrap_or_default();
    if flavor == Flavor::Oidc && !scopes.split(' ').any(|s| s == "openid") {
        return Err(("invalid_scope", "scope must include openid"));
    }
    pkce(request, require_pkce).map_err(|description| ("invalid_request", description))
}

/// The S256 challenge to bind to the code, or why the request is refused. `plain` and any other
/// method are refused, and so is a missing challenge while PKCE is required.
fn pkce(request: &RecordedRequest, required: bool) -> Result<Option<String>, &'static str> {
    match (
        request.query_param("code_challenge"),
        request.query_param("code_challenge_method"),
    ) {
        (Some(challenge), Some("S256")) if verifier_well_formed(challenge) => {
            Ok(Some(challenge.to_owned()))
        }
        (Some(_), Some("S256")) => Err("code_challenge is not a S256 challenge"),
        (Some(_), _) => Err("code_challenge_method must be S256"),
        (None, Some(_)) => Err("code_challenge_method without code_challenge"),
        (None, None) if required => Err("PKCE is required"),
        (None, None) => Ok(None),
    }
}

/// Where the authorize endpoint sends the browser back to.
struct Back {
    redirect_uri: Url,
    /// The `redirect_uri` exactly as the request sent it, which the token request must repeat.
    raw_redirect_uri: String,
    state: Option<String>,
}

impl Back {
    fn error(&self, error: &str, description: &str) -> Response {
        self.redirect(&[
            Some(("error", error.to_owned())),
            Some(("error_description", description.to_owned())),
        ])
    }

    /// 302 to the redirect URI with `pairs` and the request's `state` added to its query.
    fn redirect(&self, pairs: &[Option<(&str, String)>]) -> Response {
        let mut location = self.redirect_uri.clone();
        {
            let mut query = location.query_pairs_mut();
            for (name, value) in pairs.iter().flatten() {
                query.append_pair(name, value);
            }
            if let Some(state) = &self.state {
                query.append_pair("state", state);
            }
        }
        (
            StatusCode::FOUND,
            [(header::LOCATION, location.to_string())],
        )
            .into_response()
    }
}
