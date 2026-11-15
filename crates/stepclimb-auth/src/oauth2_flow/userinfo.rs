// SPDX-License-Identifier: AGPL-3.0-or-later
//! The userinfo request of a generic OAuth 2 provider (spec 06 B9; RFC 6750 2.1).

use http::{HeaderValue, header};
use serde_json::{Map, Value};

use crate::outbound::{OutboundClient, OutboundError};

/// Why the userinfo answer was not used (B9). None of these carries a URL, a body or a token.
#[derive(Debug, thiserror::Error)]
pub enum UserinfoError {
    /// The request was refused by the URL policy, timed out, was too large or failed.
    #[error("the userinfo request failed: {0}")]
    Fetch(#[from] OutboundError),
    /// The endpoint answered a status outside 2xx.
    #[error("the userinfo endpoint answered HTTP {0}")]
    Status(u16),
    /// The body is not a JSON object.
    #[error("the userinfo response is not a JSON object")]
    NotObject,
}

/// `GET` the `userinfo` URL with `Authorization: Bearer <access_token>` and
/// `Accept: application/json`, and return its body, which must be a JSON object.
pub async fn fetch_userinfo(
    client: &OutboundClient,
    userinfo_url: &str,
    access_token: &str,
) -> Result<Map<String, Value>, UserinfoError> {
    let mut bearer = HeaderValue::from_str(&format!("Bearer {access_token}"))
        .map_err(|_| OutboundError::Request)?;
    bearer.set_sensitive(true);
    let request = http::Request::get(userinfo_url)
        .header(header::ACCEPT, "application/json")
        .header(header::AUTHORIZATION, bearer)
        .body(Vec::new())
        .map_err(|_| OutboundError::Request)?;
    let response = client.send(request).await?;
    if !response.status().is_success() {
        return Err(UserinfoError::Status(response.status().as_u16()));
    }
    match serde_json::from_slice(response.body()) {
        Ok(Value::Object(body)) => Ok(body),
        _ => Err(UserinfoError::NotObject),
    }
}
