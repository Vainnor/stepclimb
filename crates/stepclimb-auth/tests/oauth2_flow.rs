// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OAuth 2 code exchange and userinfo call against the mock IdP's `/oauth2` provider on
//! 127.0.0.1 (docs/specs/m0/06-oauth2-and-vatsim-preset.md B8, B9, T6, T7; Q3).

use std::time::Duration;

use serde_json::{Value, json};
use stepclimb_auth::{
    encryption::KeyRing,
    oauth2_config::{self, ClientSecret, Oauth2Settings, ProviderRow},
    oauth2_flow::{
        CallbackError, CodeCallback, CodeExchange, ExchangeError, FlowSecrets, UserinfoError,
        authorization_url, exchange_code, fetch_userinfo, redeem_code,
    },
    outbound::{OutboundClient, UrlPolicy},
};
use stepclimb_mock_idp::{Endpoint, Fault, MockIdp, StatusCode};
use url::Url;
use uuid::Uuid;

const REDIRECT: &str = "https://lms.example.org/auth/plain/callback";
const POLICY: UrlPolicy = UrlPolicy::new(true, true);

/// The mock, its OAuth 2 userinfo answering [`vatsim_shaped`].
async fn mock() -> MockIdp {
    let mock = MockIdp::start().await.expect("the mock starts");
    mock.set_oauth2_userinfo(Some(vatsim_shaped()));
    mock
}

fn client() -> OutboundClient {
    OutboundClient::for_providers(POLICY).expect("a client")
}

fn vatsim_shaped() -> Value {
    json!({
        "data": {
            "cid": 1000001,
            "personal": {
                "name_first": "Test",
                "name_last": "Student",
                "email": "test.student@example.test"
            },
            "vatsim": { "rating": { "short": "S1" }, "subdivision": { "id": "USA" } }
        }
    })
}

/// A provider config pointing at `mock`, with `pkce`, and the client secret opened.
fn provider(mock: &MockIdp, pkce: bool) -> (Oauth2Settings, ClientSecret) {
    let config = json!({
        "authorize": mock.url(Endpoint::OAuth2Authorize),
        "token": mock.url(Endpoint::OAuth2Token),
        "userinfo": mock.url(Endpoint::OAuth2Userinfo),
        "client_id": mock.client_id(),
        "client_secret": mock.client_secret(),
        "scopes": ["full_name", "email"],
        "pkce": pkce,
        "claims": {
            "subject": "/data/cid",
            "given_name": "/data/personal/name_first",
            "family_name": "/data/personal/name_last",
            "email": "/data/personal/email",
            "attributes": {
                "rating": "/data/vatsim/rating/short",
                "subdivision": "/data/vatsim/subdivision/id"
            }
        }
    });
    let ring = KeyRing::parse("k1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=").expect("a ring");
    let row = ProviderRow {
        org_id: Uuid::from_u128(1),
        provider_id: Uuid::from_u128(2),
    };
    let sealed = oauth2_config::validate(&config, POLICY, None)
        .expect("a valid config")
        .seal(&ring, row)
        .expect("seal");
    let secret = sealed.client_secret(&ring, row).expect("the secret opens");
    (sealed.settings, secret)
}

