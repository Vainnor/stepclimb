// SPDX-License-Identifier: AGPL-3.0-or-later
//! The OIDC provider read and driven by the `openidconnect` crate: discovery, the JWKS, a whole
//! code flow with PKCE, ID token checks, userinfo and key rotation.

use std::str::FromStr as _;

use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreIdToken, CoreIdTokenVerifier, CoreJsonWebKeySet,
    CoreProviderMetadata, CoreUserInfoClaims,
};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse as _, PkceCodeChallenge, RedirectUrl, Scope, TokenResponse as _,
};
use serde_json::json;
use stepclimb_auth::outbound::OutboundError;
use stepclimb_mock_idp::{Endpoint, IdTokenSigning, MockIdp, MockUser};

use crate::common::{self, REDIRECT_URI, get, json, param};

/// The `openidconnect` HTTP client: the server's outbound client.
fn http() -> impl Fn(
    openidconnect::HttpRequest,
) -> std::pin::Pin<
    Box<dyn Future<Output = Result<openidconnect::HttpResponse, OutboundError>> + Send>,
> {
    let client = common::client();
    move |request| {
        let client = client.clone();
        Box::pin(async move { client.send(request).await })
    }
}

async fn metadata(mock: &MockIdp) -> CoreProviderMetadata {
    let issuer = IssuerUrl::new(mock.issuer()).expect("issuer");
    CoreProviderMetadata::discover_async(issuer, &http())
        .await
        .expect("discovery and JWKS parse and the issuer matches")
}

async fn jwks(mock: &MockIdp) -> CoreJsonWebKeySet {
    let response = get(&mock.url(Endpoint::Jwks), None).await;
    serde_json::from_slice(response.body()).expect("a JWKS")
}

fn verifier(mock: &MockIdp, keys: CoreJsonWebKeySet) -> CoreIdTokenVerifier<'static> {
    CoreIdTokenVerifier::new_confidential_client(
        ClientId::new(mock.client_id()),
        ClientSecret::new(mock.client_secret()),
        IssuerUrl::new(mock.issuer()).expect("issuer"),
        keys,
    )
}

fn id_token(tokens: &serde_json::Value) -> CoreIdToken {
    CoreIdToken::from_str(tokens["id_token"].as_str().expect("id_token")).expect("a JWT")
}

#[tokio::test]
async fn discovery_and_jwks_parse_with_openidconnect() {
    let mock = MockIdp::start().await.expect("mock");
    let metadata = metadata(&mock).await;
    assert_eq!(metadata.issuer().as_str(), mock.issuer());
    assert_eq!(
        metadata.authorization_endpoint().as_str(),
        mock.url(Endpoint::Authorize)
    );
    assert_eq!(
        metadata.token_endpoint().map(|u| u.as_str().to_owned()),
        Some(mock.url(Endpoint::Token))
    );
    assert_eq!(
        metadata.userinfo_endpoint().map(|u| u.as_str().to_owned()),
        Some(mock.url(Endpoint::Userinfo))
    );
    let kids: Vec<String> = metadata
        .jwks()
        .keys()
        .iter()
        .map(|key| {
            use openidconnect::JsonWebKey as _;
            key.key_id().expect("kid").to_string()
        })
        .collect();
    assert_eq!(kids, [mock.signing_kid()]);
    assert_eq!(mock.requests_to(Endpoint::Discovery).len(), 1);
    assert_eq!(mock.requests_to(Endpoint::Jwks).len(), 1);
}

#[tokio::test]
async fn a_whole_sign_in_through_the_openidconnect_client_succeeds() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_user(
        MockUser::new("1000001")
            .with_claim("email", json!("student@example.test"))
            .with_claim("department", json!("Training")),
    );
    let http = http();
    let client = CoreClient::from_provider_metadata(
        metadata(&mock).await,
        ClientId::new(mock.client_id()),
        Some(ClientSecret::new(mock.client_secret())),
    )
    .set_redirect_uri(RedirectUrl::new(REDIRECT_URI.to_owned()).expect("redirect"));
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let (url, csrf, nonce) = client
        .authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            CsrfToken::new_random,
            Nonce::new_random,
        )
        .add_scope(Scope::new("email".to_owned()))
        .set_pkce_challenge(challenge)
        .url();

    let response = get(url.as_str(), None).await;
    assert_eq!(response.status(), 302);
    let location =
        url::Url::parse(response.headers()["location"].to_str().expect("ascii")).expect("location");
    assert!(location.as_str().starts_with(REDIRECT_URI));
    assert_eq!(
        param(&location, "state").as_deref(),
        Some(csrf.secret().as_str())
    );
    let code = param(&location, "code").expect("code");

    let tokens = client
        .exchange_code(AuthorizationCode::new(code))
        .expect("token endpoint")
        .set_pkce_verifier(verifier)
        .request_async(&http)
        .await
        .expect("the token request succeeds");
    let id_token = tokens.id_token().expect("an id_token");
    let id_verifier = client.id_token_verifier();
    let claims = id_token
        .claims(&id_verifier, &nonce)
        .expect("signature, iss, aud, exp and nonce verify");
    assert_eq!(claims.subject().as_str(), "1000001");
    assert_eq!(
        claims.email().map(|e| e.as_str()),
        Some("student@example.test")
    );
    let at_hash = AccessTokenHash::from_token(
        tokens.access_token(),
        id_token.signing_alg().expect("alg"),
        id_token.signing_key(&id_verifier).expect("key"),
    )
    .expect("hash");
    assert_eq!(claims.access_token_hash(), Some(&at_hash));

    let userinfo: CoreUserInfoClaims = client
        .user_info(
            tokens.access_token().clone(),
            Some(claims.subject().clone()),
        )
        .expect("userinfo endpoint")
        .request_async(&http)
        .await
        .expect("userinfo with the same sub");
    assert_eq!(
        userinfo.email().map(|e| e.as_str()),
        Some("student@example.test")
    );
    let recorded = mock.requests_to(Endpoint::Userinfo);
    assert_eq!(
        recorded[0].header("Authorization"),
        Some(format!("Bearer {}", tokens.access_token().secret()).as_str())
    );
    let issued = mock.issued_tokens();
    assert_eq!(issued.len(), 1);
    assert_eq!(&issued[0].access_token, tokens.access_token().secret());
    assert_eq!(
        Some(&issued[0].refresh_token),
        tokens.refresh_token().map(|t| t.secret())
    );
}

