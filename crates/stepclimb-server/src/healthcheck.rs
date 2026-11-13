// SPDX-License-Identifier: AGPL-3.0-or-later
//! `stepclimb healthcheck`: ask the running server's `/readyz` and report whether it answered
//! 200, so a container without a shell or `curl` can have a health check (ADR 0023).
//!
//! It speaks just enough HTTP/1.1 over a plain TCP connection to read the status line, so the
//! binary needs no HTTP client.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// How long the check waits to connect, and then for each read and write. Longer than
/// `/readyz`'s own database wait (`health::READY_TIMEOUT`), so a slow database reads as 503
/// rather than as a timeout here.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

/// The most of the response the check reads looking for the status line.
const MAX_STATUS_LINE: usize = 1024;

/// Why the server is not ready, or could not be asked.
#[derive(Debug, thiserror::Error)]
pub enum HealthcheckError {
    #[error(
        "the server listens on port 0 (a port the system picks), so the check cannot know \
         where to connect; set STEPCLIMB_BIND to a fixed port"
    )]
    NoFixedPort,
    #[error("could not connect to {addr}: {source}")]
    Connect {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("could not ask {addr} for /readyz: {source}")]
    Exchange {
        addr: SocketAddr,
        source: std::io::Error,
    },
    #[error("{addr} did not answer with an HTTP status line")]
    NotHttp { addr: SocketAddr },
    #[error("/readyz on {addr} answered {status}, not 200")]
    NotReady { addr: SocketAddr, status: u16 },
}

/// The address to connect to for a server bound to `bind`: the loopback address of the same
/// family when the server listens on every interface, else `bind` itself.
pub fn target(bind: SocketAddr) -> Result<SocketAddr, HealthcheckError> {
    if bind.port() == 0 {
        return Err(HealthcheckError::NoFixedPort);
    }
    let ip = match bind.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };
    Ok(SocketAddr::new(ip, bind.port()))
}

/// Request `/readyz` from the server bound to `bind`; `Ok` when it answers 200.
pub fn check(bind: SocketAddr) -> Result<(), HealthcheckError> {
    let addr = target(bind)?;
    let mut stream = TcpStream::connect_timeout(&addr, CHECK_TIMEOUT)
        .map_err(|source| HealthcheckError::Connect { addr, source })?;
    let exchange = |source| HealthcheckError::Exchange { addr, source };
    stream
        .set_read_timeout(Some(CHECK_TIMEOUT))
        .map_err(exchange)?;
    stream
        .set_write_timeout(Some(CHECK_TIMEOUT))
        .map_err(exchange)?;
    let request = format!("GET /readyz HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(exchange)?;
    let line = read_status_line(&mut stream).map_err(exchange)?;
    match status_code(&line) {
        Some(200) => Ok(()),
        Some(status) => Err(HealthcheckError::NotReady { addr, status }),
        None => Err(HealthcheckError::NotHttp { addr }),
    }
}

/// The response up to the end of its first line, or what arrived before the connection closed.
fn read_status_line(stream: &mut impl Read) -> std::io::Result<String> {
    let mut received = Vec::new();
    let mut buffer = [0_u8; 256];
    while !received.windows(2).any(|pair| pair == b"\r\n") && received.len() < MAX_STATUS_LINE {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        received.extend_from_slice(&buffer[..read]);
    }
    let text = String::from_utf8_lossy(&received);
    Ok(text.split("\r\n").next().unwrap_or_default().to_owned())
}

/// The status code of an HTTP/1.x status line such as `HTTP/1.1 503 Service Unavailable`.
fn status_code(line: &str) -> Option<u16> {
    let mut parts = line.split(' ');
    let version = parts.next()?;
    if !version.starts_with("HTTP/1.") {
        return None;
    }
    let code = parts.next()?;
    if code.len() != 3 {
        return None;
    }
    code.parse().ok()
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::thread;

    use super::*;

    /// A one-connection server on a free loopback port that answers `response` and returns the
    /// request it read.
    fn answering(response: &'static str) -> (SocketAddr, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a free port");
        let addr = listener.local_addr().expect("local address");
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 512];
            while !request.ends_with(b"\r\n\r\n") {
                let read = stream.read(&mut buffer).expect("read the request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
            }
            stream
                .write_all(response.as_bytes())
                .expect("write the response");
            String::from_utf8_lossy(&request).into_owned()
        });
        (addr, server)
    }

    #[test]
    fn a_200_from_readyz_passes() {
        let (addr, server) = answering("HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
        check(addr).expect("ready");
        let request = server.join().expect("server thread");
        assert!(
            request.starts_with("GET /readyz HTTP/1.1\r\n"),
            "request: {request}"
        );
    }

    #[test]
    fn a_503_from_readyz_fails_naming_the_status() {
        let (addr, server) =
            answering("HTTP/1.1 503 Service Unavailable\r\ncontent-length: 0\r\n\r\n");
        let error = check(addr).expect_err("not ready");
        server.join().expect("server thread");
        assert!(
            matches!(error, HealthcheckError::NotReady { status: 503, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn an_answer_that_is_not_http_fails() {
        let (addr, server) = answering("SSH-2.0-OpenSSH_9.9\r\n");
        let error = check(addr).expect_err("not HTTP");
        server.join().expect("server thread");
        assert!(
            matches!(error, HealthcheckError::NotHttp { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn nothing_listening_fails_to_connect() {
        let addr = {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind a free port");
            listener.local_addr().expect("local address")
        };
        let error = check(addr).expect_err("nothing listens");
        assert!(
            matches!(error, HealthcheckError::Connect { .. }),
            "{error:?}"
        );
    }

    #[test]
    fn every_interface_is_checked_on_loopback_of_the_same_family() {
        let v4: SocketAddr = "0.0.0.0:8080".parse().expect("address");
        let v6: SocketAddr = "[::]:8080".parse().expect("address");
        let specific: SocketAddr = "10.1.2.3:9000".parse().expect("address");
        assert_eq!(
            target(v4).expect("target"),
            "127.0.0.1:8080".parse().expect("address")
        );
        assert_eq!(
            target(v6).expect("target"),
            "[::1]:8080".parse().expect("address")
        );
        assert_eq!(target(specific).expect("target"), specific);
    }

    #[test]
    fn port_0_is_refused() {
        let bind: SocketAddr = "0.0.0.0:0".parse().expect("address");
        assert!(matches!(target(bind), Err(HealthcheckError::NoFixedPort)));
    }

    #[test]
    fn status_lines_are_read_strictly() {
        assert_eq!(status_code("HTTP/1.1 200 OK"), Some(200));
        assert_eq!(status_code("HTTP/1.0 503 Service Unavailable"), Some(503));
        assert_eq!(status_code("HTTP/2 200"), None);
        assert_eq!(status_code("HTTP/1.1 2000 OK"), None);
        assert_eq!(status_code(""), None);
    }
}
