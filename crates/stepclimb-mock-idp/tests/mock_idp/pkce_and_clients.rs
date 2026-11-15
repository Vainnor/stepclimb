// SPDX-License-Identifier: AGPL-3.0-or-later
//! The token endpoints' checks: the PKCE verifier, single-use codes, the redirect URI and client
//! authentication; and the authorize endpoints' refusals.

use serde_json::json;
use stepclimb_mock_idp::{ClientAuth, Endpoint, MockIdp};

use crate::common::{
    REDIRECT_URI, STATE, VERIFIER, authorize, authorize_params, code, exchange, get, json, param,
    post_form,
};

fn error(response: &http::Response<Vec<u8>>) -> String {
    json(response)["error"]
        .as_str()
        .expect("an error code")
        .to_owned()
}

#[tokio::test]
async fn a_wrong_or_missing_verifier_gets_invalid_grant() {
    let mock = MockIdp::start().await.expect("mock");
    for endpoint in [Endpoint::Token, Endpoint::OAuth2Token] {
        let oidc = endpoint == Endpoint::Token;
        let wrong = format!("{}x", &VERIFIER[..VERIFIER.len() - 1]);
        let answer = exchange(&mock, endpoint, &code(&mock, oidc).await, &wrong).await;
        assert_eq!(answer.status(), 400);
        assert_eq!(error(&answer), "invalid_grant", "{endpoint:?}");

        let (id, secret) = (mock.client_id(), mock.client_secret());
        let code = code(&mock, oidc).await;
        let missing = post_form(
            &mock.url(endpoint),
            &[
                ("grant_type", "authorization_code"),
                ("code", &code),
                ("redirect_uri", REDIRECT_URI),
            ],
            Some((&id, &secret)),
        )
        .await;
        assert_eq!(error(&missing), "invalid_grant", "{endpoint:?}");

        let right = exchange(
            &mock,
            endpoint,
            &crate::common::code(&mock, oidc).await,
            VERIFIER,
        )
        .await;
        assert_eq!(right.status(), 200, "{endpoint:?}");
    }
    assert_eq!(
        mock.issued_tokens().len(),
        2,
        "only the right verifiers got tokens"
    );
}

#[tokio::test]
async fn a_code_is_spent_by_its_first_use_even_a_failed_one() {
    let mock = MockIdp::start().await.expect("mock");
    let code = code(&mock, true).await;
    let first = exchange(&mock, Endpoint::Token, &code, &"b".repeat(43)).await;
    assert_eq!(error(&first), "invalid_grant");
    let again = exchange(&mock, Endpoint::Token, &code, VERIFIER).await;
    assert_eq!(error(&again), "invalid_grant");

    let code = crate::common::code(&mock, true).await;
    assert_eq!(
        exchange(&mock, Endpoint::Token, &code, VERIFIER)
            .await
            .status(),
        200
    );
    let replay = exchange(&mock, Endpoint::Token, &code, VERIFIER).await;
    assert_eq!(error(&replay), "invalid_grant");
}

#[tokio::test]
async fn a_code_works_only_at_its_own_provider_and_redirect_uri() {
    let mock = MockIdp::start().await.expect("mock");
    let oidc_code = code(&mock, true).await;
    let at_oauth2 = exchange(&mock, Endpoint::OAuth2Token, &oidc_code, VERIFIER).await;
    assert_eq!(error(&at_oauth2), "invalid_grant");

    let code = code(&mock, true).await;
    let (id, secret) = (mock.client_id(), mock.client_secret());
    let elsewhere = post_form(
        &mock.url(Endpoint::Token),
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", "http://127.0.0.1:9/other"),
            ("code_verifier", VERIFIER),
        ],
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(error(&elsewhere), "invalid_grant");
}

