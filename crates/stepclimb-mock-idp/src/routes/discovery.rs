// SPDX-License-Identifier: AGPL-3.0-or-later
//! The discovery document (OpenID Connect Discovery 1.0, 3) and the JWKS (RFC 7517 5).

use axum::http::StatusCode;
use axum::response::Response;
use serde_json::{Map, Value, json};

use super::json_response;
use crate::state::{Shared, State, apply};
use crate::types::{Endpoint, RecordedRequest};

pub(super) fn discovery(shared: &Shared, _: &RecordedRequest) -> Response {
    let state = shared.lock();
    let mut document = document(&state);
    apply(&mut document, &state.discovery_overrides);
    json_response(StatusCode::OK, &Value::Object(document))
}

fn document(state: &State) -> Map<String, Value> {
    let methods: Vec<&str> = state.auth_methods.iter().map(|m| m.name()).collect();
    [
        ("issuer", json!(state.issuer())),
        (
            "authorization_endpoint",
            json!(state.url(Endpoint::Authorize)),
        ),
        ("token_endpoint", json!(state.url(Endpoint::Token))),
        ("userinfo_endpoint", json!(state.url(Endpoint::Userinfo))),
        ("jwks_uri", json!(state.url(Endpoint::Jwks))),
        ("scopes_supported", json!(["openid", "email", "profile"])),
        ("response_types_supported", json!(["code"])),
        ("grant_types_supported", json!(["authorization_code"])),
        ("subject_types_supported", json!(["public"])),
        ("id_token_signing_alg_values_supported", json!(["RS256"])),
        ("token_endpoint_auth_methods_supported", json!(methods)),
        ("code_challenge_methods_supported", json!(["S256"])),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

pub(super) fn jwks(shared: &Shared, _: &RecordedRequest) -> Response {
    let state = shared.lock();
    let keys: Vec<Value> = state.keys.published.iter().map(|k| k.jwk()).collect();
    json_response(StatusCode::OK, &json!({ "keys": keys }))
}
