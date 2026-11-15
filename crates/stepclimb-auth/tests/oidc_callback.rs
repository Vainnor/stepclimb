// SPDX-License-Identifier: AGPL-3.0-or-later
//! The callback's code exchange, ID token checks, key set and userinfo against the mock IdP on
//! 127.0.0.1 (docs/specs/m0/05-oidc.md B9 to B11, Q3, Q5, Q6, Q8, T8 to T11).
//!
//! Each test runs one authorization at the mock with the flow secrets it holds, then redeems the
//! code with [`redeem_code`] at a fixed time that the mock also issues its tokens at.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::CoreJwsSigningAlgorithm;
use serde_json::json;
use stepclimb_auth::claims;
use stepclimb_auth::oidc::{
    CallbackError, ClientAuth as UsedAuth, CodeCallback, ExchangeError, Expected, FlowSecrets,
    IdTokenError, JWKS_REFETCH_INTERVAL, JWKS_TTL, JwksCache, KeySet, ProviderMetadata,
    ProviderSignIn, UserinfoError, VerifiedIdToken, accepted_algorithms, authorization_url,
    client_auth, discover, fetch_jwks, verify_id_token,
};
use stepclimb_auth::oidc_config::{self, ClientSecret, DraftSecret, OidcSettings};
use stepclimb_auth::outbound::{OutboundClient, OutboundError, UrlPolicy};
use stepclimb_mock_idp::{
    ClientAuth, Endpoint, Fault, IdTokenSigning, MockIdp, MockUser, StatusCode,
};
use url::Url;
use uuid::Uuid;

const REDIRECT_URI: &str = "https://lms.example.org/auth/mock/callback";

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).expect("test setup").as_secs()
}

/// The insecure switch on, as the test setup sets it (Q4): the mock listens on `http` loopback.
fn client() -> OutboundClient {
    OutboundClient::for_providers(UrlPolicy::new(true, false)).expect("a client")
}

/// One mock and what the relying party keeps between sign-ins.
struct Rp {
    mock: MockIdp,
    client: OutboundClient,
    keys: JwksCache,
    provider_id: Uuid,
    settings: OidcSettings,
    secret: ClientSecret,
}

impl Rp {
    async fn new() -> Self {
        let mock = MockIdp::start().await.expect("the mock starts");
        mock.set_time_source(t0);
        let config = json!({
            "issuer": mock.issuer(),
            "client_id": mock.client_id(),
            "client_secret": mock.client_secret(),
        });
        let draft = oidc_config::validate(&config, UrlPolicy::new(true, true), None)
            .expect("a valid config");
        let DraftSecret::New(secret) = draft.secret else {
            panic!("a new secret");
        };
        Self {
            mock,
            client: client(),
            keys: JwksCache::default(),
            provider_id: Uuid::now_v7(),
            settings: draft.settings,
            secret,
        }
    }

    async fn metadata(&self) -> ProviderMetadata {
        discover(&self.client, &self.mock.issuer())
            .await
            .expect("the mock's discovery document")
    }

    /// Authorize at the mock with `secrets` and redeem the code at `now`.
    async fn sign_in_at(
        &self,
        secrets: &FlowSecrets,
        now: SystemTime,
    ) -> Result<ProviderSignIn, CallbackError> {
        let metadata = self.metadata().await;
        let code = self.authorize(&metadata, secrets).await;
        self.redeem(&metadata, secrets, &code, now).await
    }

    async fn sign_in(&self) -> Result<ProviderSignIn, CallbackError> {
        self.sign_in_at(&FlowSecrets::generate().expect("test setup"), t0())
            .await
    }

    async fn authorize(&self, metadata: &ProviderMetadata, secrets: &FlowSecrets) -> String {
        let url =
            authorization_url(metadata, &self.settings, REDIRECT_URI, secrets).expect("test setup");
        let request = http::Request::get(url.as_str())
            .body(Vec::new())
            .expect("test setup");
        let response = self.client.send(request).await.expect("the mock answers");
        assert_eq!(response.status(), StatusCode::FOUND, "the mock approves");
        let back = Url::parse(response.headers()["location"].to_str().expect("test setup"))
            .expect("test setup");
        back.query_pairs()
            .find(|(name, _)| name == "code")
            .map(|(_, code)| code.into_owned())
            .expect("a code")
    }

