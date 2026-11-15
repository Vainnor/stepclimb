// SPDX-License-Identifier: AGPL-3.0-or-later

use serde_json::{Value, json};
use url::Url;

use super::*;
use crate::oauth2_config::{self, Oauth2Settings};
use crate::outbound::{Refused, UrlPolicy};

const REDIRECT: &str = "https://lms.example.org/auth/plain/callback";

fn settings(config: Value) -> Oauth2Settings {
    oauth2_config::validate(&config, UrlPolicy::new(true, true), None)
        .expect("a valid config")
        .settings
}

fn config(authorize: &str) -> Value {
    json!({
        "authorize": authorize,
        "token": "https://idp.example.test/oauth/token",
        "userinfo": "https://idp.example.test/api/user",
        "client_id": "stepclimb",
        "client_secret": "secret",
        "scopes": ["full_name", "email"],
        "claims": { "subject": "/data/cid" },
    })
}

fn pairs(url: &Url) -> Vec<(String, String)> {
    url.query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn param(url: &Url, name: &str) -> Option<String> {
    pairs(url)
        .into_iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
}

/// Spec 06 T3: `response_type=code`, the space-joined scopes, the redirect URI, the state and,
/// with PKCE on, the S256 challenge of the stored verifier.
#[test]
fn spec_t3_the_authorize_url_carries_every_parameter() {
    let settings = settings(config("https://idp.example.test/oauth/authorize"));
    let secrets = FlowSecrets::generate(true).unwrap();
    let url = authorization_url(&settings, REDIRECT, &secrets, UrlPolicy::new(false, false))
        .expect("a URL");
    assert_eq!(
        url.as_str().split('?').next(),
        Some("https://idp.example.test/oauth/authorize")
    );
    let challenge = secrets.code_challenge().expect("PKCE on");
    assert_eq!(
        pairs(&url),
        [
            ("response_type", "code"),
            ("client_id", "stepclimb"),
            ("redirect_uri", REDIRECT),
            ("scope", "full_name email"),
            ("state", secrets.state()),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
    );
    assert!(!url.as_str().contains(secrets.code_verifier().unwrap()));
}

/// Spec 06 Q3: with `pkce` off, no verifier is kept and no challenge is sent.
#[test]
fn spec_t3_without_pkce_no_challenge_is_sent() {
    let mut off = config("https://idp.example.test/oauth/authorize");
    off["pkce"] = json!(false);
    let settings = settings(off);
    let secrets = FlowSecrets::generate(settings.pkce).unwrap();
    assert_eq!(secrets.code_verifier(), None);
    assert_eq!(secrets.code_challenge(), None);
    let url = authorization_url(&settings, REDIRECT, &secrets, UrlPolicy::new(false, false))
        .expect("a URL");
    assert_eq!(param(&url, "code_challenge"), None);
    assert_eq!(param(&url, "code_challenge_method"), None);
    assert_eq!(param(&url, "state").as_deref(), Some(secrets.state()));
}

#[test]
fn no_scope_parameter_is_sent_without_scopes() {
    let mut none = config("https://idp.example.test/oauth/authorize");
    none["scopes"] = json!([]);
    let url = authorization_url(
        &settings(none),
        REDIRECT,
        &FlowSecrets::generate(true).unwrap(),
        UrlPolicy::new(false, false),
    )
    .unwrap();
    assert_eq!(param(&url, "scope"), None);
}

#[test]
fn the_authorize_urls_own_query_is_kept_as_written() {
    let settings = settings(config(
        "https://idp.example.test/oauth/authorize?tenant=a%20b&prompt=login",
    ));
    let url = authorization_url(
        &settings,
        REDIRECT,
        &FlowSecrets::generate(true).unwrap(),
        UrlPolicy::new(false, false),
    )
    .unwrap();
    assert!(
        url.as_str().starts_with(
            "https://idp.example.test/oauth/authorize?tenant=a%20b&prompt=login&response_type=code&"
        ),
        "{url}"
    );
}

/// A stored `authorize` query that names a reserved parameter (validation refuses one on save;
/// a stored row is not checked again) has that parameter replaced, never sent twice.
#[test]
fn a_reserved_parameter_in_the_stored_url_is_replaced() {
    let mut settings = settings(config("https://idp.example.test/oauth/authorize"));
    settings.authorize = "https://idp.example.test/oauth/authorize?keep=1&%73tate=evil&scope=admin&client_id=other&redirect_uri=https%3A%2F%2Fevil.example&response_type=token&code_challenge=x&code_challenge_method=plain&nonce=n".to_owned();
    let secrets = FlowSecrets::generate(true).unwrap();
    let url =
        authorization_url(&settings, REDIRECT, &secrets, UrlPolicy::new(false, false)).unwrap();
    let names: Vec<String> = pairs(&url).into_iter().map(|(k, _)| k).collect();
    assert_eq!(
        names,
        [
            "keep",
            "response_type",
            "client_id",
            "redirect_uri",
            "scope",
            "state",
            "code_challenge",
            "code_challenge_method"
        ]
    );
    assert_eq!(param(&url, "state").as_deref(), Some(secrets.state()));
    assert_eq!(param(&url, "client_id").as_deref(), Some("stepclimb"));
    assert_eq!(param(&url, "redirect_uri").as_deref(), Some(REDIRECT));
    assert!(!url.as_str().contains("evil"));
}

/// The stored `authorize` URL is checked against the policy the instance is under now.
#[test]
fn an_authorize_url_the_policy_now_refuses_is_not_used() {
    let secrets = FlowSecrets::generate(true).unwrap();
    let base = settings(config("https://idp.example.test/oauth/authorize"));
    let at = |authorize: &str| Oauth2Settings {
        authorize: authorize.to_owned(),
        ..base.clone()
    };
    let refused = |settings: &Oauth2Settings, policy| match authorization_url(
        settings, REDIRECT, &secrets, policy,
    ) {
        Err(AuthorizeError::Refused(refused)) => Some(refused),
        Ok(_) => None,
        Err(other) => panic!("{other}"),
    };
    let http = at("http://idp.example.test/oauth/authorize");
    assert_eq!(
        refused(&http, UrlPolicy::new(false, true)),
        Some(Refused::Scheme)
    );
    assert_eq!(refused(&http, UrlPolicy::new(true, false)), None);
    let loopback = at("https://127.0.0.1:9/oauth/authorize");
    assert_eq!(
        refused(&loopback, UrlPolicy::new(false, true)),
        Some(Refused::Loopback)
    );
    assert_eq!(refused(&loopback, UrlPolicy::new(true, false)), None);
    let private = at("https://10.0.0.5/oauth/authorize");
    assert_eq!(
        refused(&private, UrlPolicy::new(false, false)),
        Some(Refused::Private)
    );
    assert_eq!(refused(&private, UrlPolicy::new(false, true)), None);
    let javascript = at("javascript:alert(1)");
    assert_eq!(
        refused(&javascript, UrlPolicy::new(true, true)),
        Some(Refused::Scheme)
    );
}

#[test]
fn flow_secrets_are_random_round_trip_and_hide_in_debug() {
    let one = FlowSecrets::generate(true).unwrap();
    let two = FlowSecrets::generate(true).unwrap();
    assert_ne!(one.state(), two.state());
    assert_ne!(one.code_verifier(), two.code_verifier());
    assert_eq!(one.state().len(), 43);
    assert_eq!(one.code_verifier().map(str::len), Some(43));
    assert_eq!(FlowSecrets::from_payload(&one.to_payload()).unwrap(), one);
    let shown = format!("{one:?}");
    assert!(!shown.contains(one.state()));
    // An OIDC flow's payload is not an OAuth 2 flow's.
    let oidc = crate::oidc::FlowSecrets::generate().unwrap().to_payload();
    assert!(FlowSecrets::from_payload(&oidc).is_err());
}

#[test]
fn the_state_matches_only_itself() {
    let secrets = FlowSecrets::generate(false).unwrap();
    let state = secrets.state().to_owned();
    assert!(secrets.state_matches(&state));
    let mut last = state.clone();
    let changed = if last.pop() == Some('A') { 'B' } else { 'A' };
    last.push(changed);
    for other in [last.as_str(), &state[1..], "", &format!("{state}x")] {
        assert!(!secrets.state_matches(other), "{other}");
    }
}

#[test]
fn debug_output_of_an_exchange_hides_the_secrets() {
    let ring = crate::encryption::KeyRing::parse("k1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=")
        .expect("a ring");
    let row = oauth2_config::ProviderRow {
        org_id: uuid::Uuid::from_u128(1),
        provider_id: uuid::Uuid::from_u128(2),
    };
    let mut with_secret = config("https://idp.example.test/oauth/authorize");
    with_secret["client_secret"] = json!("client-secret-zq");
    let secret = oauth2_config::validate(&with_secret, UrlPolicy::new(false, true), None)
        .unwrap()
        .seal(&ring, row)
        .unwrap()
        .client_secret(&ring, row)
        .unwrap();
    let exchange = CodeExchange {
        token_url: "https://idp.example.test/token",
        client_id: "stepclimb",
        client_secret: &secret,
        redirect_uri: REDIRECT,
        code: "code-zq",
        code_verifier: Some("verifier-zq"),
    };
    assert!(!format!("{exchange:?}").contains("zq"));
}
