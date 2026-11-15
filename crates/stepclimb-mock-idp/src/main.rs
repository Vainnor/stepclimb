// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb-mock-idp`: the mock identity provider as a process, for the Compose stack of `just
//! e2e` (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` B3). It is built only into the image of
//! `deploy/Dockerfile`'s `mock-idp` target, which `deploy/compose/ci.yaml` alone starts; the
//! release image never contains it.
//!
//! `stepclimb-mock-idp` serves until SIGTERM or SIGINT, configured by four variables, all
//! required:
//!
//! | Variable | What it is |
//! | --- | --- |
//! | `SC_MOCK_IDP_BIND` | The address to listen on, such as `0.0.0.0:8080` |
//! | `SC_MOCK_IDP_BASE_URL` | The origin the server reaches the mock at, such as `http://mock-idp:8080`; the issuer is it plus `/oidc` |
//! | `SC_MOCK_IDP_BROWSER_URL` | The origin the browser reaches the mock at; the discovery document's `authorization_endpoint` is it plus `/oidc/authorize` |
//! | `SC_MOCK_IDP_CLIENT_SECRET_FILE` | A file whose trimmed contents are the client `stepclimb-test`'s secret |
//!
//! Every authorize request is approved at once as the library's default user, "Test Student"
//! (subject `mock-user-1`). `stepclimb-mock-idp healthcheck` asks the mock on
//! `127.0.0.1:<the port of SC_MOCK_IDP_BIND>` for its discovery document and exits 0 on a 200,
//! for the container's health check: the image has no shell or HTTP client.

use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use stepclimb_mock_idp::{Endpoint, MockIdp};

const BIND: &str = "SC_MOCK_IDP_BIND";
const BASE_URL: &str = "SC_MOCK_IDP_BASE_URL";
const BROWSER_URL: &str = "SC_MOCK_IDP_BROWSER_URL";
const CLIENT_SECRET_FILE: &str = "SC_MOCK_IDP_CLIENT_SECRET_FILE";

/// How long the health check waits to connect, and then for each read or write.
const HEALTHCHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// What the process serves, read from the variables.
#[derive(Debug, PartialEq, Eq)]
struct Config {
    bind: SocketAddr,
    base_url: String,
    browser_url: String,
    client_secret: String,
}

/// The value of the variable `name` from `var`, or an error naming it.
fn required(var: &impl Fn(&str) -> Option<String>, name: &str) -> Result<String, String> {
    var(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{name} is required"))
}

/// The bind address from `var`.
fn bind(var: &impl Fn(&str) -> Option<String>) -> Result<SocketAddr, String> {
    required(var, BIND)?
        .parse()
        .map_err(|_| format!("{BIND} is not an address and port such as 0.0.0.0:8080"))
}

/// The configuration from `var`, reading the secret file with `read`.
fn config(
    var: &impl Fn(&str) -> Option<String>,
    read: impl Fn(&str) -> std::io::Result<String>,
) -> Result<Config, String> {
    let bind = bind(var)?;
    let base_url = required(var, BASE_URL)?;
    let browser_url = required(var, BROWSER_URL)?;
    let secret_file = required(var, CLIENT_SECRET_FILE)?;
    let client_secret = read(&secret_file)
        .map_err(|e| format!("{CLIENT_SECRET_FILE} could not be read: {e}"))?
        .trim()
        .to_owned();
    if client_secret.is_empty() {
        return Err(format!("{CLIENT_SECRET_FILE} names an empty file"));
    }
    Ok(Config {
        bind,
        base_url,
        browser_url: browser_url.trim_end_matches('/').to_owned(),
        client_secret,
    })
}

/// Start the mock as `config` says.
async fn start(config: &Config) -> Result<MockIdp, String> {
    let mock = MockIdp::start_at(config.bind, Some(&config.base_url))
        .await
        .map_err(|e| format!("{BASE_URL} or {BIND}: {e}"))?;
    mock.set_client_secret(&config.client_secret);
    let authorize = format!("{}{}", config.browser_url, Endpoint::Authorize.path());
    mock.set_discovery_field("authorization_endpoint", Some(authorize.into()));
    Ok(mock)
}

/// Until SIGTERM or SIGINT.
async fn stopped() -> Result<(), String> {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).map_err(|e| e.to_string())?;
    tokio::select! {
        _ = term.recv() => Ok(()),
        result = tokio::signal::ctrl_c() => result.map_err(|e| e.to_string()),
    }
}

async fn serve(config: Config) -> Result<(), String> {
    let mock = start(&config).await?;
    eprintln!(
        "stepclimb-mock-idp: listening on {}, issuer {}, authorize at {}",
        mock.addr(),
        mock.issuer(),
        config.browser_url,
    );
    stopped().await?;
    drop(mock);
    Ok(())
}