#[tokio::test]
async fn authorize_refuses_plain_and_missing_pkce_unless_pkce_is_optional() {
    let mock = MockIdp::start().await.expect("mock");
    let with = |name: &str, value: Option<&str>| {
        let mut params: Vec<(String, String)> = authorize_params(&mock, true)
            .into_iter()
            .filter(|(k, _)| k != name)
            .collect();
        if let Some(value) = value {
            params.push((name.to_owned(), value.to_owned()));
        }
        params
    };
    let plain = with("code_challenge_method", Some("plain"));
    let refused = authorize(&mock, Endpoint::Authorize, &plain).await;
    assert_eq!(param(&refused, "error").as_deref(), Some("invalid_request"));
    assert_eq!(param(&refused, "state").as_deref(), Some(STATE));
    assert_eq!(param(&refused, "code"), None);

    let without: Vec<(String, String)> = with("code_challenge", None)
        .into_iter()
        .filter(|(k, _)| k != "code_challenge_method")
        .collect();
    let refused = authorize(&mock, Endpoint::Authorize, &without).await;
    assert_eq!(param(&refused, "error").as_deref(), Some("invalid_request"));

    mock.set_require_pkce(false);
    let location = authorize(&mock, Endpoint::OAuth2Authorize, &without).await;
    let code = param(&location, "code").expect("a code without PKCE");
    let (id, secret) = (mock.client_id(), mock.client_secret());
    let form = |verifier: Option<&'static str>| {
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("redirect_uri", REDIRECT_URI),
        ];
        if let Some(verifier) = verifier {
            form.push(("code_verifier", verifier));
        }
        form
    };
    let mut no_pkce = form(None);
    no_pkce.push(("code", &code));
    let answer = post_form(
        &mock.url(Endpoint::OAuth2Token),
        &no_pkce,
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(answer.status(), 200);

    let code = param(
        &authorize(&mock, Endpoint::OAuth2Authorize, &without).await,
        "code",
    )
    .expect("code");
    let mut stray = form(Some(VERIFIER));
    stray.push(("code", &code));
    let answer = post_form(
        &mock.url(Endpoint::OAuth2Token),
        &stray,
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(
        error(&answer),
        "invalid_grant",
        "a verifier without a challenge"
    );
}

#[tokio::test]
async fn authorize_checks_the_client_the_response_type_and_the_openid_scope() {
    let mock = MockIdp::start().await.expect("mock");
    let mut url = url::Url::parse(&mock.url(Endpoint::Authorize)).expect("url");
    url.query_pairs_mut()
        .extend_pairs(authorize_params(&mock, true).into_iter().map(|(k, v)| {
            if k == "client_id" {
                (k, "someone-else".to_owned())
            } else {
                (k, v)
            }
        }));
    assert_eq!(
        get(url.as_str(), None).await.status(),
        400,
        "no redirect for an unknown client"
    );

    let params: Vec<(String, String)> = authorize_params(&mock, true)
        .into_iter()
        .map(|(k, v)| {
            if k == "response_type" {
                (k, "token".to_owned())
            } else {
                (k, v)
            }
        })
        .collect();
    let location = authorize(&mock, Endpoint::Authorize, &params).await;
    assert_eq!(
        param(&location, "error").as_deref(),
        Some("unsupported_response_type")
    );

    let params: Vec<(String, String)> = authorize_params(&mock, true)
        .into_iter()
        .map(|(k, v)| {
            if k == "scope" {
                (k, "email".to_owned())
            } else {
                (k, v)
            }
        })
        .collect();
    let location = authorize(&mock, Endpoint::Authorize, &params).await;
    assert_eq!(param(&location, "error").as_deref(), Some("invalid_scope"));
    let oauth2 = authorize(&mock, Endpoint::OAuth2Authorize, &params).await;
    assert!(
        param(&oauth2, "code").is_some(),
        "OAuth 2 needs no openid scope"
    );
}

#[tokio::test]
async fn client_authentication_follows_the_allowed_methods() {
    let mock = MockIdp::start().await.expect("mock");
    let (id, secret) = (mock.client_id(), mock.client_secret());
    let post = |code: String| {
        let (id, secret) = (id.clone(), secret.clone());
        let url = mock.url(Endpoint::Token);
        async move {
            post_form(
                &url,
                &[
                    ("grant_type", "authorization_code"),
                    ("code", &code),
                    ("redirect_uri", REDIRECT_URI),
                    ("code_verifier", VERIFIER),
                    ("client_id", &id),
                    ("client_secret", &secret),
                ],
                None,
            )
            .await
        }
    };
    assert_eq!(
        post(code(&mock, true).await).await.status(),
        200,
        "client_secret_post"
    );

    let wrong = post_form(
        &mock.url(Endpoint::Token),
        &[
            ("grant_type", "authorization_code"),
            ("code", &code(&mock, true).await),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", VERIFIER),
        ],
        Some((&id, "not-the-secret")),
    )
    .await;
    assert_eq!(wrong.status(), 401);
    assert_eq!(error(&wrong), "invalid_client");

    mock.set_token_auth_methods(&[ClientAuth::Post]);
    let discovery = json(&get(&mock.url(Endpoint::Discovery), None).await);
    assert_eq!(
        discovery["token_endpoint_auth_methods_supported"],
        json!(["client_secret_post"])
    );
    let basic = exchange(&mock, Endpoint::Token, &code(&mock, true).await, VERIFIER).await;
    assert_eq!(
        basic.status(),
        401,
        "Basic is refused once only post is allowed"
    );
    assert_eq!(post(code(&mock, true).await).await.status(), 200);

    let recorded = mock.requests_to(Endpoint::Token);
    assert_eq!(
        recorded[0].form_param("client_secret").as_deref(),
        Some(secret.as_str())
    );
    assert!(
        recorded[1]
            .header("authorization")
            .is_some_and(|h| h.starts_with("Basic "))
    );
}

/// A token request with `client_secret_basic` for `code`, the default verifier and
/// `redirect_uri`, and `grant_type`.
async fn token_request(
    mock: &MockIdp,
    code: &str,
    grant_type: &str,
    redirect_uri: &str,
    secret: &str,
) -> http::Response<Vec<u8>> {
    let id = mock.client_id();
    post_form(
        &mock.url(Endpoint::Token),
        &[
            ("grant_type", grant_type),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", VERIFIER),
        ],
        Some((&id, secret)),
    )
    .await
}

#[tokio::test]
async fn a_request_that_fails_client_authentication_does_not_spend_the_code() {
    let mock = MockIdp::start().await.expect("mock");
    let code = code(&mock, true).await;
    let refused = token_request(&mock, &code, "authorization_code", REDIRECT_URI, "wrong").await;
    assert_eq!(refused.status(), 401);
    let secret = mock.client_secret();
    let answer = token_request(&mock, &code, "authorization_code", REDIRECT_URI, &secret).await;
    assert_eq!(
        answer.status(),
        200,
        "the code survived the unauthenticated try"
    );
}

#[tokio::test]
async fn an_authenticated_request_spends_the_code_even_with_a_wrong_grant_type() {
    let mock = MockIdp::start().await.expect("mock");
    let code = code(&mock, true).await;
    let secret = mock.client_secret();
    let wrong = token_request(&mock, &code, "password", REDIRECT_URI, &secret).await;
    assert_eq!(error(&wrong), "unsupported_grant_type");
    let again = token_request(&mock, &code, "authorization_code", REDIRECT_URI, &secret).await;
    assert_eq!(error(&again), "invalid_grant");
}

#[tokio::test]
async fn the_redirect_uri_must_repeat_the_authorize_value_exactly() {
    let mock = MockIdp::start().await.expect("mock");
    let secret = mock.client_secret();
    for (sent, repeated) in [
        ("http://h.test:1", "http://h.test:1/"),
        ("http://H.test/cb", "http://h.test/cb"),
    ] {
        let params: Vec<(String, String)> = authorize_params(&mock, true)
            .into_iter()
            .map(|(k, v)| {
                if k == "redirect_uri" {
                    (k, sent.to_owned())
                } else {
                    (k, v)
                }
            })
            .collect();
        let location = authorize(&mock, Endpoint::Authorize, &params).await;
        let code = param(&location, "code").expect("code");
        let answer = token_request(&mock, &code, "authorization_code", repeated, &secret).await;
        assert_eq!(error(&answer), "invalid_grant", "{sent} then {repeated}");

        let location = authorize(&mock, Endpoint::Authorize, &params).await;
        let code = param(&location, "code").expect("code");
        let answer = token_request(&mock, &code, "authorization_code", sent, &secret).await;
        assert_eq!(answer.status(), 200, "{sent} repeated as sent");
    }
}

#[tokio::test]
async fn the_redirect_uri_allowlist_matches_exact_strings() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_redirect_uris(Some(&[REDIRECT_URI]));
    assert!(
        param(
            &authorize(&mock, Endpoint::Authorize, &authorize_params(&mock, true)).await,
            "code"
        )
        .is_some()
    );
    let with_slash: Vec<(String, String)> = authorize_params(&mock, true)
        .into_iter()
        .map(|(k, v)| {
            if k == "redirect_uri" {
                (k, format!("{v}/"))
            } else {
                (k, v)
            }
        })
        .collect();
    let mut url = url::Url::parse(&mock.url(Endpoint::Authorize)).expect("url");
    url.query_pairs_mut().extend_pairs(&with_slash);
    assert_eq!(
        get(url.as_str(), None).await.status(),
        400,
        "no redirect to an unlisted URI"
    );
    mock.set_redirect_uris(None);
    assert!(
        param(
            &authorize(&mock, Endpoint::Authorize, &with_slash).await,
            "code"
        )
        .is_some()
    );
}

