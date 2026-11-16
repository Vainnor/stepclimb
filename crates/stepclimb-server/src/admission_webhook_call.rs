// SPDX-License-Identifier: AGPL-3.0-or-later
//! Step 4 of admission: the signed call to the organization's admission webhook
//! (docs/specs/m0/07-admission.md B13 to B17, B19, Q3, Q7).
//!
//! [`decide`] reads the organization's webhook in the sign-in's transaction. With none, or one
//! that is not enabled, the step is skipped. Otherwise it POSTs
//! `{"provider", "subject", "attributes"}` as JSON to the stored URL, signed as Standard
//! Webhooks says (`stepclimb_auth::webhook`): a new `webhook-id`, the request clock's time in
//! `webhook-timestamp`, and `webhook-signature` keyed with the webhook's secret. The call
//! checks no plan feature: an organization that loses `admission_webhook` keeps the gate it
//! configured (Permissions).
//!
//! The call goes through the admission webhook client of the instance's tenancy as it is at the
//! call, counted again in the transaction (`crate::providers`, B17): the URL and every address
//! its host resolves to must pass the outbound URL policy then, so a host name that now
//! resolves to a loopback or private address, or a private address saved while the instance had
//! one organization, is refused once that no longer holds. The client never follows a
//! redirect, uses no proxy, and allows 3 seconds from the start of the connection to the end of
//! a reply of at most 64 KiB (B14, B16).
//!
//! Everything but a well-formed reply fails closed ([`Decision::Unavailable`], which admission
//! answers with `auth.admission_unavailable`): a secret that does not open (another key ring,
//! another row's context), a URL the policy refuses, a timeout, a connection or TLS error, a
//! status other than 200 (a 3xx included), a body over the limit or not JSON, `allow` missing
//! or not a boolean, `groups` not an array of strings, or `attributes` not an object. The
//! secret's opened bytes are wiped as soon as the request is signed. No reason carries the URL,
//! a header or the body.

use std::time::{SystemTime, UNIX_EPOCH};

use axum::http::{self, HeaderValue, Method, StatusCode, header};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::PgConnection;
use stepclimb_auth::{
    encryption::KeyRing,
    outbound::{OutboundClient, OutboundError},
    webhook::{ID_HEADER, SIGNATURE_HEADER, Secret, TIMESTAMP_HEADER},
};
use stepclimb_core::{
    OrgScope,
    admission_webhooks::{self, AdmissionWebhook},
    id::new_id,
    organizations,
};
use url::Url;
use utoipa::ToSchema;

use crate::{admission_webhook::secret_context, error::ApiError, providers::Providers};

/// The JSON body Stepclimb POSTs to an organization's admission webhook at each sign-in through
/// an outside identity provider. It is not a Stepclimb endpoint. The request carries the
/// Standard Webhooks headers `webhook-id`, `webhook-timestamp` (Unix seconds) and
/// `webhook-signature` (`v1,` and the base64 HMAC-SHA256 of `id.timestamp.body`, keyed with the
/// webhook's `whsec_` secret); verify them with a Standard Webhooks library. Answer within 3
/// seconds with status 200 and an `AdmissionWebhookReply`.
#[derive(Serialize, ToSchema)]
#[schema(as = AdmissionWebhookCall)]
pub struct Call<'a> {
    /// The key of the identity provider the person signed in with.
    pub provider: &'a str,
    /// The provider's identifier for the person.
    pub subject: &'a str,
    /// The attributes the provider's claims mapped.
    #[schema(value_type = Object)]
    pub attributes: &'a Map<String, Value>,
}

/// The reply an admission webhook gives Stepclimb, with status 200, at most 64 KiB. Any other
/// status, a redirect, a body that is not this object, or no answer within 3 seconds refuses
/// the sign-in with `auth.admission_unavailable`. Members not listed here are ignored.
#[derive(ToSchema)]
pub struct AdmissionWebhookReply {
    /// `true` admits the sign-in; `false` refuses it with `auth.admission_refused`.
    pub allow: bool,
    /// Group keys for the user: an array of strings when present, never `null`. This release
    /// checks them and does not apply them yet.
    #[schema(value_type = Vec<String>, required = false)]
    pub groups: Option<Vec<String>>,
    /// Attributes merged over the provider's mapped attributes before they are stored; these
    /// keys win. An object when present, never `null`.
    #[schema(value_type = Object, required = false)]
    pub attributes: Option<Map<String, Value>>,
}

/// A well-formed reply (B14). `groups`, when present, has been checked to be an array of
/// strings; applying them is spec 07 I7's.
#[derive(Debug, PartialEq)]
pub struct Reply {
    pub allow: bool,
    /// The reply's `attributes`, empty when it sent none.
    pub attributes: Map<String, Value>,
}