    async fn redeem(
        &self,
        metadata: &ProviderMetadata,
        secrets: &FlowSecrets,
        code: &str,
        now: SystemTime,
    ) -> Result<ProviderSignIn, CallbackError> {
        self.redeem_with(&self.client, metadata, secrets, code, now)
            .await
    }

    async fn redeem_with(
        &self,
        client: &OutboundClient,
        metadata: &ProviderMetadata,
        secrets: &FlowSecrets,
        code: &str,
        now: SystemTime,
    ) -> Result<ProviderSignIn, CallbackError> {
        let callback = CodeCallback {
            client,
            keys: &self.keys,
            provider_id: self.provider_id,
            metadata,
            settings: &self.settings,
            client_secret: &self.secret,
            redirect_uri: REDIRECT_URI,
            secrets,
            code,
            now,
        };
        stepclimb_auth::oidc::redeem_code(&callback).await
    }

    fn jwks_fetches(&self) -> usize {
        self.mock.requests_to(Endpoint::Jwks).len()
    }
}

fn id_token_error(result: Result<ProviderSignIn, CallbackError>) -> IdTokenError {
    match result {
        Err(CallbackError::IdToken(error)) => error,
        other => panic!("expected an ID token refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_good_sign_in_yields_the_subject_and_claims_and_sends_what_b9_lists() {
    let rp = Rp::new().await;
    let secrets = FlowSecrets::generate().unwrap();

    let sign_in = rp.sign_in_at(&secrets, t0()).await.expect("a sign-in");

    assert_eq!(sign_in.subject, "mock-user-1");
    assert_eq!(sign_in.claims["email"], "test.student@example.test");
    assert_eq!(sign_in.claims["given_name"], "Test");
    assert_eq!(sign_in.claims["iss"], rp.mock.issuer().as_str());
    let token = &rp.mock.requests_to(Endpoint::Token)[0];
    assert_eq!(token.method, "POST");
    assert_eq!(
        token.form_param("grant_type").as_deref(),
        Some("authorization_code")
    );
    assert_eq!(
        token.form_param("redirect_uri").as_deref(),
        Some(REDIRECT_URI)
    );
    assert_eq!(
        token.form_param("code_verifier").as_deref(),
        Some(secrets.code_verifier())
    );
    assert!(
        token.header("authorization").unwrap().starts_with("Basic "),
        "Q6: client_secret_basic by default"
    );
    assert_eq!(token.form_param("client_secret"), None);
    let issued = &rp.mock.issued_tokens()[0];
    let userinfo = &rp.mock.requests_to(Endpoint::Userinfo)[0];
    assert_eq!(
        userinfo.header("authorization"),
        Some(format!("Bearer {}", issued.access_token).as_str())
    );
    let printed = format!("{sign_in:?}");
    assert!(!printed.contains("test.student"), "{printed}");
}

#[tokio::test]
async fn q6_client_secret_post_when_the_provider_lists_only_it() {
    let rp = Rp::new().await;
    rp.mock.set_token_auth_methods(&[ClientAuth::Post]);
    assert_eq!(client_auth(&rp.metadata().await), UsedAuth::Post);

    rp.sign_in()
        .await
        .expect("a sign-in with client_secret_post");

    let token = &rp.mock.requests_to(Endpoint::Token)[0];
    assert_eq!(token.header("authorization"), None);
    assert_eq!(token.form_param("client_id"), Some(rp.mock.client_id()));
    assert_eq!(
        token.form_param("client_secret"),
        Some(rp.mock.client_secret())
    );

    // Both listed, or none: Basic.
    rp.mock
        .set_token_auth_methods(&[ClientAuth::Post, ClientAuth::Basic]);
    assert_eq!(client_auth(&rp.metadata().await), UsedAuth::Basic);
    rp.mock
        .set_discovery_field("token_endpoint_auth_methods_supported", None);
    assert_eq!(client_auth(&rp.metadata().await), UsedAuth::Basic);
}

#[tokio::test]
async fn t8_a_failed_token_request_is_an_exchange_error() {
    let rp = Rp::new().await;
    rp.mock.set_fault(
        Endpoint::Token,
        Fault::status(StatusCode::INTERNAL_SERVER_ERROR),
    );
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Exchange(ExchangeError::Status(500)))
        ),
        "{result:?}"
    );
    rp.mock.clear_fault(Endpoint::Token);

    rp.mock.set_token_response_field("id_token", None);
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Exchange(ExchangeError::NoIdToken))
        ),
        "{result:?}"
    );
    rp.mock.clear_overrides();

    rp.mock
        .set_token_response_field("token_type", Some(json!("mac")));
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Exchange(ExchangeError::TokenType))
        ),
        "{result:?}"
    );
    rp.mock.clear_overrides();

    rp.mock.set_fault(
        Endpoint::Token,
        Fault::body("application/json", "{not json").unwrap(),
    );
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Exchange(ExchangeError::Parse(_)))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn t8_a_token_endpoint_slower_than_the_timeout_is_an_exchange_error() {
    let rp = Rp::new().await;
    let metadata = rp.metadata().await;
    let secrets = FlowSecrets::generate().unwrap();
    let code = rp.authorize(&metadata, &secrets).await;
    rp.mock
        .set_fault(Endpoint::Token, Fault::delay(Duration::from_secs(5)));
    // The provider client's 5 s, shortened so the test does not wait for it.
    let impatient = OutboundClient::new(
        UrlPolicy::new(true, false),
        Duration::from_millis(200),
        1024 * 1024,
    )
    .unwrap();

    let result = rp
        .redeem_with(&impatient, &metadata, &secrets, &code, t0())
        .await;

    assert!(
        matches!(
            result,
            Err(CallbackError::Exchange(ExchangeError::Fetch(
                OutboundError::Timeout(_)
            )))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn t9_a_key_not_in_the_key_set_is_refused() {
    let rp = Rp::new().await;
    rp.mock
        .set_id_token_signing(IdTokenSigning::UnpublishedKey)
        .unwrap();
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::UnknownKey);
    assert_eq!(
        rp.jwks_fetches(),
        1,
        "the set was just fetched, so the unknown kid does not refetch it"
    );
}

#[tokio::test]
async fn t9_alg_none_is_refused() {
    let rp = Rp::new().await;
    rp.mock
        .set_id_token_signing(IdTokenSigning::AlgNone)
        .unwrap();
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Unsigned);
}

#[tokio::test]
async fn q8_an_algorithm_the_discovery_document_does_not_list_is_refused() {
    let rp = Rp::new().await;
    // The mock signs RS256; the document now lists only ES256.
    rp.mock.set_discovery_field(
        "id_token_signing_alg_values_supported",
        Some(json!(["ES256"])),
    );
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Algorithm);
    // HS256 listed is still not accepted.
    rp.mock.set_discovery_field(
        "id_token_signing_alg_values_supported",
        Some(json!(["HS256"])),
    );
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Algorithm);
    // ES512 listed is not accepted either: openidconnect does not verify it.
    rp.mock.set_discovery_field(
        "id_token_signing_alg_values_supported",
        Some(json!(["ES512", "RS256"])),
    );
    let metadata = rp.metadata().await;
    assert_eq!(
        accepted_algorithms(&metadata),
        [CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256]
    );
    let issued = Issued::new(&rp).await;
    let es512 = issued.with_header(&json!({ "alg": "ES512", "kid": rp.mock.signing_kid() }));
    assert_eq!(
        issued.verify_with(&es512, &accepted_algorithms(&metadata), &issued.keys),
        Err(IdTokenError::Algorithm)
    );
}

