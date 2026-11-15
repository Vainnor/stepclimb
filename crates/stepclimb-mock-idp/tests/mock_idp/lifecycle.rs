// SPDX-License-Identifier: AGPL-3.0-or-later
//! Dropping the mock stops it: the port closes and open connections are cut.

use std::time::Duration;

use stepclimb_mock_idp::{Endpoint, Fault, MockIdp};

#[tokio::test]
async fn a_kept_alive_connection_fails_after_the_drop() {
    let mock = MockIdp::start().await.expect("mock");
    let client = crate::common::client();
    let url = mock.url(Endpoint::Discovery);
    let request = || {
        http::Request::get(url.clone())
            .body(Vec::new())
            .expect("request")
    };
    let first = client.send(request()).await.expect("the mock answers");
    assert_eq!(first.status(), 200);
    drop(mock);
    assert!(
        client.send(request()).await.is_err(),
        "the same client, whose pooled connection is still open without the cut"
    );
}

#[tokio::test]
async fn a_request_waiting_on_a_delay_ends_with_the_drop() {
    let mock = MockIdp::start().await.expect("mock");
    mock.set_fault(Endpoint::Discovery, Fault::delay(Duration::from_secs(60)));
    let url = mock.url(Endpoint::Discovery);
    let waiting = tokio::spawn(async move { crate::common::get_result(&url).await });
    while mock.requests_to(Endpoint::Discovery).is_empty() {
        tokio::task::yield_now().await;
    }
    drop(mock);
    let outcome = tokio::time::timeout(Duration::from_secs(4), waiting)
        .await
        .expect("the request ends well before its 60 s delay")
        .expect("task");
    assert!(
        outcome.is_err() || outcome.is_ok_and(|status| status == 503),
        "cut, or answered 503"
    );
}

#[tokio::test]
async fn a_base_url_names_the_issuer_the_endpoints_and_the_id_tokens_iss() {
    let any_port = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
    let mock = MockIdp::start_at(any_port, Some("http://mock-idp.test:8080/"))
        .await
        .expect("mock");
    assert_eq!(mock.base_url(), "http://mock-idp.test:8080");
    assert_eq!(mock.issuer(), "http://mock-idp.test:8080/oidc");
    assert_eq!(
        mock.url(Endpoint::Token),
        "http://mock-idp.test:8080/oidc/token"
    );
    // The mock still listens on the address bound, whatever its URLs name.
    let port = mock.addr().port();
    let local = |endpoint: Endpoint| format!("http://127.0.0.1:{port}{}", endpoint.path());
    let document =
        crate::common::json(&crate::common::get(&local(Endpoint::Discovery), None).await);
    assert_eq!(document["issuer"], "http://mock-idp.test:8080/oidc");
    assert_eq!(
        document["authorization_endpoint"],
        "http://mock-idp.test:8080/oidc/authorize"
    );

    let mut url = url::Url::parse(&local(Endpoint::Authorize)).expect("url");
    url.query_pairs_mut()
        .extend_pairs(crate::common::authorize_params(&mock, true));
    let response = crate::common::get(url.as_str(), None).await;
    let location =
        url::Url::parse(response.headers()["location"].to_str().expect("ascii")).expect("location");
    let code = crate::common::param(&location, "code").expect("a code");
    let (id, secret) = (mock.client_id(), mock.client_secret());
    let tokens = crate::common::post_form(
        &local(Endpoint::Token),
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", crate::common::REDIRECT_URI),
            ("code_verifier", crate::common::VERIFIER),
        ],
        Some((&id, &secret)),
    )
    .await;
    assert_eq!(tokens.status(), 200);
    let id_token = crate::common::json(&tokens)["id_token"]
        .as_str()
        .expect("an ID token")
        .to_owned();
    let (_, claims) = crate::common::decode_jwt(&id_token);
    assert_eq!(claims["iss"], "http://mock-idp.test:8080/oidc");
}

#[tokio::test]
async fn a_base_url_that_is_not_an_http_origin_is_refused_before_binding() {
    let any_port = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, 0));
    for bad in [
        "mock-idp:8080",
        "ftp://mock-idp",
        "http://mock-idp/oidc",
        "http://mock-idp/?a=b",
        "http://mock-idp/#top",
        "http://user@mock-idp",
        "not a url",
    ] {
        let started = MockIdp::start_at(any_port, Some(bad)).await;
        assert!(
            matches!(started, Err(stepclimb_mock_idp::StartError::BaseUrl)),
            "{bad}: {started:?}"
        );
    }
}