/// What step 4 decided.
#[derive(Debug)]
pub enum Decision {
    /// The organization has no webhook, or it is not enabled.
    Skipped,
    /// The webhook answered `allow: true`, with the attributes it sent.
    Allowed(Map<String, Value>),
    /// The webhook answered `allow: false` (B15).
    Refused,
    /// The webhook could not decide (B14).
    Unavailable(Unavailable),
}

/// Why the webhook could not decide. None of these carries the URL, a header or a body.
#[derive(Debug, thiserror::Error)]
pub enum Unavailable {
    /// The stored secret does not open with this key ring for this row, or is not a secret.
    #[error("the webhook's signing secret could not be opened")]
    Secret,
    /// The stored URL cannot be sent as a request.
    #[error("the webhook's URL cannot be sent as a request")]
    Url,
    /// The policy, the timeout, the body limit or the connection refused the call.
    #[error(transparent)]
    Outbound(#[from] OutboundError),
    /// The status was not 200.
    #[error("the webhook answered status {0}")]
    Status(u16),
    /// The body is not JSON.
    #[error("the webhook's reply is not JSON")]
    NotJson,
    /// The reply, or one of its members, has the wrong shape.
    #[error("the webhook's reply has a malformed `{0}`")]
    Malformed(&'static str),
}

/// Step 4 for `call` in the scope's organization, on the sign-in's transaction `conn`, at the
/// request clock's `now`. A query that fails, or a clock before 1970, is a fault; everything
/// about the webhook itself is a [`Decision`].
pub async fn decide(
    conn: &mut PgConnection,
    scope: OrgScope,
    now: SystemTime,
    keys: &KeyRing,
    providers: &Providers,
    call: &Call<'_>,
) -> Result<Decision, ApiError> {
    let webhook = admission_webhooks::get(&mut *conn, scope)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let Some(webhook) = webhook.filter(|webhook| webhook.enabled) else {
        return Ok(Decision::Skipped);
    };
    let tenancy = organizations::tenancy(&mut *conn)
        .await
        .map_err(|e| ApiError::internal(&e))?;
    let client = providers.admission_webhook(tenancy);
    let timestamp = now
        .duration_since(UNIX_EPOCH)
        .map_err(|e| ApiError::internal(&e))?
        .as_secs();
    let body = serde_json::to_vec(call).map_err(|e| ApiError::internal(&e))?;
    let request = match signed_request(keys, scope, &webhook, timestamp, body) {
        Ok(request) => request,
        Err(reason) => return Ok(Decision::Unavailable(reason)),
    };
    Ok(match send(client, request).await {
        Ok(Reply {
            allow: true,
            attributes,
        }) => Decision::Allowed(attributes),
        Ok(Reply { allow: false, .. }) => Decision::Refused,
        Err(reason) => Decision::Unavailable(reason),
    })
}

/// The POST of `body` to `webhook`'s URL, signed with its secret opened from the key ring for
/// its row, at `timestamp` under a new message id. The secret is dropped, and its bytes wiped,
/// before this returns.
fn signed_request(
    keys: &KeyRing,
    scope: OrgScope,
    webhook: &AdmissionWebhook,
    timestamp: u64,
    body: Vec<u8>,
) -> Result<http::Request<Vec<u8>>, Unavailable> {
    let org_id = scope.org_id();
    let opened = keys
        .decrypt(
            &webhook.sealed_secret,
            &secret_context(&org_id, &webhook.id),
        )
        .map_err(|_| Unavailable::Secret)?;
    let secret = Secret::from_decrypted(opened).map_err(|_| Unavailable::Secret)?;
    // A UUID's simple form has no `.`, which Standard Webhooks forbids in an id.
    let id = format!("msg_{}", new_id().simple());
    let signature = secret.sign(&id, timestamp, &body);
    drop(secret);
    request(&webhook.url, &id, timestamp, &signature, body)
}

/// The POST of `body` to `url` with the Standard Webhooks headers.
pub fn request(
    url: &str,
    id: &str,
    timestamp: u64,
    signature: &str,
    body: Vec<u8>,
) -> Result<http::Request<Vec<u8>>, Unavailable> {
    // The URL was checked on save; parsing it again gives the ASCII form a request line takes.
    let url = Url::parse(url).map_err(|_| Unavailable::Url)?;
    http::Request::builder()
        .method(Method::POST)
        .uri(url.as_str())
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .header(ID_HEADER, id)
        .header(TIMESTAMP_HEADER, timestamp)
        .header(SIGNATURE_HEADER, signature)
        .body(body)
        .map_err(|_| Unavailable::Url)
}

/// Send `request` through `client` and read the reply (B14).
pub async fn send(
    client: &OutboundClient,
    request: http::Request<Vec<u8>>,
) -> Result<Reply, Unavailable> {
    let response = client.send(request).await?;
    parse(response.status(), response.body())
}

/// The reply in `body` with `status`, when it is well formed (B14).
fn parse(status: StatusCode, body: &[u8]) -> Result<Reply, Unavailable> {
    if status != StatusCode::OK {
        return Err(Unavailable::Status(status.as_u16()));
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| Unavailable::NotJson)?;
    let Value::Object(mut reply) = value else {
        return Err(Unavailable::Malformed("body"));
    };
    let Some(Value::Bool(allow)) = reply.get("allow") else {
        return Err(Unavailable::Malformed("allow"));
    };
    let allow = *allow;
    match reply.get("groups") {
        None => {}
        Some(Value::Array(groups)) if groups.iter().all(Value::is_string) => {}
        Some(_) => return Err(Unavailable::Malformed("groups")),
    }
    let attributes = match reply.remove("attributes") {
        None => Map::new(),
        Some(Value::Object(attributes)) => attributes,
        Some(_) => return Err(Unavailable::Malformed("attributes")),
    };
    Ok(Reply { allow, attributes })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parsed(body: Value) -> Result<Reply, Unavailable> {
        parse(StatusCode::OK, body.to_string().as_bytes())
    }

    #[test]
    fn a_reply_needs_a_boolean_allow() {
        assert_eq!(
            parsed(json!({ "allow": true })).unwrap(),
            Reply {
                allow: true,
                attributes: Map::new()
            }
        );
        assert!(!parsed(json!({ "allow": false, "extra": 1 })).unwrap().allow);
        for body in [
            json!({}),
            json!({ "allow": "true" }),
            json!({ "allow": 1 }),
            json!({ "allow": null }),
            json!([true]),
            json!(true),
        ] {
            assert!(
                matches!(parsed(body.clone()), Err(Unavailable::Malformed(_))),
                "{body}"
            );
        }
    }

    #[test]
    fn groups_are_strings_and_attributes_an_object() {
        let reply = parsed(json!({
            "allow": true,
            "groups": ["a", "b"],
            "attributes": { "rating": 5 },
        }))
        .unwrap();
        assert_eq!(
            reply.attributes,
            json!({ "rating": 5 }).as_object().cloned().unwrap()
        );
        assert!(parsed(json!({ "allow": true, "groups": [] })).is_ok());
        for body in [
            json!({ "allow": true, "groups": "a" }),
            json!({ "allow": true, "groups": ["a", 1] }),
            json!({ "allow": true, "groups": null }),
            json!({ "allow": true, "attributes": [] }),
            json!({ "allow": true, "attributes": null }),
            json!({ "allow": false, "attributes": "x" }),
        ] {
            assert!(
                matches!(parsed(body.clone()), Err(Unavailable::Malformed(_))),
                "{body}"
            );
        }
    }

    #[test]
    fn only_200_with_json_is_a_reply() {
        let allow = br#"{"allow": true}"#;
        for status in [201, 204, 302, 400, 500] {
            let status = StatusCode::from_u16(status).unwrap();
            assert!(matches!(
                parse(status, allow),
                Err(Unavailable::Status(code)) if code == status.as_u16()
            ));
        }
        assert!(matches!(
            parse(StatusCode::OK, b"allow"),
            Err(Unavailable::NotJson)
        ));
        assert!(matches!(
            parse(StatusCode::OK, b""),
            Err(Unavailable::NotJson)
        ));
    }

    #[test]
    fn the_request_is_a_json_post_with_the_three_headers() {
        let request = request(
            "https://hooks.example.test/a?b=1",
            "msg_1",
            1_763_000_000,
            "v1,abc=",
            b"{}".to_vec(),
        )
        .unwrap();
        assert_eq!(request.method(), Method::POST);
        assert_eq!(request.uri(), "https://hooks.example.test/a?b=1");
        let headers = request.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "application/json");
        assert_eq!(headers["webhook-id"], "msg_1");
        assert_eq!(headers["webhook-timestamp"], "1763000000");
        assert_eq!(headers["webhook-signature"], "v1,abc=");
        assert!(matches!(
            super::request("not a url", "msg_1", 0, "v1,", Vec::new()),
            Err(Unavailable::Url)
        ));
    }

    #[test]
    fn the_body_has_exactly_the_three_members() {
        let attributes = json!({ "rating": 5 }).as_object().cloned().unwrap();
        let call = Call {
            provider: "sso",
            subject: "sub-1",
            attributes: &attributes,
        };
        assert_eq!(
            serde_json::to_value(&call).unwrap(),
            json!({ "provider": "sso", "subject": "sub-1", "attributes": { "rating": 5 } })
        );
    }
}