#[tokio::test]
async fn t9_another_issuer_is_refused() {
    let rp = Rp::new().await;
    let trailing = format!("{}/", rp.mock.issuer());
    for iss in [
        json!("https://other.example/oidc"),
        json!(trailing),
        json!(1),
    ] {
        rp.mock.set_id_token_claim("iss", Some(iss));
        assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Issuer);
    }
    rp.mock.set_id_token_claim("iss", None);
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Issuer);
}

#[tokio::test]
async fn t9_an_audience_without_the_client_id_is_refused() {
    let rp = Rp::new().await;
    for aud in [
        Some(json!("another-client")),
        Some(json!(["another-client", "third"])),
        Some(json!([])),
        Some(json!(7)),
        None,
    ] {
        rp.mock.set_id_token_claim("aud", aud);
        assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Audience);
    }
}

#[tokio::test]
async fn t9_azp_must_be_the_client_id_when_present_or_with_two_audiences() {
    let rp = Rp::new().await;
    let me = rp.mock.client_id();
    rp.mock
        .set_id_token_claim("aud", Some(json!([me, "another-client"])));
    rp.mock
        .set_id_token_claim("azp", Some(json!("another-client")));
    assert_eq!(
        id_token_error(rp.sign_in().await),
        IdTokenError::AuthorizedParty
    );
    rp.mock.set_id_token_claim("azp", None);
    assert_eq!(
        id_token_error(rp.sign_in().await),
        IdTokenError::AuthorizedParty,
        "two audiences need an azp"
    );
    rp.mock.set_id_token_claim("azp", Some(json!(me)));
    rp.sign_in()
        .await
        .expect("two audiences with azp = client id");

    rp.mock.set_id_token_claim("aud", Some(json!(me)));
    rp.mock
        .set_id_token_claim("azp", Some(json!("another-client")));
    assert_eq!(
        id_token_error(rp.sign_in().await),
        IdTokenError::AuthorizedParty,
        "one audience with another client's azp"
    );
}