#[tokio::test]
async fn basic_credentials_must_be_form_encoded() {
    let mock = MockIdp::start().await.expect("mock");
    let (id, secret) = (mock.client_id(), mock.client_secret());
    assert!(secret.contains('+') && secret.contains('/') && secret.contains('='));
    let form = |code: &str| {
        vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", REDIRECT_URI.to_owned()),
            ("code_verifier", VERIFIER.to_owned()),
        ]
    };
    let code = code(&mock, true).await;
    let pairs = form(&code);
    let pairs: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let raw = crate::common::post_form_raw_basic(
        &mock.url(Endpoint::Token),
        &pairs,
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(raw.status(), 401, "an unencoded `+` decodes as a space");
    let encoded = post_form(&mock.url(Endpoint::Token), &pairs, Some((&id, &secret))).await;
    assert_eq!(encoded.status(), 200);

    mock.set_client_secret("plain-secret");
    let code = crate::common::code(&mock, true).await;
    let answer = exchange(&mock, Endpoint::Token, &code, VERIFIER).await;
    assert_eq!(answer.status(), 200, "the set secret authenticates");
}

#[tokio::test]
async fn ambiguous_client_authentication_is_refused() {
    let mock = MockIdp::start().await.expect("mock");
    let (id, secret) = (mock.client_id(), mock.client_secret());
    let code = code(&mock, true).await;
    let both = post_form(
        &mock.url(Endpoint::Token),
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", VERIFIER),
            ("client_secret", &secret),
        ],
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(both.status(), 400);
    assert_eq!(error(&both), "invalid_request", "Basic and post at once");

    let other_id = post_form(
        &mock.url(Endpoint::Token),
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", VERIFIER),
            ("client_id", "someone-else"),
        ],
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(
        error(&other_id),
        "invalid_request",
        "a form client_id unlike Basic's"
    );
}