/// Whether the mock on `port` of `127.0.0.1` answers its discovery document with a 200.
fn healthcheck(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream =
        TcpStream::connect_timeout(&addr, HEALTHCHECK_TIMEOUT).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(HEALTHCHECK_TIMEOUT))
        .and_then(|()| stream.set_write_timeout(Some(HEALTHCHECK_TIMEOUT)))
        .map_err(|e| e.to_string())?;
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n",
        Endpoint::Discovery.path()
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| e.to_string())?;
    let status_line = response.split(|&b| b == b'\r').next().unwrap_or_default();
    if status_line.starts_with(b"HTTP/1.1 200 ") {
        Ok(())
    } else {
        Err(format!(
            "the discovery document answered {:?}",
            String::from_utf8_lossy(status_line)
        ))
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

#[tokio::main]
async fn main() -> ExitCode {
    let result = match std::env::args().nth(1).as_deref() {
        None => match config(&env, |path| std::fs::read_to_string(path)) {
            Ok(config) => serve(config).await,
            Err(e) => Err(e),
        },
        Some("healthcheck") => bind(&env).and_then(|addr| healthcheck(addr.port())),
        Some(other) => Err(format!(
            "unknown argument {other:?}: run with none to serve, or `healthcheck`"
        )),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("stepclimb-mock-idp: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| map.get(name).cloned()
    }

    const ALL: [(&str, &str); 4] = [
        (BIND, "127.0.0.1:0"),
        (BASE_URL, "http://mock-idp:8080"),
        (BROWSER_URL, "http://127.0.0.1:20016/"),
        (CLIENT_SECRET_FILE, "/run/secrets/mock_idp_client_secret"),
    ];

    fn secret(path: &str) -> std::io::Result<String> {
        assert_eq!(path, "/run/secrets/mock_idp_client_secret");
        Ok("e2e+/=secret\n".to_owned())
    }

    #[test]
    fn the_config_reads_every_variable_and_trims_the_secret() {
        let config = config(&vars(&ALL), secret).expect("a config");
        assert_eq!(
            config,
            Config {
                bind: "127.0.0.1:0".parse().expect("an address"),
                base_url: "http://mock-idp:8080".to_owned(),
                browser_url: "http://127.0.0.1:20016".to_owned(),
                client_secret: "e2e+/=secret".to_owned(),
            }
        );
    }

    #[test]
    fn each_missing_or_empty_variable_is_named() {
        for (missing, _) in ALL {
            let without: Vec<_> = ALL.into_iter().filter(|(k, _)| *k != missing).collect();
            let error = config(&vars(&without), secret).expect_err("refused");
            assert_eq!(error, format!("{missing} is required"));
            let mut empty = without.clone();
            empty.push((missing, ""));
            let error = config(&vars(&empty), secret).expect_err("refused");
            assert_eq!(error, format!("{missing} is required"));
        }
    }

    #[test]
    fn a_bad_bind_address_or_an_empty_secret_is_refused() {
        let mut bad = ALL.to_vec();
        bad[0] = (BIND, "mock-idp:8080");
        assert!(
            config(&vars(&bad), secret)
                .expect_err("refused")
                .starts_with(BIND)
        );
        let blank = |_: &str| Ok(" \n".to_owned());
        assert_eq!(
            config(&vars(&ALL), blank).expect_err("refused"),
            format!("{CLIENT_SECRET_FILE} names an empty file")
        );
    }

    #[tokio::test]
    async fn the_mock_names_the_base_url_and_sends_browsers_to_the_browser_url() {
        let config = config(&vars(&ALL), secret).expect("a config");
        let mock = start(&config).await.expect("the mock starts");
        assert_eq!(mock.issuer(), "http://mock-idp:8080/oidc");
        assert_eq!(mock.client_secret(), "e2e+/=secret");
        let port = mock.addr().port();
        let body = tokio::task::spawn_blocking(move || discovery(port))
            .await
            .expect("task");
        let document: serde_json::Value = serde_json::from_str(&body).expect("JSON");
        assert_eq!(document["issuer"], "http://mock-idp:8080/oidc");
        assert_eq!(
            document["authorization_endpoint"],
            "http://127.0.0.1:20016/oidc/authorize"
        );
        assert_eq!(
            document["token_endpoint"],
            "http://mock-idp:8080/oidc/token"
        );
        assert_eq!(document["jwks_uri"], "http://mock-idp:8080/oidc/jwks");
    }

    /// The discovery document's body from the mock on `port`.
    fn discovery(port: u16) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            Endpoint::Discovery.path()
        );
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read");
        let (_, body) = response.split_once("\r\n\r\n").expect("a body");
        body.to_owned()
    }

    #[tokio::test]
    async fn the_healthcheck_passes_on_the_mock_and_fails_on_a_closed_port_or_another_answer() {
        let mock = MockIdp::start().await.expect("the mock starts");
        let port = mock.addr().port();
        let check = move || tokio::task::spawn_blocking(move || healthcheck(port));
        assert_eq!(check().await.expect("task"), Ok(()));

        mock.set_fault(
            Endpoint::Discovery,
            stepclimb_mock_idp::Fault::status(stepclimb_mock_idp::StatusCode::SERVICE_UNAVAILABLE),
        );
        let error = check()
            .await
            .expect("task")
            .expect_err("a 503 is not healthy");
        assert!(error.contains("503"), "{error}");

        drop(mock);
        assert!(check().await.expect("task").is_err(), "the port is closed");
    }
}