#[tokio::test]
async fn t9_exp_in_the_past_by_more_than_the_skew_is_refused() {
    let rp = Rp::new().await;
    let now = secs(t0());
    rp.mock.set_id_token_claim("exp", Some(json!(now - 60)));
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Expired);
    rp.mock.set_id_token_claim("exp", Some(json!(now - 59)));
    rp.sign_in().await.expect("expired within the skew");
    rp.mock.set_id_token_claim("exp", Some(json!("soon")));
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Malformed);
}

#[tokio::test]
async fn t9_iat_in_the_future_by_more_than_the_skew_is_refused() {
    let rp = Rp::new().await;
    let now = secs(t0());
    rp.mock.set_id_token_claim("exp", Some(json!(now + 600)));
    rp.mock.set_id_token_claim("iat", Some(json!(now + 61)));
    assert_eq!(
        id_token_error(rp.sign_in().await),
        IdTokenError::IssuedInFuture
    );
    rp.mock.set_id_token_claim("iat", Some(json!(now + 60)));
    rp.sign_in().await.expect("issued within the skew");
    rp.mock.set_id_token_claim("iat", None);
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Malformed);
}

#[tokio::test]
async fn an_nbf_in_the_future_by_more_than_the_skew_is_refused() {
    let rp = Rp::new().await;
    let now = secs(t0());
    rp.mock.set_id_token_claim("nbf", Some(json!(now + 61)));
    assert_eq!(
        id_token_error(rp.sign_in().await),
        IdTokenError::NotYetValid
    );
    rp.mock.set_id_token_claim("nbf", Some(json!(now + 60)));
    rp.sign_in().await.expect("valid from within the skew");
}

#[tokio::test]
async fn the_server_clock_decides_expiry() {
    let rp = Rp::new().await;
    let lifetime = stepclimb_mock_idp::ID_TOKEN_LIFETIME;
    let secrets = FlowSecrets::generate().unwrap();
    let late = t0() + lifetime + Duration::from_secs(60);
    assert_eq!(
        id_token_error(rp.sign_in_at(&secrets, late).await),
        IdTokenError::Expired
    );
    let early = t0() - Duration::from_secs(61);
    assert_eq!(
        id_token_error(rp.sign_in_at(&secrets, early).await),
        IdTokenError::IssuedInFuture
    );
}

