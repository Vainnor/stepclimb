// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests of the pieces that need no provider. Discovery against the mock IdP is in
//! `tests/oidc_discovery.rs`.

use std::collections::{BTreeMap, HashSet};

use serde_json::json;

use super::*;
use crate::json_pointer::JsonPointer;
use crate::oidc_config::{ClaimsMapping, OidcSettings};

fn secrets(state: &str, nonce: &str, verifier: &str) -> FlowSecrets {
    let payload = json!({ "state": state, "nonce": nonce, "code_verifier": verifier });
    FlowSecrets::from_payload(payload.to_string().as_bytes()).expect("a payload")
}

fn pointer(text: &str) -> JsonPointer {
    JsonPointer::parse(text).expect("a pointer")
}

fn settings(scopes: &[&str]) -> OidcSettings {
    OidcSettings {
        issuer: "https://idp.example.test/realms/acme".into(),
        client_id: "stepclimb app".into(),
        scopes: scopes.iter().map(|scope| (*scope).to_owned()).collect(),
        claims: ClaimsMapping {
            given_name: pointer("/given_name"),
            family_name: pointer("/family_name"),
            email: pointer("/email"),
            email_verified: pointer("/email_verified"),
            groups: None,
            attributes: BTreeMap::new(),
        },
        userinfo: true,
    }
}

fn metadata(authorization_endpoint: &str) -> ProviderMetadata {
    serde_json::from_value(json!({
        "issuer": "https://idp.example.test/realms/acme",
        "authorization_endpoint": authorization_endpoint,
        "token_endpoint": "https://idp.example.test/token",
        "jwks_uri": "https://idp.example.test/jwks",
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"]
    }))
    .expect("a discovery document")
}

fn query(url: &url::Url) -> Vec<(String, String)> {
    url.query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect()
}

#[test]
fn the_discovery_path_is_joined_with_one_slash_keeping_the_issuers_path() {
    for (issuer, expected) in [
        (
            "https://idp.example.test/realms/acme",
            "https://idp.example.test/realms/acme/.well-known/openid-configuration",
        ),
        (
            "https://idp.example.test/realms/acme/",
            "https://idp.example.test/realms/acme/.well-known/openid-configuration",
        ),
        (
            "https://idp.example.test",
            "https://idp.example.test/.well-known/openid-configuration",
        ),
    ] {
        assert_eq!(
            discovery_url(issuer).unwrap().as_str(),
            expected,
            "{issuer}"
        );
    }
    assert!(matches!(
        discovery_url("not a url"),
        Err(DiscoveryError::IssuerUrl)
    ));
}

#[test]
fn generated_secrets_are_distinct_43_character_tokens() {
    let mut seen = HashSet::new();
    for _ in 0..100 {
        let secrets = FlowSecrets::generate().unwrap();
        for value in [secrets.state(), secrets.nonce(), secrets.code_verifier()] {
            assert_eq!(value.len(), 43);
            assert!(
                value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
                "{value}"
            );
            assert!(seen.insert(value.to_owned()), "a repeated value");
        }
    }
}

#[test]
fn the_challenge_is_s256_of_the_verifier() {
    // RFC 7636 Appendix B.
    let secrets = secrets("s", "n", "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    assert_eq!(
        secrets.code_challenge(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn the_payload_round_trips_and_refuses_anything_else() {
    let secrets = FlowSecrets::generate().unwrap();
    let back = FlowSecrets::from_payload(&secrets.to_payload()).unwrap();
    assert_eq!(back, secrets);
    for other in [
        &b"not json"[..],
        br#"{"state":"a","nonce":"b"}"#,
        br#"{"state":"a","nonce":"b","code_verifier":"c","extra":1}"#,
    ] {
        assert!(FlowSecrets::from_payload(other).is_err());
    }
}

#[test]
fn debug_output_leaves_every_secret_out() {
    let secrets = FlowSecrets::generate().unwrap();
    let printed = format!("{secrets:?}");
    for value in [secrets.state(), secrets.nonce(), secrets.code_verifier()] {
        assert!(!printed.contains(value), "{printed}");
    }
}

#[test]
fn the_authorization_url_carries_every_parameter_of_b5() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let secrets = secrets("state-value", "nonce-value", verifier);
    let url = authorization_url(
        &metadata("https://idp.example.test/authorize?tenant=acme"),
        &settings(&["openid", "email", "profile"]),
        "https://lms.example.org/auth/acme/callback",
        &secrets,
    )
    .unwrap();

    assert_eq!(
        url.as_str().split('?').next(),
        Some("https://idp.example.test/authorize")
    );
    let mut pairs = query(&url);
    pairs.sort();
    let mut expected: Vec<(String, String)> = [
        ("client_id", "stepclimb app"),
        (
            "code_challenge",
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM",
        ),
        ("code_challenge_method", "S256"),
        ("nonce", "nonce-value"),
        ("redirect_uri", "https://lms.example.org/auth/acme/callback"),
        ("response_type", "code"),
        ("scope", "openid email profile"),
        ("state", "state-value"),
        ("tenant", "acme"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect();
    expected.sort();
    assert_eq!(pairs, expected);
    assert!(!url.as_str().contains(verifier), "the verifier stays here");
}

#[test]
fn openid_is_requested_once_even_when_it_is_the_only_scope() {
    let url = authorization_url(
        &metadata("https://idp.example.test/authorize"),
        &settings(&["openid"]),
        "https://lms.example.org/auth/acme/callback",
        &FlowSecrets::generate().unwrap(),
    )
    .unwrap();
    let scope: Vec<_> = query(&url)
        .into_iter()
        .filter(|(name, _)| name == "scope")
        .collect();
    assert_eq!(scope, [("scope".to_owned(), "openid".to_owned())]);
}

#[test]
fn a_redirect_uri_that_is_not_a_url_is_refused() {
    let result = authorization_url(
        &metadata("https://idp.example.test/authorize"),
        &settings(&["openid"]),
        "/auth/acme/callback",
        &FlowSecrets::generate().unwrap(),
    );
    assert!(matches!(result, Err(AuthorizeError::RedirectUri)));
}
