// SPDX-License-Identifier: AGPL-3.0-or-later
//! The values a test passes to the mock and reads back from it.

use std::time::Duration;

use axum::http::{HeaderValue, StatusCode};

use serde_json::{Map, Value, json};

/// One endpoint of the mock. The OIDC provider's live under the issuer path `/oidc`, the plain
/// OAuth 2 provider's under `/oauth2`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Endpoint {
    /// `GET /oidc/.well-known/openid-configuration`
    Discovery,
    /// `GET /oidc/jwks`
    Jwks,
    /// `GET /oidc/authorize`
    Authorize,
    /// `POST /oidc/token`
    Token,
    /// `GET` or `POST /oidc/userinfo`
    Userinfo,
    /// `GET /oauth2/authorize`
    OAuth2Authorize,
    /// `POST /oauth2/token`
    OAuth2Token,
    /// `GET` or `POST /oauth2/userinfo`
    OAuth2Userinfo,
}

impl Endpoint {
    /// Every endpoint, in the order of the enum.
    pub const ALL: [Self; 8] = [
        Self::Discovery,
        Self::Jwks,
        Self::Authorize,
        Self::Token,
        Self::Userinfo,
        Self::OAuth2Authorize,
        Self::OAuth2Token,
        Self::OAuth2Userinfo,
    ];

    /// The endpoint's path on the mock.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Discovery => "/oidc/.well-known/openid-configuration",
            Self::Jwks => "/oidc/jwks",
            Self::Authorize => "/oidc/authorize",
            Self::Token => "/oidc/token",
            Self::Userinfo => "/oidc/userinfo",
            Self::OAuth2Authorize => "/oauth2/authorize",
            Self::OAuth2Token => "/oauth2/token",
            Self::OAuth2Userinfo => "/oauth2/userinfo",
        }
    }
}

/// How an endpoint misbehaves until the test clears it: wait, then answer with a status, a body,
/// or both instead of the normal answer. A fault with only a delay waits, then answers normally.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fault {
    pub(crate) delay: Option<Duration>,
    pub(crate) status: Option<StatusCode>,
    pub(crate) body: Option<(HeaderValue, Vec<u8>)>,
}

impl Fault {
    /// Answer with `status` and an empty body.
    #[must_use]
    pub fn status(status: StatusCode) -> Self {
        Self::default().with_status(status)
    }

    /// Wait `delay` before answering.
    #[must_use]
    pub fn delay(delay: Duration) -> Self {
        Self::default().with_delay(delay)
    }

    /// Answer 200 (or the fault's status) with this body and `Content-Type`; an error if
    /// `content_type` is not a valid header value.
    pub fn body(content_type: &str, body: impl Into<Vec<u8>>) -> Result<Self, InvalidContentType> {
        Self::default().with_body(content_type, body)
    }

    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = Some(status);
        self
    }

    #[must_use]
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    /// The fault with this body and `Content-Type`; an error if `content_type` is not a valid
    /// header value.
    pub fn with_body(
        mut self,
        content_type: &str,
        body: impl Into<Vec<u8>>,
    ) -> Result<Self, InvalidContentType> {
        let content_type = HeaderValue::from_str(content_type).map_err(|_| InvalidContentType)?;
        self.body = Some((content_type, body.into()));
        Ok(self)
    }
}

/// A fault's `Content-Type` is not a valid header value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the fault's Content-Type is not a valid header value")]
pub struct InvalidContentType;

/// The person the authorize endpoints approve at once, with no login page.
#[derive(Clone, Debug, PartialEq)]
pub struct MockUser {
    /// The ID token's and the OIDC userinfo's `sub`.
    pub subject: String,
    /// Claims beside `sub`, in the ID token and the OIDC userinfo response.
    pub claims: Map<String, Value>,
}

impl MockUser {
    /// A user with `subject` and no other claims.
    #[must_use]
    pub fn new(subject: &str) -> Self {
        Self {
            subject: subject.to_owned(),
            claims: Map::new(),
        }
    }

    /// The user with one more claim.
    #[must_use]
    pub fn with_claim(mut self, name: &str, value: Value) -> Self {
        self.claims.insert(name.to_owned(), value);
        self
    }
}

impl Default for MockUser {
    /// A synthetic user: subject `mock-user-1`, "Test Student", a verified `example.test` email.
    fn default() -> Self {
        Self::new("mock-user-1")
            .with_claim("given_name", json!("Test"))
            .with_claim("family_name", json!("Student"))
            .with_claim("email", json!("test.student@example.test"))
            .with_claim("email_verified", json!(true))
    }
}

/// How a client authenticates at the token endpoints (RFC 6749 2.3.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientAuth {
    /// `client_secret_basic`: HTTP Basic with the form-encoded id and secret.
    Basic,
    /// `client_secret_post`: `client_id` and `client_secret` in the form body.
    Post,
}

impl ClientAuth {
    /// The method's name in a discovery document.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Basic => "client_secret_basic",
            Self::Post => "client_secret_post",
        }
    }
}

/// How the OIDC token endpoint signs the ID tokens it issues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IdTokenSigning {
    /// RS256 with the current published key, its `kid` in the header.
    #[default]
    Current,
    /// `alg: none` and an empty signature (RFC 7519 6.1).
    AlgNone,
    /// RS256 with a key whose `kid` the JWKS never lists.
    UnpublishedKey,
}

/// An error the authorize endpoints redirect with instead of a code (RFC 6749 4.1.2.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorizeError {
    /// The `error` parameter, such as `access_denied` or `server_error`.
    pub error: String,
    /// The `error_description` parameter, if any.
    pub description: Option<String>,
}

/// One request the mock received, recorded before any fault applies.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordedRequest {
    /// The endpoint, or `None` for a path the mock does not serve (answered 404).
    pub endpoint: Option<Endpoint>,
    pub method: String,
    pub path: String,
    /// The decoded query parameters, in order.
    pub query: Vec<(String, String)>,
    /// The headers, names in lowercase, values as received (lossy UTF-8).
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RecordedRequest {
    /// The first query parameter called `name`.
    #[must_use]
    pub fn query_param(&self, name: &str) -> Option<&str> {
        first(&self.query, name)
    }

    /// The first header called `name` (any case).
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        first(&self.headers, &name)
    }

    /// The first `application/x-www-form-urlencoded` body parameter called `name`.
    #[must_use]
    pub fn form_param(&self, name: &str) -> Option<String> {
        url::form_urlencoded::parse(&self.body)
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    }
}

fn first<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

/// The tokens one successful token request issued.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedTokens {
    /// The endpoint that issued them: [`Endpoint::Token`] or [`Endpoint::OAuth2Token`].
    pub endpoint: Endpoint,
    pub access_token: String,
    pub refresh_token: String,
    /// The ID token as sent, when the OIDC endpoint issued one.
    pub id_token: Option<String>,
}