#[tokio::test]
async fn userinfo_needs_a_token_the_oidc_endpoint_issued() {
    let mock = MockIdp::start().await.expect("mock");
    let tokens = common::oidc_tokens(&mock).await;
    let access = tokens["access_token"].as_str().expect("access_token");

    let answer = get(&mock.url(Endpoint::Userinfo), Some(access)).await;
    assert_eq!(answer.status(), 200);
    let body = json(&answer);
    assert_eq!(body["sub"], "mock-user-1");
    assert_eq!(body["given_name"], "Test");

    let unknown = get(&mock.url(Endpoint::Userinfo), Some("not-a-token")).await;
    assert_eq!(unknown.status(), 401);
    assert!(
        unknown.headers()["www-authenticate"]
            .to_str()
            .expect("ascii")
            .starts_with("Bearer")
    );
    assert_eq!(get(&mock.url(Endpoint::Userinfo), None).await.status(), 401);
    let other = get(&mock.url(Endpoint::OAuth2Userinfo), Some(access)).await;
    assert_eq!(other.status(), 401, "an OIDC token is not an OAuth 2 one");
}

#[tokio::test]
async fn a_rotated_key_signs_new_tokens_and_both_keys_stay_published() {
    let mock = MockIdp::start().await.expect("mock");
    let old_kid = mock.signing_kid();
    let old_keys = jwks(&mock).await;

    let new_kid = mock.rotate_signing_key().expect("rotate");
    assert_ne!(new_kid, old_kid);
    assert_eq!(mock.signing_kid(), new_kid);
    let published: Vec<String> = json(&get(&mock.url(Endpoint::Jwks), None).await)["keys"]
        .as_array()
        .expect("keys")
        .iter()
        .map(|k| k["kid"].as_str().expect("kid").to_owned())
        .collect();
    assert_eq!(published, [old_kid.clone(), new_kid.clone()]);

    let tokens = common::oidc_tokens(&mock).await;
    let token = id_token(&tokens);
    let (header, _) = common::decode_jwt(tokens["id_token"].as_str().expect("id_token"));
    assert_eq!(header["kid"], new_kid.as_str());
    let nonce = Nonce::new(common::NONCE.to_owned());
    token
        .claims(&verifier(&mock, jwks(&mock).await), &nonce)
        .expect("verifies with the JWKS fetched after the rotation");
    assert!(
        token.claims(&verifier(&mock, old_keys), &nonce).is_err(),
        "the JWKS cached before the rotation does not have the new key"
    );

    assert!(mock.withdraw_key(&new_kid));
    assert!(!mock.withdraw_key(&new_kid));
    let after = jwks(&mock).await;
    assert!(
        token.claims(&verifier(&mock, after), &nonce).is_err(),
        "a kid the JWKS no longer lists does not verify"
    );
    let still = json(&get(&mock.url(Endpoint::Jwks), None).await);
    assert_eq!(still["keys"][0]["kid"], old_kid.as_str());
    assert_eq!(still["keys"].as_array().map(Vec::len), Some(1));
}

#[tokio::test]
async fn alg_none_and_an_unpublished_key_fail_signature_checks() {
    let mock = MockIdp::start().await.expect("mock");
    let nonce = Nonce::new(common::NONCE.to_owned());

    mock.set_id_token_signing(IdTokenSigning::AlgNone)
        .expect("signing");
    let tokens = common::oidc_tokens(&mock).await;
    let raw = tokens["id_token"].as_str().expect("id_token");
    let (header, _) = common::decode_jwt(raw);
    assert_eq!(header["alg"], "none");
    assert!(raw.ends_with('.'), "an empty signature");
    let keys = verifier(&mock, jwks(&mock).await);
    if let Ok(token) = CoreIdToken::from_str(raw) {
        assert!(token.claims(&keys, &nonce).is_err(), "alg none is refused");
    }

    mock.set_id_token_signing(IdTokenSigning::UnpublishedKey)
        .expect("signing");
    let tokens = common::oidc_tokens(&mock).await;
    let (header, _) = common::decode_jwt(tokens["id_token"].as_str().expect("id_token"));
    assert_eq!(header["alg"], "RS256");
    assert_eq!(header["kid"], "mock-key-unpublished");
    assert!(
        id_token(&tokens)
            .claims(&verifier(&mock, jwks(&mock).await), &nonce)
            .is_err()
    );

    mock.set_id_token_signing(IdTokenSigning::Current)
        .expect("signing");
    let tokens = common::oidc_tokens(&mock).await;
    id_token(&tokens)
        .claims(&verifier(&mock, jwks(&mock).await), &nonce)
        .expect("signed with the published key again");
}
