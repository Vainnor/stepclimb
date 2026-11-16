// SPDX-License-Identifier: AGPL-3.0-or-later
//! The HTTP client for every request the server makes to a URL an admin entered: identity
//! provider discovery, keys, tokens and user info, and the admission webhook
//! (`docs/specs/m0/05-oidc.md` B2, Q4; `07-admission.md` B14, B16, B17).
//!
//! Every [`OutboundClient`] request fails closed:
//!
//! - the URL must pass the [`UrlPolicy`] (scheme, no credentials, an address written as the host)
//!   before anything is sent;
//! - a host name is resolved by the client's own resolver, which refuses the whole name when any
//!   address it resolves to breaks the policy, and the connection goes only to the addresses it
//!   checked, so a name cannot pass the check and then connect elsewhere;
//! - redirects are never followed: a 3xx is returned to the caller as it came, so a redirect
//!   cannot lead the request to an address the policy refuses;
//! - no proxy is used, whatever `HTTP_PROXY` or `HTTPS_PROXY` say, since a proxy would resolve
//!   the name where the policy cannot see it;
//! - the whole exchange, from the start of the connection to the last byte of the body, has one
//!   timeout ([`PROVIDER_TIMEOUT`] for providers, [`ADMISSION_WEBHOOK_TIMEOUT`] for the
//!   admission webhook);
//! - a body longer than the client's limit ([`PROVIDER_MAX_BODY`] for providers,
//!   [`ADMISSION_WEBHOOK_MAX_BODY`] for the admission webhook) is an error,
//!   checked against `Content-Length` and again while reading.
//!
//! TLS is rustls with ring, verified against the operating system's trust store, HTTP/1.1 only.
//! Errors never carry the URL, a header or a body: a URL can hold a secret.

mod policy;

use std::error::Error as StdError;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use rustls_platform_verifier::BuilderVerifierExt as _;
use url::Url;

pub use policy::{AddressClass, Refused, UrlPolicy, classify};

/// The timeout of one request to an identity provider (`05-oidc.md` Q4).
pub const PROVIDER_TIMEOUT: Duration = Duration::from_secs(5);

/// The largest response body accepted from an identity provider: 1 MiB, far above any discovery
/// document, key set or token response.
pub const PROVIDER_MAX_BODY: usize = 1024 * 1024;

/// The timeout of one admission webhook call, from the start of the connection to the end of
/// the response body (`07-admission.md` B14).
pub const ADMISSION_WEBHOOK_TIMEOUT: Duration = Duration::from_secs(3);

/// The largest admission webhook reply accepted: 64 KiB (`07-admission.md` B14, Q7).
pub const ADMISSION_WEBHOOK_MAX_BODY: usize = 64 * 1024;

/// A client for outbound requests under one policy, timeout and body limit. Cloning is cheap;
/// clones share the connection pool.
#[derive(Clone)]
pub struct OutboundClient {
    http: reqwest::Client,
    policy: UrlPolicy,
    timeout: Duration,
    max_body: usize,
}

impl std::fmt::Debug for OutboundClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboundClient")
            .field("policy", &self.policy)
            .field("timeout", &self.timeout)
            .field("max_body", &self.max_body)
            .finish_non_exhaustive()
    }
}

/// Why the client could not be built: the system's trust store or the TLS setup failed.
#[derive(Debug, thiserror::Error)]
#[error("could not build the outbound HTTP client: {reason}")]
pub struct BuildError {
    reason: String,
}

/// Why an outbound request failed. None of these carries the URL, a header or a body.
#[derive(Debug, thiserror::Error)]
pub enum OutboundError {
    /// The URL or an address it resolves to breaks the policy; nothing was sent to it.
    #[error("the outbound URL policy refused the request: {0}")]
    Refused(Refused),
    /// The request did not finish, body included, within the client's timeout.
    #[error("the request did not finish within {0:?}")]
    Timeout(Duration),
    /// The response body is longer than the client's limit.
    #[error("the response body is longer than {0} bytes")]
    TooLarge(usize),
    /// The request could not be built from what the caller passed.
    #[error("the request is not one the client can send")]
    Request,
    /// The connection, TLS or HTTP exchange failed.
    #[error("the request failed: {reason}")]
    Transport { reason: String },
}

impl OutboundClient {
    /// A client for identity provider requests: [`PROVIDER_TIMEOUT`] and [`PROVIDER_MAX_BODY`].
    pub fn for_providers(policy: UrlPolicy) -> Result<Self, BuildError> {
        Self::new(policy, PROVIDER_TIMEOUT, PROVIDER_MAX_BODY)
    }