#[tokio::test]
async fn t9_another_nonce_is_refused() {
    let rp = Rp::new().await;
    rp.mock.set_id_token_claim("nonce", Some(json!("another")));
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Nonce);
    rp.mock.set_id_token_claim("nonce", None);
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Nonce);
}

#[tokio::test]
async fn t9_a_wrong_at_hash_is_refused_and_an_absent_one_is_not_required() {
    let rp = Rp::new().await;
    rp.mock
        .set_id_token_claim("at_hash", Some(json!("AAAAAAAAAAAAAAAAAAAAAA")));
    assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::AtHash);
    rp.mock.set_id_token_claim("at_hash", None);
    rp.sign_in().await.expect("no at_hash is accepted");
}

#[tokio::test]
async fn a_subject_no_identity_can_hold_is_refused() {
    let rp = Rp::new().await;
    let longest = "\u{1F600}".repeat(claims::MAX_SUBJECT_CHARS);
    rp.mock.set_user(MockUser::new(&longest));
    let sign_in = rp.sign_in().await.expect("255 characters fit");
    assert_eq!(sign_in.subject, longest);
    let too_long = format!("{longest}x");
    for sub in [
        Some(json!("")),
        Some(json!("a\u{0}b")),
        Some(json!(too_long)),
        Some(json!(7)),
        None,
    ] {
        rp.mock.set_id_token_claim("sub", sub);
        assert_eq!(id_token_error(rp.sign_in().await), IdTokenError::Subject);
    }
}

#[tokio::test]
async fn b13_a_profile_claim_of_another_type_does_not_refuse_the_token() {
    let rp = Rp::new().await;
    rp.mock.set_user(
        MockUser::new("mock-user-2")
            .with_claim("given_name", json!(7))
            .with_claim("email_verified", json!("true")),
    );
    let sign_in = rp.sign_in().await.expect("the token is accepted");
    assert_eq!(sign_in.claims["given_name"], 7);
    assert_eq!(sign_in.claims["email_verified"], "true");
}

#[tokio::test]
async fn t10_a_rotated_key_is_found_by_one_refetch_after_the_interval() {
    let rp = Rp::new().await;
    rp.sign_in().await.expect("the first key");
    assert_eq!(rp.jwks_fetches(), 1);
    rp.sign_in().await.expect("the kept set");
    assert_eq!(rp.jwks_fetches(), 1, "the set is kept");

    rp.mock.rotate_signing_key().unwrap();
    let secrets = FlowSecrets::generate().unwrap();
    assert_eq!(
        id_token_error(rp.sign_in_at(&secrets, t0()).await),
        IdTokenError::UnknownKey,
        "within the refetch interval nothing is fetched"
    );
    assert_eq!(rp.jwks_fetches(), 1);

    let later = t0() + JWKS_REFETCH_INTERVAL;
    rp.mock.set_time_source(move || later);
    rp.sign_in_at(&secrets, later)
        .await
        .expect("the new kid after one refetch");
    assert_eq!(rp.jwks_fetches(), 2);

    // A new signing key withdrawn before the server saw it: the refetch does not find it, and
    // the sign-in fails.
    let unseen = rp.mock.rotate_signing_key().unwrap();
    assert!(rp.mock.withdraw_key(&unseen));
    let much_later = later + JWKS_REFETCH_INTERVAL;
    rp.mock.set_time_source(move || much_later);
    assert_eq!(
        id_token_error(rp.sign_in_at(&secrets, much_later).await),
        IdTokenError::UnknownKey
    );
    assert_eq!(rp.jwks_fetches(), 3, "one refetch, then the failure");
}

