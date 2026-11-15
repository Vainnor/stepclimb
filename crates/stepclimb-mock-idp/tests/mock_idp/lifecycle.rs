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
