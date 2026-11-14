// SPDX-License-Identifier: AGPL-3.0-or-later
//! The client against a local HTTP server on 127.0.0.1 that answers each connection with bytes
//! the test writes and records what it received.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

/// What the local server does with each connection.
#[derive(Clone)]
enum Answer {
    /// Write these bytes and close.
    Bytes(Vec<u8>),
    /// Wait, then write these bytes.
    Late(Duration, Vec<u8>),
    /// Write these bytes, then hold the connection open without closing it.
    Stall(Vec<u8>),
    /// Write these bytes at once, without reading a request, and close.
    Unasked(Vec<u8>),
}

struct Server {
    port: u16,
    /// The head of every request received, one per connection.
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    async fn start(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().expect("address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let answer = answer.clone();
                let seen = Arc::clone(&seen);
                tokio::spawn(async move {
                    if let Answer::Unasked(bytes) = answer {
                        seen.lock().expect("lock").push(String::new());
                        let _ = stream.write_all(&bytes).await;
                        return;
                    }
                    let head = read_head(&mut stream).await;
                    seen.lock().expect("lock").push(head);
                    match answer {
                        Answer::Bytes(bytes) => {
                            let _ = stream.write_all(&bytes).await;
                        }
                        Answer::Late(delay, bytes) => {
                            tokio::time::sleep(delay).await;
                            let _ = stream.write_all(&bytes).await;
                        }
                        Answer::Stall(bytes) => {
                            let _ = stream.write_all(&bytes).await;
                            tokio::time::sleep(Duration::from_secs(30)).await;
                        }
                        Answer::Unasked(_) => {}
                    }
                });
            }
        });
        Self { port, requests }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("lock").clone()
    }
}

async fn read_head(stream: &mut tokio::net::TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte).await {
            Ok(1) => head.push(byte[0]),
            _ => break,
        }
    }
    String::from_utf8_lossy(&head).into_owned()
}

fn ok(body: &str) -> Answer {
    Answer::Bytes(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .into_bytes(),
    )
}

fn get(url: &str) -> http::Request<Vec<u8>> {
    http::Request::get(url).body(Vec::new()).expect("a request")
}

const INSECURE: UrlPolicy = UrlPolicy::new(true, true);

fn client(policy: UrlPolicy) -> OutboundClient {
    OutboundClient::new(policy, Duration::from_millis(500), 1024).expect("a client")
}

