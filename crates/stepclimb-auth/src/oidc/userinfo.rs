// SPDX-License-Identifier: AGPL-3.0-or-later
//! The userinfo request and the merge of its claims (`docs/specs/m0/05-oidc.md` B11, Q3; OIDC
//! Core 5.3).

use http::{HeaderValue, StatusCode, header};
use serde_json::{Map, Value};
use url::Url;

use super::discovery::is_json;
use crate::outbound::{OutboundClient, OutboundError};

/// The claims that always come from the ID token, whatever userinfo says (B11).
const ID_TOKEN_ONLY: [&str; 3] = ["sub", "iss", "aud"];

/// Why the userinfo answer was not used (B11). None of these carries a URL, a body or a token.
#[derive(Debug, thiserror::Error)]
pub enum UserinfoError {
    /// The request was refused by the URL policy, timed out, was too large or failed.
    #[error("the userinfo request failed: {0}")]
    Fetch(#[from] OutboundError),
    /// The endpoint answered a status other than 200.
    #[error("the userinfo endpoint answered HTTP {0}")]
    Status(u16),
    /// The response is not `application/json`: a signed or encrypted userinfo response
    /// (`application/jwt`) is not read.
    #[error("the userinfo response is not application/json")]
    ContentType,
    /// The body is not a JSON object.
    #[error("the userinfo response is not a JSON object")]
    Parse,
    /// The response's `sub` is missing or not the ID token's (OIDC Core 5.3.2).
    #[error("the userinfo sub is not the ID token's")]
    SubjectMismatch,
}

/// `GET` the userinfo endpoint with the access token as a Bearer credential (RFC 6750 2.1) and
/// return its claims when its `sub` is `subject`.
pub async fn fetch_userinfo(
    client: &OutboundClient,
    endpoint: &Url,
    access_token: &str,
    subject: &str,
) -> Result<Map<String, Value>, UserinfoError> {
    let mut bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|_| OutboundError::Request)?;
    bearer.set_sensitive(true);
    let request = http::Request::get(endpoint.as_str())
        .header(header::ACCEPT, "application/json")
        .header(header::AUTHORIZATION, bearer)
        .body(Vec::new())
        .map_err(|_| OutboundError::Request)?;
    let response = client.send(request).await?;
    if response.status() != StatusCode::OK {
        return Err(UserinfoError::Status(response.status().as_u16()));
    }
    if !is_json(response.headers()) {
        return Err(UserinfoError::ContentType);
    }
    let Ok(Value::Object(claims)) = serde_json::from_slice(response.body()) else {
        return Err(UserinfoError::Parse);
    };
    match claims.get("sub") {
        Some(Value::String(sub)) if sub == subject => Ok(claims),
        _ => Err(UserinfoError::SubjectMismatch),
    }
}

/// The ID token's claims with userinfo's laid over them, except `sub`, `iss` and `aud`, which
/// stay the ID token's (B11).
#[must_use]
pub fn merge_claims(
    mut id_token: Map<String, Value>,
    userinfo: Map<String, Value>,
) -> Map<String, Value> {
    for (name, value) in userinfo {
        if !ID_TOKEN_ONLY.contains(&name.as_str()) {
            id_token.insert(name, value);
        }
    }
    id_token
}