#[tokio::test]
async fn q5_the_key_set_is_fetched_again_after_an_hour() {
    let rp = Rp::new().await;
    rp.sign_in().await.unwrap();
    let almost = t0() + JWKS_TTL - Duration::from_secs(1);
    let secrets = FlowSecrets::generate().unwrap();
    rp.mock.set_time_source(move || almost);
    rp.sign_in_at(&secrets, almost).await.unwrap();
    assert_eq!(rp.jwks_fetches(), 1);
    let hour = t0() + JWKS_TTL;
    rp.mock.set_time_source(move || hour);
    rp.sign_in_at(&secrets, hour).await.unwrap();
    assert_eq!(rp.jwks_fetches(), 2);
}

#[tokio::test]
async fn a_key_set_that_cannot_be_read_is_a_keys_error() {
    let rp = Rp::new().await;
    rp.mock.set_fault(
        Endpoint::Jwks,
        Fault::status(StatusCode::INTERNAL_SERVER_ERROR),
    );
    let result = rp.sign_in().await;
    assert!(matches!(result, Err(CallbackError::Keys(_))), "{result:?}");
}

#[tokio::test]
async fn t11_a_userinfo_sub_of_someone_else_is_refused() {
    let rp = Rp::new().await;
    rp.mock
        .set_userinfo_claim("sub", Some(json!("someone-else")));
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Userinfo(UserinfoError::SubjectMismatch))
        ),
        "{result:?}"
    );
    rp.mock.set_userinfo_claim("sub", None);
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Userinfo(UserinfoError::SubjectMismatch))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn t11_userinfo_claims_override_the_id_tokens_except_sub_iss_and_aud() {
    let rp = Rp::new().await;
    rp.mock
        .set_userinfo_claim("email", Some(json!("other@example.test")));
    rp.mock
        .set_userinfo_claim("iss", Some(json!("https://other.example")));
    rp.mock
        .set_userinfo_claim("aud", Some(json!("another-client")));
    rp.mock.set_userinfo_claim("groups", Some(json!(["staff"])));

    let sign_in = rp.sign_in().await.unwrap();

    assert_eq!(sign_in.claims["email"], "other@example.test");
    assert_eq!(sign_in.claims["groups"], json!(["staff"]));
    assert_eq!(sign_in.claims["iss"], rp.mock.issuer().as_str());
    assert_eq!(sign_in.claims["aud"], rp.mock.client_id().as_str());
    assert_eq!(sign_in.claims["sub"], "mock-user-1");
}

#[tokio::test]
async fn q3_userinfo_is_called_only_when_the_flag_is_on_and_the_endpoint_exists() {
    let mut rp = Rp::new().await;
    rp.mock
        .set_userinfo_claim("email", Some(json!("other@example.test")));
    rp.settings.userinfo = false;
    let sign_in = rp.sign_in().await.unwrap();
    assert_eq!(sign_in.claims["email"], "test.student@example.test");
    assert!(rp.mock.requests_to(Endpoint::Userinfo).is_empty());

    rp.settings.userinfo = true;
    rp.mock.set_discovery_field("userinfo_endpoint", None);
    rp.sign_in().await.expect("no endpoint, no call");
    assert!(rp.mock.requests_to(Endpoint::Userinfo).is_empty());
}

#[tokio::test]
async fn a_failing_or_signed_userinfo_answer_is_refused() {
    let rp = Rp::new().await;
    rp.mock.set_fault(
        Endpoint::Userinfo,
        Fault::status(StatusCode::INTERNAL_SERVER_ERROR),
    );
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Userinfo(UserinfoError::Status(500)))
        ),
        "{result:?}"
    );
    rp.mock.set_fault(
        Endpoint::Userinfo,
        Fault::body("application/jwt", "e30.e30.").unwrap(),
    );
    let result = rp.sign_in().await;
    assert!(
        matches!(
            result,
            Err(CallbackError::Userinfo(UserinfoError::ContentType))
        ),
        "{result:?}"
    );
}