    /// A client for admission webhook calls: [`ADMISSION_WEBHOOK_TIMEOUT`] and
    /// [`ADMISSION_WEBHOOK_MAX_BODY`].
    pub fn for_admission_webhook(policy: UrlPolicy) -> Result<Self, BuildError> {
        Self::new(
            policy,
            ADMISSION_WEBHOOK_TIMEOUT,
            ADMISSION_WEBHOOK_MAX_BODY,
        )
    }

    /// A client with its own timeout and body limit.
    pub fn new(policy: UrlPolicy, timeout: Duration, max_body: usize) -> Result<Self, BuildError> {
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls_config()?)
            .dns_resolver(Arc::new(PolicyResolver { policy }))
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .referer(false)
            .build()
            .map_err(|e| BuildError {
                reason: error_chain(&e),
            })?;
        Ok(Self {
            http,
            policy,
            timeout,
            max_body,
        })
    }

    /// The policy this client enforces.
    #[must_use]
    pub fn policy(&self) -> UrlPolicy {
        self.policy
    }

    /// Send `request` and read its whole response, under the policy, the timeout and the body
    /// limit. Any status, 3xx included, is a response; the caller decides what it accepts.
    pub async fn send(
        &self,
        request: http::Request<Vec<u8>>,
    ) -> Result<http::Response<Vec<u8>>, OutboundError> {
        let url = Url::parse(&request.uri().to_string())
            .map_err(|_| OutboundError::Refused(Refused::NotAbsolute))?;
        self.policy
            .check_url(&url)
            .map_err(OutboundError::Refused)?;
        let request = reqwest::Request::try_from(request).map_err(|_| OutboundError::Request)?;
        match tokio::time::timeout(self.timeout, self.exchange(request)).await {
            Ok(result) => result,
            Err(_) => Err(OutboundError::Timeout(self.timeout)),
        }
    }

    async fn exchange(
        &self,
        request: reqwest::Request,
    ) -> Result<http::Response<Vec<u8>>, OutboundError> {
        let mut response = self.http.execute(request).await.map_err(transport_error)?;
        let too_large = OutboundError::TooLarge(self.max_body);
        if response
            .content_length()
            .is_some_and(|length| length > self.max_body as u64)
        {
            return Err(too_large);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(transport_error)? {
            if body.len() + chunk.len() > self.max_body {
                return Err(too_large);
            }
            body.extend_from_slice(&chunk);
        }
        let mut answer = http::Response::new(body);
        *answer.status_mut() = response.status();
        *answer.version_mut() = response.version();
        *answer.headers_mut() = std::mem::take(response.headers_mut());
        Ok(answer)
    }
}

/// rustls with ring and the platform verifier, HTTP/1.1 by ALPN.
fn tls_config() -> Result<rustls::ClientConfig, BuildError> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|builder| builder.with_platform_verifier())
        .map_err(|e| BuildError {
            reason: e.to_string(),
        })?
        .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(config)
}

/// A refusal from the resolver, found in the error's source chain, or a transport error whose
/// reason is the chain's messages (`without_url` keeps the URL out of reqwest's own).
fn transport_error(error: reqwest::Error) -> OutboundError {
    let mut source: Option<&(dyn StdError + 'static)> = Some(&error);
    while let Some(current) = source {
        if let Some(refused) = current.downcast_ref::<Refused>() {
            return OutboundError::Refused(*refused);
        }
        source = current.source();
    }
    OutboundError::Transport {
        reason: error_chain(&error.without_url()),
    }
}

fn error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut reason = error.to_string();
    let mut source = error.source();
    while let Some(current) = source {
        reason.push_str(": ");
        reason.push_str(&current.to_string());
        source = current.source();
    }
    reason
}

/// Resolves a host name with the system resolver and refuses it when any address breaks the
/// policy, so the connector only ever sees checked addresses.
struct PolicyResolver {
    policy: UrlPolicy,
}

impl Resolve for PolicyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy;
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            checked(policy, addrs)
                .map(|addrs| Box::new(addrs.into_iter()) as Addrs)
                .map_err(Into::into)
        })
    }
}

/// `addrs` when every one passes the policy; none at all is refused too.
fn checked(policy: UrlPolicy, addrs: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, Refused> {
    if addrs.is_empty() {
        return Err(Refused::NoAddress);
    }
    for addr in &addrs {
        policy.check_address(addr.ip())?;
    }
    Ok(addrs)
}

#[cfg(test)]
mod tests;
