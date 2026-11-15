// SPDX-License-Identifier: AGPL-3.0-or-later
//! A mock identity provider for tests (`docs/specs/m0/06-oauth2-and-vatsim-preset.md` B1, B2;
//! `docs/adr/0027-mock-identity-provider.md`). It never ships: crates take it as a
//! dev-dependency only, and `bin/check-layering.sh` refuses it as a normal or build dependency.
//!
//! [`MockIdp::start`] serves two providers on `127.0.0.1` and an ephemeral port, inside the
//! test's tokio runtime. Dropping the [`MockIdp`] closes the port and cuts every open
//! connection.
//!
//! - an OpenID Connect provider with the issuer `http://127.0.0.1:<port>/oidc`: discovery, a
//!   JWKS, authorize, token and userinfo, and RS256 ID tokens signed with 2048-bit keys it
//!   generates at start;
//! - a plain OAuth 2 provider under `/oauth2`: authorize, token, and a userinfo endpoint that
//!   answers whatever JSON the test sets.
//!
//! Both authorize endpoints approve at once as the [`MockUser`] the test selects; there is no
//! login page. One client is registered, [`MockIdp::client_id`] with [`MockIdp::client_secret`].
//! Its secret carries `+`, `/` and `=`, so Basic credentials that skip form-encoding fail
//! ([`MockIdp::set_client_secret`] replaces it). Authorize accepts any absolute `http` or `https`
//! redirect URI unless [`MockIdp::set_redirect_uris`] sets an allowlist of exact strings. Both
//! token endpoints authenticate the client, then spend the code whatever else the request gets
//! wrong, require the authorize request's `redirect_uri` byte for byte, and answer
//! `invalid_grant` when the PKCE verifier does not match the S256 challenge. PKCE is required by
//! default ([`MockIdp::set_require_pkce`]).
//!
//! The test makes the mock misbehave with the setters on [`MockIdp`]; each lasts until it is
//! set back. The faults of spec 05 T3 to T11 map onto them as follows.
//!
//! | Fault | Setter |
//! | --- | --- |
//! | discovery `issuer` differs | `set_discovery_field("issuer", Some(..))` |
//! | wrong `iss`, `aud`, `azp`, `nonce`, `at_hash` | `set_id_token_claim(name, Some(..))` |
//! | two audiences | `set_id_token_claim("aud", Some(json!([a, b])))` |
//! | expired or future token | `set_id_token_claim("exp" or "iat", Some(json!(secs)))`, or [`MockIdp::set_time_source`] |
//! | `alg: none`, an unknown signing key | [`MockIdp::set_id_token_signing`] |
//! | key rotation | [`MockIdp::rotate_signing_key`], [`MockIdp::withdraw_key`] |
//! | `error=access_denied` at authorize | [`MockIdp::set_authorize_error`] |
//! | a status, a body or a delay at any endpoint | [`MockIdp::set_fault`] |
//! | no `id_token`, no `access_token`, `token_type` `mac` | `set_token_response_field(name, ..)` |
//! | userinfo `sub` mismatch, userinfo claims | `set_userinfo_claim(name, ..)` |
//!
//! Every request is recorded before any fault applies ([`MockIdp::requests`]), and every token
//! the token endpoints generate is kept ([`MockIdp::issued_tokens`]).

mod control;
mod jwt;
mod keys;
mod routes;
mod server;
mod state;
mod types;

use std::collections::{BTreeMap, HashMap};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

pub use axum::http::StatusCode;
pub use keys::KeyError;
pub use types::{
    AuthorizeError, ClientAuth, Endpoint, Fault, IdTokenSigning, InvalidContentType, IssuedTokens,
    MockUser, RecordedRequest,
};

use crate::keys::Key;
use crate::server::{CutListener, Stop};
use crate::state::{Keys, Shared, Signing, State};

/// The lifetime of the mock's ID tokens: `exp` is `iat` plus this.
pub const ID_TOKEN_LIFETIME: Duration = Duration::from_secs(300);

/// Why the mock could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error("could not bind 127.0.0.1: {0}")]
    Bind(#[from] std::io::Error),
    #[error(transparent)]
    Key(#[from] KeyError),
    #[error("the OS random number generator failed")]
    Random,
}

/// A running mock identity provider. Dropping it closes the port and cuts every open
/// connection, kept-alive ones and requests waiting on a delay fault included.
pub struct MockIdp {
    shared: Shared,
    addr: SocketAddr,
    server: JoinHandle<()>,
    /// Dropped with the mock, which fires every connection's [`Stop`].
    _stop: watch::Sender<()>,
}

impl std::fmt::Debug for MockIdp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MockIdp")
            .field("addr", &self.addr)
            .finish_non_exhaustive()
    }
}

impl MockIdp {
    /// Start the mock on `127.0.0.1` and a port the OS picks, on the current tokio runtime.
    pub async fn start() -> Result<Self, StartError> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0))).await?;
        let addr = listener.local_addr()?;
        let key = Arc::new(Key::generate("mock-key-1".to_owned())?);
        // `+`, `/` and `=` change under form-encoding, so a client that puts the secret into
        // Basic credentials without form-encoding it first (RFC 6749 2.3.1) fails.
        let client_secret = format!("s+/={}", routes::random_token().ok_or(StartError::Random)?);
        let (stop_sender, stop) = Stop::new();
        let shared = Shared::new(State {
            base_url: format!("http://{addr}"),
            client_id: "stepclimb-test".to_owned(),
            client_secret,
            auth_methods: vec![ClientAuth::Basic, ClientAuth::Post],
            require_pkce: true,
            redirect_uris: None,
            user: MockUser::default(),
            oauth2_userinfo: None,
            authorize_error: None,
            faults: HashMap::new(),
            discovery_overrides: BTreeMap::new(),
            token_overrides: BTreeMap::new(),
            id_token_overrides: BTreeMap::new(),
            userinfo_overrides: BTreeMap::new(),
            signing: Signing::Current,
            keys: Keys {
                published: vec![Arc::clone(&key)],
                current: key,
                generated: 1,
            },
            grants: HashMap::new(),
            access_tokens: HashMap::new(),
            issued: Vec::new(),
            requests: Vec::new(),
            now: Arc::new(SystemTime::now),
        });
        let router = routes::router(shared.clone());
        let listener = CutListener {
            inner: listener,
            stop,
        };
        let server = tokio::spawn(async move {
            // Serving only ends when the runtime shuts down; the test is over by then.
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            shared,
            addr,
            server,
            _stop: stop_sender,
        })
    }

    /// The address the mock listens on.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>`, without a trailing slash.
    #[must_use]
    pub fn base_url(&self) -> String {
        self.shared.lock().base_url.clone()
    }

    /// The OIDC provider's issuer, `http://127.0.0.1:<port>/oidc`; its discovery document is at
    /// `<issuer>/.well-known/openid-configuration`.
    #[must_use]
    pub fn issuer(&self) -> String {
        self.shared.lock().issuer()
    }

    /// The absolute URL of `endpoint`.
    #[must_use]
    pub fn url(&self, endpoint: Endpoint) -> String {
        self.shared.lock().url(endpoint)
    }

    /// The registered client's id, `stepclimb-test`.
    #[must_use]
    pub fn client_id(&self) -> String {
        self.shared.lock().client_id.clone()
    }

    /// The registered client's secret: `s+/=` and 43 random characters, new for each mock.
    #[must_use]
    pub fn client_secret(&self) -> String {
        self.shared.lock().client_secret.clone()
    }
}

impl Drop for MockIdp {
    fn drop(&mut self) {
        self.server.abort();
    }
}