#[tokio::test]
async fn a_token_body_must_be_form_encoded() {
    let mock = MockIdp::start().await.expect("mock");
    let request = http::Request::post(mock.url(Endpoint::Token))
        .header("content-type", "application/json")
        .body(br#"{"grant_type":"authorization_code"}"#.to_vec())
        .expect("request");
    let answer = crate::common::client()
        .send(request)
        .await
        .expect("response");
    assert_eq!(answer.status(), 400);
    assert_eq!(error(&answer), "invalid_request");
}

#[tokio::test]
async fn a_method_without_a_challenge_and_a_malformed_challenge_are_refused() {
    let mock = MockIdp::start().await.expect("mock");
    let method_only: Vec<(String, String)> = authorize_params(&mock, true)
        .into_iter()
        .filter(|(k, _)| k != "code_challenge")
        .collect();
    let location = authorize(&mock, Endpoint::Authorize, &method_only).await;
    assert_eq!(
        param(&location, "error").as_deref(),
        Some("invalid_request")
    );

    let malformed: Vec<(String, String)> = authorize_params(&mock, true)
        .into_iter()
        .map(|(k, v)| {
            if k == "code_challenge" {
                (k, "too-short".to_owned())
            } else {
                (k, v)
            }
        })
        .collect();
    let location = authorize(&mock, Endpoint::Authorize, &malformed).await;
    assert_eq!(
        param(&location, "error").as_deref(),
        Some("invalid_request")
    );
    assert_eq!(param(&location, "code"), None);
}