#[tokio::test]
async fn no_error_or_debug_output_carries_a_token_code_or_secret() {
    let rp = Rp::new().await;
    let metadata = rp.metadata().await;
    let secrets = FlowSecrets::generate().unwrap();
    let code = rp.authorize(&metadata, &secrets).await;
    rp.mock
        .set_userinfo_claim("sub", Some(json!("someone-else")));
    let error = rp
        .redeem(&metadata, &secrets, &code, t0())
        .await
        .unwrap_err();
    let issued = rp.mock.issued_tokens();
    let tokens = &issued[0];
    let callback = CodeCallback {
        client: &rp.client,
        keys: &rp.keys,
        provider_id: rp.provider_id,
        metadata: &metadata,
        settings: &rp.settings,
        client_secret: &rp.secret,
        redirect_uri: REDIRECT_URI,
        secrets: &secrets,
        code: &code,
        now: t0(),
    };
    let printed = format!("{error} {error:?} {callback:?}");
    let id_token = tokens.id_token.clone().unwrap();
    let client_secret = rp.mock.client_secret();
    for secret in [
        tokens.access_token.as_str(),
        tokens.refresh_token.as_str(),
        id_token.as_str(),
        code.as_str(),
        secrets.nonce(),
        secrets.code_verifier(),
        client_secret.as_str(),
    ] {
        assert!(!printed.contains(secret), "a secret reached {printed}");
    }
}

/// A token the mock issued, its access token, the mock's key set and what to expect of it, for
/// the checks a mock fault cannot reach: the token is altered after it is issued.
struct Issued {
    id_token: String,
    access_token: String,
    nonce: String,
    issuer: String,
    client_id: String,
    keys: KeySet,
}

impl Issued {
    async fn new(rp: &Rp) -> Self {
        let secrets = FlowSecrets::generate().expect("test setup");
        rp.sign_in_at(&secrets, t0()).await.expect("test setup");
        let tokens = rp.mock.issued_tokens().remove(0);
        let jwks = Url::parse(&rp.mock.url(Endpoint::Jwks)).expect("test setup");
        Self {
            id_token: tokens.id_token.expect("test setup"),
            access_token: tokens.access_token,
            nonce: secrets.nonce().to_owned(),
            issuer: rp.mock.issuer(),
            client_id: rp.mock.client_id(),
            keys: fetch_jwks(&rp.client, &jwks).await.expect("test setup"),
        }
    }

    fn verify(&self, token: &str) -> Result<VerifiedIdToken, IdTokenError> {
        let algorithms = [CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256];
        self.verify_with(token, &algorithms, &self.keys)
    }

    fn verify_with(
        &self,
        token: &str,
        algorithms: &[CoreJwsSigningAlgorithm],
        keys: &KeySet,
    ) -> Result<VerifiedIdToken, IdTokenError> {
        let expected = Expected {
            issuer: &self.issuer,
            client_id: &self.client_id,
            nonce: &self.nonce,
            access_token: &self.access_token,
            algorithms,
        };
        verify_id_token(token, keys, &expected, t0())
    }

    /// The token with its header replaced by `header`, payload and signature kept.
    fn with_header(&self, header: &serde_json::Value) -> String {
        let rest = self.id_token.split_once('.').expect("test setup").1;
        format!("{}.{rest}", URL_SAFE_NO_PAD.encode(header.to_string()))
    }
}

