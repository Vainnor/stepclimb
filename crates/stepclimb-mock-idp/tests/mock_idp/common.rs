// SPDX-License-Identifier: AGPL-3.0-or-later
//! Requests to the mock through the server's own outbound client, with the insecure-providers
//! switch on so `http://127.0.0.1` is allowed.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use stepclimb_auth::outbound::{OutboundClient, UrlPolicy};
use stepclimb_mock_idp::{Endpoint, MockIdp};
use url::Url;

pub const REDIRECT_URI: &str = "http://127.0.0.1:9/auth/mock/callback";
pub const STATE: &str = "state-123";
pub const NONCE: &str = "nonce-456";
/// A verifier of RFC 7636's allowed form, 43 characters.
pub const VERIFIER: &str = "verifier-verifier-verifier-verifier-0123456";

pub fn client() -> OutboundClient {
    OutboundClient::new(UrlPolicy::new(true, false), Duration::from_secs(5), 1 << 20)
        .expect("client")
}

pub async fn get(url: &str, bearer: Option<&str>) -> http::Response<Vec<u8>> {
    let mut request = http::Request::builder().method("GET").uri(url);
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    client()
        .send(request.body(Vec::new()).expect("request"))
        .await
        .expect("response")
}

/// GET `url`; the status, or the transport error.
pub async fn get_result(url: &str) -> Result<u16, stepclimb_auth::outbound::OutboundError> {
    let request = http::Request::get(url).body(Vec::new()).expect("request");
    client().send(request).await.map(|r| r.status().as_u16())
}

/// POST a form, with `client_secret_basic` credentials when `basic` is set, form-encoded first
/// as RFC 6749 2.3.1 asks.
pub async fn post_form(
    url: &str,
    form: &[(&str, &str)],
    basic: Option<(&str, &str)>,
) -> http::Response<Vec<u8>> {
    let encode = |value: &str| -> String {
        url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
    };
    let basic = basic.map(|(id, secret)| (encode(id), encode(secret)));
    post_form_raw_basic(
        url,
        form,
        basic
            .as_ref()
            .map(|(id, secret)| (id.as_str(), secret.as_str())),
    )
    .await
}

/// [`post_form`] with the Basic credentials put in as given, not form-encoded.
pub async fn post_form_raw_basic(
    url: &str,
    form: &[(&str, &str)],
    basic: Option<(&str, &str)>,
) -> http::Response<Vec<u8>> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let mut request = http::Request::builder()
        .method("POST")
        .uri(url)
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some((id, secret)) = basic {
        let encoded = STANDARD.encode(format!("{id}:{secret}"));
        request = request.header("authorization", format!("Basic {encoded}"));
    }
    client()
        .send(request.body(body.into_bytes()).expect("request"))
        .await
        .expect("response")
}

pub fn json(response: &http::Response<Vec<u8>>) -> Value {
    serde_json::from_slice(response.body()).expect("a JSON body")
}

pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The authorize request a relying party sends: code flow, S256 PKCE for `VERIFIER`, `STATE`,
/// and for OIDC `openid` and `NONCE`.
pub fn authorize_params(mock: &MockIdp, oidc: bool) -> Vec<(String, String)> {
    let mut params = vec![
        ("response_type", "code".to_owned()),
        ("client_id", mock.client_id()),
        ("redirect_uri", REDIRECT_URI.to_owned()),
        ("state", STATE.to_owned()),
        ("code_challenge", challenge(VERIFIER)),
        ("code_challenge_method", "S256".to_owned()),
    ];
    if oidc {
        params.push(("scope", "openid email profile".to_owned()));
        params.push(("nonce", NONCE.to_owned()));
    } else {
        params.push(("scope", "full_name email".to_owned()));
    }
    params.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

/// GET the authorize endpoint with `params` and return the redirect's `Location`.
pub async fn authorize(mock: &MockIdp, endpoint: Endpoint, params: &[(String, String)]) -> Url {
    let mut url = Url::parse(&mock.url(endpoint)).expect("url");
    url.query_pairs_mut().extend_pairs(params);
    let response = get(url.as_str(), None).await;
    assert_eq!(response.status(), 302, "authorize redirects");
    let location = response.headers()["location"].to_str().expect("ascii");
    Url::parse(location).expect("location")
}

pub fn param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.into_owned())
}

/// Authorize with the default parameters and return the code.
pub async fn code(mock: &MockIdp, oidc: bool) -> String {
    let endpoint = if oidc {
        Endpoint::Authorize
    } else {
        Endpoint::OAuth2Authorize
    };
    let location = authorize(mock, endpoint, &authorize_params(mock, oidc)).await;
    param(&location, "code").expect("a code")
}

/// Exchange `code` with `verifier` and `client_secret_basic`.
pub async fn exchange(
    mock: &MockIdp,
    endpoint: Endpoint,
    code: &str,
    verifier: &str,
) -> http::Response<Vec<u8>> {
    let (id, secret) = (mock.client_id(), mock.client_secret());
    post_form(
        &mock.url(endpoint),
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier),
        ],
        Some((&id, &secret)),
    )
    .await
}

/// A whole OIDC code flow up to the token response, which must succeed.
pub async fn oidc_tokens(mock: &MockIdp) -> Value {
    let code = code(mock, true).await;
    let response = exchange(mock, Endpoint::Token, &code, VERIFIER).await;
    assert_eq!(response.status(), 200, "token: {:?}", json(&response));
    json(&response)
}

/// The decoded header and claims of a compact JWT.
pub fn decode_jwt(token: &str) -> (Value, Value) {
    let mut parts = token.split('.');
    let mut part = || {
        let bytes = URL_SAFE_NO_PAD
            .decode(parts.next().expect("a JWT part"))
            .expect("base64url");
        serde_json::from_slice(&bytes).expect("JSON")
    };
    (part(), part())
}
