// SPDX-License-Identifier: AGPL-3.0-or-later
//! Each fault switch changes exactly what it names, endpoint faults replace or delay answers, and
//! every request is recorded.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};
use stepclimb_auth::outbound::{OutboundClient, OutboundError, UrlPolicy};
use stepclimb_mock_idp::{AuthorizeError, Endpoint, Fault, ID_TOKEN_LIFETIME, MockIdp, StatusCode};

use crate::common::{
    self, NONCE, STATE, VERIFIER, authorize, authorize_params, decode_jwt, get, json, param,
};

/// A fixed time for `iat`, so two sign-ins issue the same times.
const AT: u64 = 2_000_000_000;

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

/// The names whose values differ between `a` and `b`, including names only one has.
fn changed(a: &Map<String, Value>, b: &Map<String, Value>) -> Vec<String> {
    let mut names: Vec<String> = a
        .keys()
        .chain(b.keys())
        .filter(|name| a.get(*name) != b.get(*name))
        .cloned()
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The ID token's claims without `at_hash`, which differs per access token.
async fn id_claims(mock: &MockIdp) -> Map<String, Value> {
    let tokens = common::oidc_tokens(mock).await;
    let (_, claims) = decode_jwt(tokens["id_token"].as_str().expect("id_token"));
    let mut claims = object(claims);
    claims.remove("at_hash");
    claims
}

#[tokio::test]
async fn the_id_token_carries_the_registered_claims_and_the_users() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_time_source(|| UNIX_EPOCH + Duration::from_secs(AT));
    let tokens = common::oidc_tokens(&mock).await;
    let (header, claims) = decode_jwt(tokens["id_token"].as_str().expect("id_token"));
    assert_eq!(
        header,
        json!({ "alg": "RS256", "typ": "JWT", "kid": mock.signing_kid() })
    );
    let mut claims = object(claims);
    let at_hash = claims.remove("at_hash").expect("at_hash");
    assert_eq!(
        at_hash.as_str().map(str::len),
        Some(22),
        "16 bytes, base64url"
    );
    assert_eq!(
        Value::Object(claims),
        json!({
            "iss": mock.issuer(),
            "sub": "mock-user-1",
            "aud": mock.client_id(),
            "iat": AT,
            "exp": AT + ID_TOKEN_LIFETIME.as_secs(),
            "nonce": NONCE,
            "given_name": "Test",
            "family_name": "Student",
            "email": "test.student@example.test",
            "email_verified": true,
        })
    );
}

#[tokio::test]
async fn each_id_token_claim_switch_changes_only_its_claim() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_time_source(|| UNIX_EPOCH + Duration::from_secs(AT));
    let baseline = id_claims(&mock).await;
    let cases = [
        ("iss", json!("http://127.0.0.1:1/other")),
        ("aud", json!([mock.client_id(), "another-client"])),
        ("azp", json!("another-client")),
        ("exp", json!(AT - 120)),
        ("iat", json!(AT + 120)),
        ("nonce", json!("another-nonce")),
        ("sub", json!("someone-else")),
    ];
    for (name, value) in cases {
        mock.set_id_token_claim(name, Some(value.clone()));
        let faulted = id_claims(&mock).await;
        assert_eq!(changed(&baseline, &faulted), [name], "{name}");
        assert_eq!(faulted[name], value, "{name}");
        mock.clear_overrides();
    }

    mock.set_id_token_claim("nonce", None);
    assert_eq!(changed(&baseline, &id_claims(&mock).await), ["nonce"]);
    mock.clear_overrides();

    mock.set_id_token_claim("at_hash", Some(json!("AAAAAAAAAAAAAAAAAAAAAA")));
    let tokens = common::oidc_tokens(&mock).await;
    let (_, claims) = decode_jwt(tokens["id_token"].as_str().expect("id_token"));
    assert_eq!(claims["at_hash"], "AAAAAAAAAAAAAAAAAAAAAA");
    let mut claims = object(claims);
    claims.remove("at_hash");
    assert_eq!(claims, baseline);
}

#[tokio::test]
async fn the_time_source_sets_iat_and_exp() {
    let mock = MockIdp::start().await.expect("mock");
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_secs();
    let claims = id_claims(&mock).await;
    let iat = claims["iat"].as_u64().expect("iat");
    assert!(
        iat >= before && iat <= before + 5,
        "the system clock by default"
    );

    mock.set_time_source(|| UNIX_EPOCH + Duration::from_secs(1_000));
    let claims = id_claims(&mock).await;
    assert_eq!(claims["iat"], 1_000);
    assert_eq!(claims["exp"], 1_000 + ID_TOKEN_LIFETIME.as_secs());
}