#[tokio::test]
async fn a_token_altered_after_signing_is_refused() {
    let rp = Rp::new().await;
    let issued = Issued::new(&rp).await;
    issued
        .verify(&issued.id_token)
        .expect("the token as issued");

    let [header, payload, signature]: [&str; 3] = issued
        .id_token
        .split('.')
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let mut claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap();
    claims["sub"] = json!("someone-else");
    let forged = format!(
        "{header}.{}.{signature}",
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    assert_eq!(issued.verify(&forged), Err(IdTokenError::Signature));
    assert_eq!(
        issued.verify(&format!("{header}.{payload}.")),
        Err(IdTokenError::Signature),
        "an empty signature"
    );
    let kid = rp.mock.signing_kid();
    // Without a kid the one RSA key is used, and the changed header breaks the signature.
    assert_eq!(
        issued.verify(&issued.with_header(&json!({ "alg": "RS256" }))),
        Err(IdTokenError::Signature)
    );
    assert_eq!(
        issued.verify(&issued.with_header(&json!({ "alg": "RS256", "kid": "nobody" }))),
        Err(IdTokenError::UnknownKey)
    );
    assert_eq!(
        issued.verify(&issued.with_header(&json!({ "alg": "HS256", "kid": kid }))),
        Err(IdTokenError::Algorithm)
    );
    assert_eq!(
        issued.verify(&issued.with_header(&json!({ "alg": "none" }))),
        Err(IdTokenError::Unsigned)
    );
}

#[tokio::test]
async fn a_header_the_server_does_not_read_is_malformed() {
    let rp = Rp::new().await;
    let issued = Issued::new(&rp).await;
    let kid = rp.mock.signing_kid();
    for header in [
        json!({ "alg": "RS256", "kid": kid, "typ": "at+jwt" }),
        json!({ "alg": "RS256", "kid": kid, "crit": ["exp"] }),
        json!({ "alg": "RS256", "kid": kid, "cty": "JWT" }),
        json!({ "alg": "RS256", "kid": kid, "enc": "A256GCM" }),
        json!({ "alg": "RS256", "kid": 7 }),
        json!({ "kid": kid }),
        json!(["RS256"]),
    ] {
        assert_eq!(
            issued.verify(&issued.with_header(&header)),
            Err(IdTokenError::Malformed),
            "{header}"
        );
    }
    let (header, rest) = issued.id_token.split_once('.').unwrap();
    for token in [
        String::new(),
        format!("{header}.{rest}.extra"),
        format!("{header}={rest}"),
        format!("{header}.e30"),
        format!("{header}.WzFd.{}", "AA"),
    ] {
        assert_eq!(
            issued.verify(&token),
            Err(IdTokenError::Malformed),
            "{token}"
        );
    }
}

#[tokio::test]
async fn two_keys_that_fit_a_token_without_a_kid_are_ambiguous() {
    let rp = Rp::new().await;
    rp.mock.rotate_signing_key().unwrap();
    let issued = Issued::new(&rp).await;
    assert_eq!(issued.keys.keys().len(), 2);
    assert_eq!(
        issued.verify(&issued.with_header(&json!({ "alg": "RS256" }))),
        Err(IdTokenError::AmbiguousKey)
    );
}

/// The mock's key set with the signing key's JSON changed by `change`.
fn with_signing_key(issued: &Issued, kid: &str, change: impl Fn(&mut serde_json::Value)) -> KeySet {
    let mut set = serde_json::to_value(&issued.keys).expect("a key set serializes");
    for key in set["keys"].as_array_mut().expect("keys") {
        if key["kid"] == kid {
            change(key);
        }
    }
    serde_json::from_value(set).expect("a key set")
}

#[tokio::test]
async fn a_kid_naming_a_key_of_another_type_or_use_is_no_key() {
    let rp = Rp::new().await;
    let issued = Issued::new(&rp).await;
    let kid = rp.mock.signing_kid();
    let rs256 = [CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256];

    // The kid names an EC key (RFC 7515 A.3's public key): an RS256 signature cannot use it.
    let ec = with_signing_key(&issued, &kid, |key| {
        *key = json!({
            "kty": "EC",
            "crv": "P-256",
            "kid": kid,
            "x": "f83OJ3D2xF1Bg8vub9tLe1gHMzV76e8Tus9uPHvRVEU",
            "y": "x_FEzRu9m36HLN_tue659LNpXW6pCyStikYjKIWI5a0",
        });
    });
    assert_eq!(
        issued.verify_with(&issued.id_token, &rs256, &ec),
        Err(IdTokenError::UnknownKey)
    );

    // The kid names the right RSA key, published for encryption only.
    let enc = with_signing_key(&issued, &kid, |key| key["use"] = json!("enc"));
    assert_eq!(
        issued.verify_with(&issued.id_token, &rs256, &enc),
        Err(IdTokenError::UnknownKey)
    );

    // The same key for signatures verifies.
    let sig = with_signing_key(&issued, &kid, |key| key["use"] = json!("sig"));
    issued
        .verify_with(&issued.id_token, &rs256, &sig)
        .expect("a signing key");
}