#[tokio::test]
async fn a_request_under_the_policy_returns_status_headers_and_body() {
    let server = Server::start(ok(r#"{"issuer":"x"}"#)).await;
    let url = format!(
        "http://127.0.0.1:{}/.well-known/openid-configuration",
        server.port
    );

    let response = client(INSECURE).send(get(&url)).await.expect("a response");

    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/json",
        "headers are kept"
    );
    assert_eq!(response.body(), br#"{"issuer":"x"}"#);
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /.well-known/openid-configuration HTTP/1.1\r\n"),
        "{requests:?}"
    );
}

#[tokio::test]
async fn http_without_the_insecure_switch_is_refused_before_connecting() {
    let server = Server::start(ok("{}")).await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let error = client(UrlPolicy::new(false, true))
        .send(get(&url))
        .await
        .expect_err("refused");

    assert!(
        matches!(error, OutboundError::Refused(Refused::Scheme)),
        "{error:?}"
    );
    assert!(server.requests().is_empty(), "nothing reached the server");
}

#[tokio::test]
async fn a_loopback_address_in_the_url_is_refused_before_connecting() {
    let server = Server::start(ok("{}")).await;
    let url = format!("https://127.0.0.1:{}/", server.port);

    let error = client(UrlPolicy::new(false, true))
        .send(get(&url))
        .await
        .expect_err("refused");

    assert!(
        matches!(error, OutboundError::Refused(Refused::Loopback)),
        "{error:?}"
    );
    assert!(server.requests().is_empty(), "nothing reached the server");
}

#[tokio::test]
async fn a_name_resolving_to_a_refused_address_is_refused_at_connect_time() {
    let server = Server::start(ok("{}")).await;
    // `localhost` passes the URL check (it is a name) and resolves to loopback.
    let url = format!("https://localhost:{}/", server.port);
    let strict = UrlPolicy::new(false, true);
    assert_eq!(strict.check_url(&Url::parse(&url).expect("a URL")), Ok(()));

    let error = client(strict).send(get(&url)).await.expect_err("refused");

    assert!(
        matches!(error, OutboundError::Refused(Refused::Loopback)),
        "{error:?}"
    );
    assert!(server.requests().is_empty(), "nothing reached the server");
}

#[tokio::test]
async fn a_name_resolving_to_an_allowed_address_connects() {
    let server = Server::start(ok("{}")).await;
    let url = format!("http://localhost:{}/", server.port);

    let response = client(INSECURE).send(get(&url)).await.expect("a response");

    assert_eq!(response.status(), http::StatusCode::OK);
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_resolved_name_is_refused_when_any_address_breaks_the_policy() {
    let strict = UrlPolicy::new(false, false);
    let public: SocketAddr = "93.184.215.14:0".parse().expect("an address");
    let private: SocketAddr = "10.0.0.7:0".parse().expect("an address");

    assert_eq!(checked(strict, vec![public]), Ok(vec![public]));
    assert_eq!(
        checked(strict, vec![public, private]),
        Err(Refused::Private)
    );
    assert_eq!(checked(strict, Vec::new()), Err(Refused::NoAddress));
}

#[tokio::test]
async fn a_redirect_is_returned_not_followed() {
    let server = Server::start(Answer::Bytes(
        b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
    ))
    .await;
    let url = format!("http://127.0.0.1:{}/start", server.port);

    let response = client(INSECURE).send(get(&url)).await.expect("a response");

    assert_eq!(response.status(), http::StatusCode::FOUND);
    assert_eq!(server.requests().len(), 1, "one request, no follow-up");
}

#[tokio::test]
async fn a_slow_answer_times_out() {
    let server = Server::start(Answer::Late(Duration::from_secs(5), Vec::new())).await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let error = client(INSECURE).send(get(&url)).await.expect_err("timeout");

    assert!(
        matches!(error, OutboundError::Timeout(d) if d == Duration::from_millis(500)),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_timeout_covers_reading_the_body() {
    let server = Server::start(Answer::Stall(
        b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc".to_vec(),
    ))
    .await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let error = client(INSECURE).send(get(&url)).await.expect_err("timeout");

    assert!(matches!(error, OutboundError::Timeout(_)), "{error:?}");
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_by_its_length() {
    let server = Server::start(ok(&"x".repeat(1025))).await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let error = client(INSECURE)
        .send(get(&url))
        .await
        .expect_err("too large");

    assert!(matches!(error, OutboundError::TooLarge(1024)), "{error:?}");
}

#[tokio::test]
async fn a_declared_length_over_the_limit_is_refused_before_reading() {
    // The server declares more than the limit and never sends it: only the length check can
    // answer before the timeout.
    let server = Server::start(Answer::Stall(
        b"HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n".to_vec(),
    ))
    .await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let error = client(INSECURE)
        .send(get(&url))
        .await
        .expect_err("too large");

    assert!(matches!(error, OutboundError::TooLarge(1024)), "{error:?}");
}

#[tokio::test]
async fn a_body_over_the_limit_is_refused_while_reading_without_a_length() {
    let chunk = "x".repeat(600);
    let answer = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n258\r\n{chunk}\r\n258\r\n{chunk}\r\n0\r\n\r\n"
    );
    let server = Server::start(Answer::Bytes(answer.into_bytes())).await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let error = client(INSECURE)
        .send(get(&url))
        .await
        .expect_err("too large");

    assert!(matches!(error, OutboundError::TooLarge(1024)), "{error:?}");
}

#[tokio::test]
async fn a_body_at_the_limit_is_read() {
    let server = Server::start(ok(&"x".repeat(1024))).await;
    let url = format!("http://127.0.0.1:{}/", server.port);

    let response = client(INSECURE).send(get(&url)).await.expect("a response");

    assert_eq!(response.body().len(), 1024);
}

#[tokio::test]
async fn a_tls_failure_is_a_transport_error_without_the_url() {
    // A plain-HTTP reply to the TLS handshake: rustls refuses it.
    let server = Server::start(Answer::Unasked(
        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n".to_vec(),
    ))
    .await;
    let url = format!(
        "https://127.0.0.1:{}/token?client_secret=do-not-log-me",
        server.port
    );

    let error = client(INSECURE)
        .send(get(&url))
        .await
        .expect_err("TLS fails");

    let OutboundError::Transport { reason } = &error else {
        panic!("{error:?}");
    };
    let shown = format!("{error} {error:?}");
    assert!(!reason.is_empty());
    assert!(!shown.contains("do-not-log-me"), "{shown}");
    assert!(!shown.contains("/token"), "{shown}");
}

#[tokio::test]
async fn an_unreachable_port_is_a_transport_error() {
    // Bind and drop to find a port nothing listens on.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind")
        .local_addr()
        .expect("address")
        .port();
    let url = format!("http://127.0.0.1:{port}/");

    let error = client(INSECURE).send(get(&url)).await.expect_err("refused");

    assert!(
        matches!(error, OutboundError::Transport { .. }),
        "{error:?}"
    );
}

#[test]
fn the_provider_client_has_the_documented_limits() {
    let client = OutboundClient::for_providers(INSECURE).expect("a client");
    assert_eq!(client.timeout, Duration::from_secs(5));
    assert_eq!(client.max_body, 1024 * 1024);
    assert_eq!(client.policy(), INSECURE);
}

/// Set in the child process of `proxy_variables_are_ignored`, naming the port of the server the
/// child requests directly.
const PROXY_CHILD: &str = "STEPCLIMB_TEST_PROXY_CHILD";

/// The process environment cannot be changed without `unsafe`, so the test runs itself again with
/// `HTTP_PROXY` and `HTTPS_PROXY` naming a listener of its own. The child makes one request; the
/// listener must see no connection, and the request must reach the server directly.
#[test]
fn proxy_variables_are_ignored() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    if let Ok(port) = std::env::var(PROXY_CHILD) {
        let url = format!("http://127.0.0.1:{port}/");
        let response = runtime
            .block_on(client(INSECURE).send(get(&url)))
            .expect("the request reaches the server directly");
        assert_eq!(response.status(), http::StatusCode::OK);
        return;
    }
    let proxy = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    proxy.set_nonblocking(true).expect("nonblocking");
    let proxy_url = format!("http://{}", proxy.local_addr().expect("address"));
    let server = runtime.block_on(Server::start(ok("{}")));

    let output = std::process::Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "outbound::tests::proxy_variables_are_ignored",
            "--nocapture",
        ])
        .env(PROXY_CHILD, server.port.to_string())
        .env("HTTP_PROXY", &proxy_url)
        .env("HTTPS_PROXY", &proxy_url)
        .env("ALL_PROXY", &proxy_url)
        .env("http_proxy", &proxy_url)
        .env("https_proxy", &proxy_url)
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .output()
        .expect("the child runs");

    assert!(
        matches!(proxy.accept(), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock),
        "the proxy saw a connection"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "child failed: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("1 passed"),
        "the child ran the test: {stdout}"
    );
    assert_eq!(server.requests().len(), 1, "the server saw the request");
}