#[tokio::test]
async fn discovery_switches_change_only_their_field() {
    let mock = MockIdp::start().await.expect("mock");
    let url = mock.url(Endpoint::Discovery);
    let baseline = object(json(&get(&url, None).await));

    mock.set_discovery_field("issuer", Some(json!(format!("{}/", mock.issuer()))));
    let faulted = object(json(&get(&url, None).await));
    assert_eq!(changed(&baseline, &faulted), ["issuer"]);
    assert_eq!(faulted["issuer"], format!("{}/", mock.issuer()));

    mock.clear_overrides();
    mock.set_discovery_field("userinfo_endpoint", None);
    let faulted = object(json(&get(&url, None).await));
    assert_eq!(changed(&baseline, &faulted), ["userinfo_endpoint"]);
    assert!(!faulted.contains_key("userinfo_endpoint"));
}

#[tokio::test]
async fn token_response_switches_change_only_their_field() {
    let mock = MockIdp::start().await.expect("mock");
    let baseline = object(common::oidc_tokens(&mock).await);
    let mut expected: Vec<&str> = baseline.keys().map(String::as_str).collect();
    expected.sort_unstable();
    assert_eq!(
        expected,
        [
            "access_token",
            "expires_in",
            "id_token",
            "refresh_token",
            "token_type"
        ]
    );
    assert_eq!(baseline["token_type"], "Bearer");

    mock.set_token_response_field("id_token", None);
    let faulted = object(common::oidc_tokens(&mock).await);
    assert!(!faulted.contains_key("id_token"));
    assert_eq!(faulted.len(), baseline.len() - 1);

    mock.clear_overrides();
    mock.set_token_response_field("token_type", Some(json!("mac")));
    let faulted = object(common::oidc_tokens(&mock).await);
    assert_eq!(faulted["token_type"], "mac");
    assert_eq!(faulted.len(), baseline.len());

    mock.clear_overrides();
    mock.set_token_response_field("access_token", None);
    let faulted = object(common::oidc_tokens(&mock).await);
    assert!(!faulted.contains_key("access_token"));
    assert!(faulted.contains_key("id_token"));
}

#[tokio::test]
async fn userinfo_switches_change_only_their_claim() {
    let mock = MockIdp::start().await.expect("mock");
    let tokens = common::oidc_tokens(&mock).await;
    let access = tokens["access_token"].as_str().expect("access_token");
    let url = mock.url(Endpoint::Userinfo);
    let baseline = object(json(&get(&url, Some(access)).await));

    mock.set_userinfo_claim("sub", Some(json!("someone-else")));
    let faulted = object(json(&get(&url, Some(access)).await));
    assert_eq!(changed(&baseline, &faulted), ["sub"]);

    mock.clear_overrides();
    mock.set_userinfo_claim("email", Some(json!("other@example.test")));
    let faulted = object(json(&get(&url, Some(access)).await));
    assert_eq!(changed(&baseline, &faulted), ["email"]);
    let (_, id) = decode_jwt(tokens["id_token"].as_str().expect("id_token"));
    assert_eq!(
        id["email"], "test.student@example.test",
        "the ID token keeps its email"
    );
}

#[tokio::test]
async fn an_authorize_error_redirects_with_the_error_and_the_state() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_authorize_error(Some(AuthorizeError {
        error: "access_denied".to_owned(),
        description: Some("The user said no".to_owned()),
    }));
    for (endpoint, oidc) in [
        (Endpoint::Authorize, true),
        (Endpoint::OAuth2Authorize, false),
    ] {
        let location = authorize(&mock, endpoint, &authorize_params(&mock, oidc)).await;
        assert!(location.as_str().starts_with(common::REDIRECT_URI));
        assert_eq!(param(&location, "error").as_deref(), Some("access_denied"));
        assert_eq!(
            param(&location, "error_description").as_deref(),
            Some("The user said no")
        );
        assert_eq!(param(&location, "state").as_deref(), Some(STATE));
        assert_eq!(param(&location, "code"), None);
    }
    mock.set_authorize_error(None);
    let location = authorize(&mock, Endpoint::Authorize, &authorize_params(&mock, true)).await;
    assert!(param(&location, "code").is_some());
}

