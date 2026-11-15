// SPDX-License-Identifier: AGPL-3.0-or-later
//! The userinfo endpoints (OIDC Core 5.3, RFC 6750 2.1): the bearer token must be one the same
//! provider issued.

use axum::http::{StatusCode, header};
use axum::response::Response;
use serde_json::{Map, Value, json};

use super::json_response;
use crate::state::{Flavor, Shared, apply};
use crate::types::{MockUser, RecordedRequest};

pub(super) fn answer(shared: &Shared, request: &RecordedRequest, flavor: Flavor) -> Response {
    let state = shared.lock();
    let token = request
        .header("authorization")
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
        .map(|(_, token)| token.trim());
    let Some((_, user)) = token
        .and_then(|token| state.access_tokens.get(token))
        .filter(|(issued_by, _)| *issued_by == flavor)
    else {
        return invalid_token();
    };
    let body = match flavor {
        Flavor::Oidc => {
            let mut claims = claims(user);
            apply(&mut claims, &state.userinfo_overrides);
            Value::Object(claims)
        }
        Flavor::OAuth2 => state
            .oauth2_userinfo
            .clone()
            .unwrap_or_else(|| Value::Object(claims(user))),
    };
    json_response(StatusCode::OK, &body)
}

/// `sub` and the user's claims.
fn claims(user: &MockUser) -> Map<String, Value> {
    let mut claims = user.claims.clone();
    claims.insert("sub".to_owned(), json!(user.subject));
    claims
}

fn invalid_token() -> Response {
    let mut response = json_response(
        StatusCode::UNAUTHORIZED,
        &json!({ "error": "invalid_token" }),
    );
    if let Ok(value) = "Bearer error=\"invalid_token\"".parse() {
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, value);
    }
    response
}