/// Follow the authorization URL at the mock, which approves at once: the code it sends back,
/// after checking the state came back unchanged.
async fn authorize(settings: &Oauth2Settings, secrets: &FlowSecrets) -> String {
    let url = authorization_url(settings, REDIRECT, secrets, POLICY).expect("a URL");
    let request = http::Request::get(url.as_str())
        .body(Vec::new())
        .expect("a request");
    let response = client().send(request).await.expect("the mock answers");
    assert_eq!(response.status(), StatusCode::FOUND, "the mock approves");
    let back = Url::parse(
        response.headers()[http::header::LOCATION]
            .to_str()
            .expect("ASCII"),
    )
    .expect("a URL");
    let param = |name: &str| {
        back.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    assert_eq!(param("state").as_deref(), Some(secrets.state()));
    param("code").expect("a code")
}

async fn redeem(
    mock: &MockIdp,
    pkce: bool,
) -> Result<stepclimb_auth::oauth2_config::MappedSignIn, CallbackError> {
    let (settings, secret) = provider(mock, pkce);
    let secrets = FlowSecrets::generate(pkce).expect("random secrets");
    let code = authorize(&settings, &secrets).await;
    let client = client();
    let callback = CodeCallback {
        client: &client,
        settings: &settings,
        client_secret: &secret,
        redirect_uri: REDIRECT,
        secrets: &secrets,
        code: &code,
    };
    redeem_code(&callback).await
}

/// Spec 06 T7, T15's shape: the full exchange maps a VATSIM-shaped body, and the userinfo call
/// carries the issued access token as a Bearer credential. The Basic credentials carry a secret
/// with `+`, `/` and `=`, which only form-encoding gets through (RFC 6749 2.3.1).
#[tokio::test]
async fn spec_t7_a_full_exchange_maps_the_userinfo_body() {
    let mock = mock().await;

    let signed_in = redeem(&mock, true).await.expect("the sign-in maps");

    assert_eq!(signed_in.subject, "1000001");
    assert_eq!(signed_in.profile.given_name.as_deref(), Some("Test"));
    assert_eq!(signed_in.profile.family_name.as_deref(), Some("Student"));
    assert_eq!(
        signed_in.profile.email.as_deref(),
        Some("test.student@example.test")
    );
    assert_eq!(
        Value::Object(signed_in.profile.attributes),
        json!({ "rating": "S1", "subdivision": "USA" })
    );
    let issued = mock.issued_tokens();
    let [tokens] = issued.as_slice() else {
        panic!("one token response, got {}", issued.len());
    };
    let userinfo = mock.requests_to(Endpoint::OAuth2Userinfo);
    let [call] = userinfo.as_slice() else {
        panic!("one userinfo call");
    };
    assert_eq!(call.method, "GET");
    assert_eq!(
        call.header("authorization"),
        Some(format!("Bearer {}", tokens.access_token).as_str())
    );
    assert_eq!(call.header("accept"), Some("application/json"));
    let token = mock.requests_to(Endpoint::OAuth2Token);
    assert!(
        token[0]
            .header("authorization")
            .is_some_and(|h| h.starts_with("Basic ")),
        "client_secret_basic"
    );
    assert_eq!(token[0].form_param("client_secret"), None);
}

/// Q3: with `pkce` off, no verifier is sent, and a mock that does not require PKCE redeems the
/// code; with it on, the verifier is sent and the mock checks it against the challenge.
#[tokio::test]
async fn q3_the_verifier_is_sent_exactly_when_pkce_is_on() {
    let mock = mock().await;
    redeem(&mock, true).await.expect("PKCE on");
    let sent = mock.requests_to(Endpoint::OAuth2Token);
    assert!(sent[0].form_param("code_verifier").is_some());

    mock.set_require_pkce(false);
    redeem(&mock, false).await.expect("PKCE off");
    let sent = mock.requests_to(Endpoint::OAuth2Token);
    assert_eq!(sent[1].form_param("code_verifier"), None);
    let authorize = mock.requests_to(Endpoint::OAuth2Authorize);
    assert_eq!(authorize[1].query_param("code_challenge"), None);
}

/// A verifier that is not the flow's is refused by the mock: the one the flow sends is checked.
#[tokio::test]
async fn a_wrong_verifier_is_a_provider_error() {
    let mock = mock().await;
    let (settings, secret) = provider(&mock, true);
    let secrets = FlowSecrets::generate(true).unwrap();
    let code = authorize(&settings, &secrets).await;
    let other = FlowSecrets::generate(true).unwrap();
    let exchange = CodeExchange {
        token_url: &settings.token,
        client_id: &settings.client_id,
        client_secret: &secret,
        redirect_uri: REDIRECT,
        code: &code,
        code_verifier: other.code_verifier(),
    };
    let error = exchange_code(&client(), &exchange).await.unwrap_err();
    assert!(
        matches!(&error, ExchangeError::Status { status: 400, error: Some(e) } if e == "invalid_grant"),
        "{error:?}"
    );
}

async fn exchange_error(mock: &MockIdp, client: &OutboundClient) -> ExchangeError {
    let (settings, secret) = provider(mock, true);
    let secrets = FlowSecrets::generate(true).expect("random secrets");
    let code = authorize(&settings, &secrets).await;
    let exchange = CodeExchange {
        token_url: &settings.token,
        client_id: &settings.client_id,
        client_secret: &secret,
        redirect_uri: REDIRECT,
        code: &code,
        code_verifier: secrets.code_verifier(),
    };
    exchange_code(client, &exchange)
        .await
        .expect_err("the exchange is refused")
}

/// Spec 06 T6: no `access_token`, `token_type` `mac`, status 400, or a delay past the timeout
/// are refused; `token_type` `Bearer` in another case is accepted.
#[tokio::test]
async fn spec_t6_a_bad_token_response_is_refused() {
    let mock = mock().await;
    let client = client();

    mock.set_token_response_field("access_token", None);
    let missing = exchange_error(&mock, &client).await;
    assert!(matches!(missing, ExchangeError::Parse(_)), "{missing:?}");
    mock.clear_overrides();

    mock.set_token_response_field("token_type", Some(json!("mac")));
    let mac = exchange_error(&mock, &client).await;
    assert!(matches!(mac, ExchangeError::TokenType), "{mac:?}");

    mock.set_token_response_field("token_type", Some(json!(7)));
    let typed = exchange_error(&mock, &client).await;
    assert!(matches!(typed, ExchangeError::Parse(_)), "{typed:?}");
    mock.clear_overrides();

    mock.set_fault(
        Endpoint::OAuth2Token,
        Fault::status(StatusCode::BAD_REQUEST),
    );
    let status = exchange_error(&mock, &client).await;
    assert!(
        matches!(
            status,
            ExchangeError::Status {
                status: 400,
                error: None
            }
        ),
        "{status:?}"
    );
    mock.set_fault(
        Endpoint::OAuth2Token,
        Fault::body("text/html", "<html>").unwrap(),
    );
    let html = exchange_error(&mock, &client).await;
    assert!(matches!(html, ExchangeError::NotJson), "{html:?}");
    mock.set_fault(
        Endpoint::OAuth2Token,
        Fault::body("application/json", "[]").unwrap(),
    );
    let array = exchange_error(&mock, &client).await;
    assert!(matches!(array, ExchangeError::Parse(_)), "{array:?}");

    mock.set_fault(Endpoint::OAuth2Token, Fault::delay(Duration::from_secs(5)));
    let short = OutboundClient::new(POLICY, Duration::from_millis(200), 1024 * 1024).unwrap();
    let slow = exchange_error(&mock, &short).await;
    assert!(matches!(slow, ExchangeError::Fetch(_)), "{slow:?}");
    mock.clear_fault(Endpoint::OAuth2Token);

    mock.set_token_response_field("token_type", Some(json!("BeArEr")));
    redeem(&mock, true).await.expect("bearer in any case");
}

/// Spec 06 T7: a userinfo answering 401, a JSON array, or a body that is not JSON is refused.
#[tokio::test]
async fn spec_t7_a_bad_userinfo_answer_is_refused() {
    let mock = mock().await;
    let client = client();
    let url = mock.url(Endpoint::OAuth2Userinfo);

    let unknown = fetch_userinfo(&client, &url, "not-a-token-the-mock-issued").await;
    assert!(
        matches!(unknown, Err(UserinfoError::Status(401))),
        "{unknown:?}"
    );

    mock.set_oauth2_userinfo(Some(json!([{ "data": { "cid": 1000001 } }])));
    let array = redeem(&mock, true).await;
    assert!(
        matches!(
            array,
            Err(CallbackError::Userinfo(UserinfoError::NotObject))
        ),
        "{array:?}"
    );
    mock.set_fault(
        Endpoint::OAuth2Userinfo,
        Fault::body("application/json", "{not json").unwrap(),
    );
    let broken = redeem(&mock, true).await;
    assert!(
        matches!(
            broken,
            Err(CallbackError::Userinfo(UserinfoError::NotObject))
        ),
        "{broken:?}"
    );
    mock.set_fault(
        Endpoint::OAuth2Userinfo,
        Fault::status(StatusCode::INTERNAL_SERVER_ERROR),
    );
    let failed = redeem(&mock, true).await;
    assert!(
        matches!(
            failed,
            Err(CallbackError::Userinfo(UserinfoError::Status(500)))
        ),
        "{failed:?}"
    );
}

/// Spec 06 B6 through the whole callback: a userinfo body without a usable subject.
#[tokio::test]
async fn a_body_without_a_usable_subject_is_subject_missing() {
    let mock = mock().await;
    for body in [
        json!({ "data": {} }),
        json!({ "data": { "cid": true } }),
        json!({ "data": { "cid": "" } }),
    ] {
        mock.set_oauth2_userinfo(Some(body.clone()));
        let refused = redeem(&mock, true).await;
        assert!(
            matches!(refused, Err(CallbackError::Subject(_))),
            "{body}: {refused:?}"
        );
    }
}

/// The Basic credentials are form-encoded before base64 (RFC 6749 2.3.1): the mock's `+/=`
/// secret authenticates, and once the mock holds another secret the stored one is refused.
#[tokio::test]
async fn the_client_authenticates_with_form_encoded_basic_credentials() {
    let mock = mock().await;
    assert!(mock.client_secret().contains("+/="));
    redeem(&mock, true)
        .await
        .expect("the encoded secret authenticates");

    let (settings, secret) = provider(&mock, true);
    mock.set_client_secret("a-different-secret");
    let secrets = FlowSecrets::generate(true).unwrap();
    let code = authorize(&settings, &secrets).await;
    let exchange = CodeExchange {
        token_url: &settings.token,
        client_id: &settings.client_id,
        client_secret: &secret,
        redirect_uri: REDIRECT,
        code: &code,
        code_verifier: secrets.code_verifier(),
    };
    let error = exchange_code(&client(), &exchange).await.unwrap_err();
    assert!(
        matches!(error, ExchangeError::Status { status: 401, .. }),
        "{error:?}"
    );
}
