// SPDX-License-Identifier: AGPL-3.0-or-later
//! The plain OAuth 2 provider: a code flow without an ID token, and a userinfo endpoint that
//! answers the JSON the test sets.

use serde_json::json;
use stepclimb_mock_idp::{Endpoint, MockIdp};

use crate::common::{VERIFIER, code, exchange, get, json};

#[tokio::test]
async fn the_token_response_has_no_id_token() {
    let mock = MockIdp::start().await.expect("mock");
    let code = code(&mock, false).await;
    let answer = exchange(&mock, Endpoint::OAuth2Token, &code, VERIFIER).await;
    assert_eq!(answer.status(), 200);
    assert_eq!(answer.headers()["cache-control"], "no-store");
    let body = json(&answer);
    assert_eq!(body["token_type"], "Bearer");
    assert!(body.get("id_token").is_none());
    let issued = mock.issued_tokens();
    assert_eq!(issued[0].endpoint, Endpoint::OAuth2Token);
    assert_eq!(issued[0].id_token, None);
    assert_eq!(body["access_token"], issued[0].access_token.as_str());
}

#[tokio::test]
async fn userinfo_answers_the_body_the_test_set() {
    let mock = MockIdp::start().await.expect("mock");
    let code = code(&mock, false).await;
    let tokens = json(&exchange(&mock, Endpoint::OAuth2Token, &code, VERIFIER).await);
    let access = tokens["access_token"].as_str().expect("access_token");
    let url = mock.url(Endpoint::OAuth2Userinfo);

    let default = json(&get(&url, Some(access)).await);
    assert_eq!(default["sub"], "mock-user-1");
    assert_eq!(default["family_name"], "Student");

    let body = json!({
        "data": {
            "cid": 1_000_001,
            "personal": { "name_first": "Test", "name_last": "Student" },
            "vatsim": { "rating": { "short": "S1" } }
        }
    });
    mock.set_oauth2_userinfo(Some(body.clone()));
    let answer = get(&url, Some(access)).await;
    assert_eq!(answer.status(), 200);
    assert_eq!(json(&answer), body);
    let recorded = mock.requests_to(Endpoint::OAuth2Userinfo);
    assert_eq!(
        recorded[1].header("authorization"),
        Some(format!("Bearer {access}").as_str())
    );

    mock.set_userinfo_claim("sub", Some(json!("ignored")));
    assert_eq!(
        json(&get(&url, Some(access)).await),
        body,
        "OIDC overrides do not apply"
    );
    assert_eq!(
        get(&mock.url(Endpoint::Userinfo), Some(access))
            .await
            .status(),
        401,
        "an OAuth 2 token is not an OIDC one"
    );
}
