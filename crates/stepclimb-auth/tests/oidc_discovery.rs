// SPDX-License-Identifier: AGPL-3.0-or-later
//! OIDC discovery and its cache against the mock IdP on 127.0.0.1 (docs/specs/m0/05-oidc.md
//! B4, Q5, T3).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use stepclimb_auth::oidc::{DISCOVERY_TTL, DiscoveryCache, DiscoveryError, discover};
use stepclimb_auth::outbound::{OutboundClient, OutboundError, Refused, UrlPolicy};
use stepclimb_mock_idp::{Endpoint, Fault, MockIdp, StatusCode};
use uuid::Uuid;

/// The insecure switch on, as the test setup sets it (Q4): the mock listens on `http` loopback.
fn client() -> OutboundClient {
    OutboundClient::for_providers(UrlPolicy::new(true, false)).expect("a client")
}

fn t0() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_763_000_000)
}

fn discoveries(mock: &MockIdp) -> usize {
    mock.requests_to(Endpoint::Discovery).len()
}

#[tokio::test]
async fn the_mocks_document_is_accepted_for_its_exact_issuer() {
    let mock = MockIdp::start().await.unwrap();
    let metadata = discover(&client(), &mock.issuer()).await.unwrap();
    assert_eq!(metadata.issuer().as_str(), mock.issuer());
    assert_eq!(
        metadata.authorization_endpoint().url().as_str(),
        mock.url(Endpoint::Authorize)
    );
    let request = &mock.requests_to(Endpoint::Discovery)[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.header("accept"), Some("application/json"));
}

#[tokio::test]
async fn t3_an_issuer_differing_by_a_trailing_slash_is_refused() {
    let mock = MockIdp::start().await.unwrap();
    // The configured issuer has the slash; the document's does not.
    let configured = format!("{}/", mock.issuer());
    let result = discover(&client(), &configured).await;
    assert!(
        matches!(result, Err(DiscoveryError::IssuerMismatch)),
        "{result:?}"
    );
    assert_eq!(
        discoveries(&mock),
        1,
        "the document was fetched and refused"
    );

    // And the other way round: the document has the slash, the configuration does not.
    mock.set_discovery_field("issuer", Some(json!(configured)));
    let result = discover(&client(), &mock.issuer()).await;
    assert!(
        matches!(result, Err(DiscoveryError::IssuerMismatch)),
        "{result:?}"
    );
}

#[tokio::test]
async fn t3_a_discovery_endpoint_answering_500_is_refused() {
    let mock = MockIdp::start().await.unwrap();
    mock.set_fault(
        Endpoint::Discovery,
        Fault::status(StatusCode::INTERNAL_SERVER_ERROR),
    );
    let result = discover(&client(), &mock.issuer()).await;
    assert!(
        matches!(result, Err(DiscoveryError::Status(500))),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let mock = MockIdp::start().await.unwrap();
    mock.set_fault(Endpoint::Discovery, Fault::status(StatusCode::FOUND));
    let result = discover(&client(), &mock.issuer()).await;
    assert!(
        matches!(result, Err(DiscoveryError::Status(302))),
        "{result:?}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_json_or_lacks_a_member_is_refused() {
    let mock = MockIdp::start().await.unwrap();
    mock.set_fault(
        Endpoint::Discovery,
        Fault::body("text/html", "<html>sign in</html>").unwrap(),
    );
    let result = discover(&client(), &mock.issuer()).await;
    assert!(
        matches!(result, Err(DiscoveryError::ContentType)),
        "{result:?}"
    );

    mock.clear_fault(Endpoint::Discovery);
    mock.set_discovery_field("authorization_endpoint", None);
    let result = discover(&client(), &mock.issuer()).await;
    let Err(DiscoveryError::Parse(reason)) = result else {
        panic!("expected a parse error, got {result:?}");
    };
    assert!(!reason.contains("127.0.0.1"), "{reason}");
}

#[tokio::test]
async fn an_authorization_endpoint_with_another_scheme_is_refused() {
    let mock = MockIdp::start().await.unwrap();
    mock.set_discovery_field("authorization_endpoint", Some(json!("javascript:alert(1)")));
    let result = discover(&client(), &mock.issuer()).await;
    assert!(
        matches!(result, Err(DiscoveryError::AuthorizationEndpoint)),
        "{result:?}"
    );
}

#[tokio::test]
async fn with_the_switch_off_an_http_issuer_is_refused_before_any_request() {
    let mock = MockIdp::start().await.unwrap();
    let strict = OutboundClient::for_providers(UrlPolicy::new(false, true)).unwrap();
    let result = discover(&strict, &mock.issuer()).await;
    assert!(
        matches!(
            result,
            Err(DiscoveryError::Fetch(OutboundError::Refused(
                Refused::Scheme
            )))
        ),
        "{result:?}"
    );
    assert_eq!(discoveries(&mock), 0);
}

#[tokio::test]
async fn q5_a_document_is_kept_for_an_hour_per_provider_and_issuer() {
    let mock = MockIdp::start().await.unwrap();
    let cache = DiscoveryCache::default();
    let client = client();
    let provider = Uuid::now_v7();
    let issuer = mock.issuer();

    cache.get(&client, provider, &issuer, t0()).await.unwrap();
    let just_before = t0() + DISCOVERY_TTL - Duration::from_secs(1);
    cache
        .get(&client, provider, &issuer, just_before)
        .await
        .unwrap();
    assert_eq!(discoveries(&mock), 1, "a fresh document is reused");

    cache
        .get(&client, Uuid::now_v7(), &issuer, t0())
        .await
        .unwrap();
    assert_eq!(discoveries(&mock), 2, "another provider has its own");

    cache
        .get(&client, provider, &issuer, t0() + DISCOVERY_TTL)
        .await
        .unwrap();
    assert_eq!(discoveries(&mock), 3, "an hour old is fetched again");

    let back_in_time = t0() - Duration::from_secs(1);
    cache
        .get(&client, provider, &issuer, back_in_time)
        .await
        .unwrap();
    assert_eq!(
        discoveries(&mock),
        4,
        "a clock that went back fetches again"
    );
}

#[tokio::test]
async fn a_changed_issuer_is_fetched_again_and_a_failure_is_not_kept() {
    let mock = MockIdp::start().await.unwrap();
    let cache = DiscoveryCache::default();
    let client = client();
    let provider = Uuid::now_v7();

    cache
        .get(&client, provider, &mock.issuer(), t0())
        .await
        .unwrap();
    let changed = format!("{}/", mock.issuer());
    let result = cache.get(&client, provider, &changed, t0()).await;
    assert!(matches!(result, Err(DiscoveryError::IssuerMismatch)));
    assert_eq!(
        discoveries(&mock),
        2,
        "the cached document was for the old issuer"
    );

    let result = cache.get(&client, provider, &changed, t0()).await;
    assert!(matches!(result, Err(DiscoveryError::IssuerMismatch)));
    assert_eq!(discoveries(&mock), 3, "a refused document is not kept");
}