#[tokio::test]
async fn a_status_fault_replaces_every_endpoints_answer_until_cleared() {
    let mock = MockIdp::start().await.expect("mock");
    for endpoint in Endpoint::ALL {
        mock.set_fault(endpoint, Fault::status(StatusCode::INTERNAL_SERVER_ERROR));
    }
    let tokens_form = [("grant_type", "authorization_code")];
    for endpoint in Endpoint::ALL {
        let url = mock.url(endpoint);
        let answer = match endpoint {
            Endpoint::Token | Endpoint::OAuth2Token => {
                common::post_form(&url, &tokens_form, None).await
            }
            _ => get(&url, None).await,
        };
        assert_eq!(answer.status(), 500, "{endpoint:?}");
        assert!(answer.body().is_empty(), "{endpoint:?}");
        assert_eq!(
            mock.requests_to(endpoint).len(),
            1,
            "recorded: {endpoint:?}"
        );
    }
    mock.clear_fault(Endpoint::Discovery);
    assert_eq!(
        get(&mock.url(Endpoint::Discovery), None).await.status(),
        200
    );
    assert_eq!(get(&mock.url(Endpoint::Jwks), None).await.status(), 500);
}

#[tokio::test]
async fn a_body_fault_answers_that_body() {
    let mock = MockIdp::start().await.expect("mock");
    let tokens = common::oidc_tokens(&mock).await;
    let access = tokens["access_token"].as_str().expect("access_token");
    mock.set_fault(
        Endpoint::Userinfo,
        Fault::body("application/json", "[1, 2]").expect("fault"),
    );
    let answer = get(&mock.url(Endpoint::Userinfo), Some(access)).await;
    assert_eq!(answer.status(), 200);
    assert_eq!(answer.headers()["content-type"], "application/json");
    assert_eq!(answer.body(), b"[1, 2]");

    mock.set_fault(
        Endpoint::Token,
        Fault::body("text/plain", "not json")
            .expect("fault")
            .with_status(StatusCode::BAD_GATEWAY),
    );
    let code = common::code(&mock, true).await;
    let answer = common::exchange(&mock, Endpoint::Token, &code, VERIFIER).await;
    assert_eq!(answer.status(), 502);
    assert_eq!(answer.body(), b"not json");
}

#[tokio::test]
async fn a_delay_fault_holds_the_answer_past_a_client_timeout() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_fault(
        Endpoint::Discovery,
        Fault::delay(Duration::from_millis(300)),
    );
    let started = Instant::now();
    let answer = get(&mock.url(Endpoint::Discovery), None).await;
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert_eq!(
        answer.status(),
        200,
        "a delay alone answers normally after it"
    );

    let impatient = OutboundClient::new(
        UrlPolicy::new(true, false),
        Duration::from_millis(50),
        1 << 20,
    )
    .expect("client");
    let request = http::Request::get(mock.url(Endpoint::Discovery))
        .body(Vec::new())
        .expect("request");
    assert!(matches!(
        impatient.send(request).await,
        Err(OutboundError::Timeout(_))
    ));
}

#[tokio::test]
async fn every_request_is_recorded_unknown_paths_and_wrong_methods_too() {
    let mock = MockIdp::start().await.expect("mock");
    let missing = get(
        &format!("{}/.well-known/openid-configuration", mock.base_url()),
        None,
    )
    .await;
    assert_eq!(missing.status(), 404);
    let wrong_method = common::post_form(&mock.url(Endpoint::Jwks), &[("a", "b")], None).await;
    assert_eq!(wrong_method.status(), 405);
    let location = authorize(&mock, Endpoint::Authorize, &authorize_params(&mock, true)).await;
    assert!(param(&location, "code").is_some());

    let recorded = mock.requests();
    assert_eq!(recorded.len(), 3);
    assert_eq!(recorded[0].endpoint, None);
    assert_eq!(recorded[0].path, "/.well-known/openid-configuration");
    assert_eq!(recorded[1].endpoint, Some(Endpoint::Jwks));
    assert_eq!(recorded[1].method, "POST");
    assert_eq!(recorded[1].form_param("a").as_deref(), Some("b"));
    let authorize = &recorded[2];
    assert_eq!(authorize.endpoint, Some(Endpoint::Authorize));
    assert_eq!(authorize.query_param("nonce"), Some(NONCE));
    assert_eq!(
        authorize.query_param("code_challenge"),
        Some(common::challenge(VERIFIER).as_str())
    );
    assert_eq!(authorize.query_param("code_challenge_method"), Some("S256"));
    assert!(authorize.header("host").is_some());
}

#[tokio::test]
async fn a_fault_body_needs_a_valid_content_type() {
    assert!(Fault::body("text/plain\n", "x").is_err());
    assert!(
        Fault::status(StatusCode::OK)
            .with_body("bad\u{7f}", "x")
            .is_err()
    );
}
