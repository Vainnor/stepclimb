// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sign-in flows over HTTP (docs/specs/m0/08-sessions-and-csrf.md B17 to B21, Q8).
//!
//! A sign-in step that must be continued by a later request calls [`start`] with the
//! organization it runs in, what the flow is for and the secrets to keep. [`start`] seals the
//! payload with the key ring, stores the flow and returns the `Set-Cookie` value for
//! `__Host-sc_flow`, which binds the flow to this browser. The step that continues it calls
//! [`finish`] inside the transaction that completes the sign-in: it reads the cookie, deletes the
//! flow and opens the payload, or answers [`FinishError::Invalid`] or [`FinishError::Expired`].
//! The response that completes the flow clears the cookie with [`clear_cookie`].
//!
//! [`spawn_cleanup`] deletes expired flows on a `tokio` interval until the jobs runner exists.
//! The cookie's value is the flow token and never reaches a log line or an error body.

use std::time::{Duration, SystemTime};

use axum::http::{HeaderMap, HeaderValue};
use sqlx::{PgExecutor, PgPool};
use stepclimb_auth::{
    encryption::{DecryptError, EncryptError, KeyRing},
    token::{RandomError, Token, TokenHash},
};
use stepclimb_core::{
    OrgScope,
    auth_flows::{self, Consumed, Kind, LIFETIME, NewFlow, ReturnTo},
    clock::Clock,
    id::{Uuid, new_id},
    organizations,
};

use crate::cookie::{self, Presented};

/// The flow cookie's name. The `__Host-` prefix makes the browser refuse it unless it is
/// `Secure`, has `Path=/` and no `Domain`, so no other host can set or read it.
pub const COOKIE_NAME: &str = "__Host-sc_flow";

/// `SameSite=Lax`, so the cookie comes back on the top-level GET of a provider's redirect.
const ATTRIBUTES: &str = "Path=/; Secure; HttpOnly; SameSite=Lax";

/// How often [`spawn_cleanup`] deletes expired flows. An expired flow that no request finds
/// stays at most this long past its expiry.
pub const CLEANUP_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// The table and column the payload is sealed for (`stepclimb_auth::encryption` context rules).
const PAYLOAD_TABLE: &[u8] = b"auth_flows";
const PAYLOAD_COLUMN: &[u8] = b"payload";

/// The codes a flow's continuing step answers with. The status, or a redirect carrying the code,
/// is the caller's: an API step and a browser callback answer differently.
pub mod code {
    /// No flow cookie, or one that names no live flow of this step's kind (B18).
    pub const AUTH_FLOW_INVALID: &str = "auth.flow_invalid";
    /// The flow passed its lifetime (B18).
    pub const AUTH_FLOW_EXPIRED: &str = "auth.flow_expired";
}

/// A flow to start.
#[derive(Clone, Copy)]
pub struct FlowStart<'a> {
    pub kind: Kind,
    /// The user the flow is for, when known before it completes. Must be a user of the scope's
    /// organization.
    pub user_id: Option<Uuid>,
    /// What the continuing step needs (a PKCE verifier, a nonce, WebAuthn state), serialized by
    /// the caller. It is stored sealed (B20).
    pub payload: &'a [u8],
    /// The `return_to` the request asked for; [`ReturnTo::sanitize`] decides what is kept (B19).
    pub return_to: Option<&'a str>,
}

impl std::fmt::Debug for FlowStart<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlowStart")
            .field("kind", &self.kind)
            .field("user_id", &self.user_id)
            .field("return_to", &self.return_to)
            .finish_non_exhaustive()
    }
}

/// A flow [`start`] stored, and the cookie that hands its token to the browser.
#[derive(Debug)]
pub struct Started {
    pub id: Uuid,
    pub return_to: ReturnTo,
    pub expires_at: SystemTime,
    /// The `Set-Cookie` value. It holds the token: never log it.
    pub set_cookie: HeaderValue,
}

/// Why [`start`] could not store a flow.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Random(#[from] RandomError),
    #[error("could not seal the flow payload: {0}")]
    Encrypt(#[from] EncryptError),
    #[error(transparent)]
    Flow(#[from] auth_flows::Error),
}

/// Store a new flow in the scope's organization, started at `now`, and return its cookie (B17).
///
/// The token is new for every flow. The payload is sealed bound to the organization, the table,
/// the row's id and the column, so a payload copied to another flow does not open.
pub async fn start(
    db: impl PgExecutor<'_>,
    keys: &KeyRing,
    scope: OrgScope,
    flow: FlowStart<'_>,
    now: SystemTime,
) -> Result<Started, StartError> {
    let id = new_id();
    let token = Token::generate()?;
    let org_id = scope.org_id();
    let sealed = keys.encrypt(flow.payload, &payload_context(&org_id, &id))?;
    let return_to = ReturnTo::sanitize(flow.return_to);
    let token_hash = token.hash();
    let new = NewFlow {
        id,
        kind: flow.kind,
        user_id: flow.user_id,
        token_hash: token_hash.as_bytes(),
        payload: &sealed,
        return_to: &return_to,
    };
    let expires_at = auth_flows::create(db, scope, &new, now).await?;
    Ok(Started {
        id,
        return_to,
        expires_at,
        set_cookie: set_cookie(&token),
    })
}

/// A flow [`finish`] took: its row is deleted in the caller's transaction.
#[derive(Clone, PartialEq, Eq)]
pub struct Finished {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    /// The payload [`start`] was given, opened. It can hold secrets: never log it.
    pub payload: Vec<u8>,
    pub return_to: ReturnTo,
}

impl std::fmt::Debug for Finished {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Finished")
            .field("id", &self.id)
            .field("user_id", &self.user_id)
            .field("return_to", &self.return_to)
            .finish_non_exhaustive()
    }
}

/// Why [`finish`] did not return a flow.
#[derive(Debug, thiserror::Error)]
pub enum FinishError {
    /// No flow cookie, two of them, a non-ASCII one, or a token that names no flow of this kind
    /// in the organization (never started, already used, another kind's or another
    /// organization's).
    #[error("the sign-in flow is unknown or already used")]
    Invalid,
    /// The flow had passed its lifetime; its row is deleted.
    #[error("the sign-in flow has expired")]
    Expired,
    /// The stored payload did not open: a key missing from the ring or an altered row.
    #[error("could not open the flow payload: {0}")]
    Decrypt(#[from] DecryptError),
    #[error(transparent)]
    Flow(#[from] auth_flows::Error),
}

impl FinishError {
    /// The code to answer with, for the two outcomes a user can cause. The others are server
    /// faults: log them and answer 500.
    pub fn code(&self) -> Option<&'static str> {
        match self {
            Self::Invalid => Some(code::AUTH_FLOW_INVALID),
            Self::Expired => Some(code::AUTH_FLOW_EXPIRED),
            Self::Decrypt(_) | Self::Flow(_) => None,
        }
    }
}

/// Take the flow of kind `kind` that the request's flow cookie names, in the scope's
/// organization, at `now` (B18).
///
/// Pass the transaction that completes the sign-in (`&mut *tx`): the flow is deleted in it, so
/// it is used once even when two requests race, and stays usable if the transaction rolls back.
/// Commit after [`FinishError::Expired`] too, so the expired row is gone (B21). A caller that
/// knows the user (adding a passkey while signed in) also checks [`Finished::user_id`].
pub async fn finish(
    db: impl PgExecutor<'_>,
    keys: &KeyRing,
    scope: OrgScope,
    request_headers: &HeaderMap,
    kind: Kind,
    now: SystemTime,
) -> Result<Finished, FinishError> {
    let Presented::One(token) = cookie::presented(request_headers, COOKIE_NAME) else {
        return Err(FinishError::Invalid);
    };
    let hash = TokenHash::of(token);
    let flow = match auth_flows::consume(db, scope, hash.as_bytes(), kind, now).await? {
        Consumed::Valid(flow) => flow,
        Consumed::Expired => return Err(FinishError::Expired),
        Consumed::Unknown => return Err(FinishError::Invalid),
    };
    let org_id = scope.org_id();
    let payload = keys.decrypt(&flow.payload, &payload_context(&org_id, &flow.id))?;
    Ok(Finished {
        id: flow.id,
        user_id: flow.user_id,
        payload,
        return_to: flow.return_to,
    })
}

/// Where a flow's payload lives: `[org_id, "auth_flows", id, "payload"]`.
fn payload_context<'a>(org_id: &'a Uuid, id: &'a Uuid) -> [&'a [u8]; 4] {
    [
        org_id.as_bytes(),
        PAYLOAD_TABLE,
        id.as_bytes(),
        PAYLOAD_COLUMN,
    ]
}

/// `__Host-sc_flow=<token>` with `Max-Age` of the flow's lifetime. A relative lifetime does not
/// depend on the browser's clock agreeing with the server's; the server enforces the expiry
/// anyway.
fn set_cookie(token: &Token) -> HeaderValue {
    let cookie = format!(
        "{COOKIE_NAME}={}; Max-Age={}; {ATTRIBUTES}",
        token.expose_secret(),
        LIFETIME.as_secs(),
    );
    // A token is base64url and the rest is fixed ASCII, all valid in a header value.
    HeaderValue::from_str(&cookie).expect("a flow cookie is a valid header value")
}

/// The `Set-Cookie` value that removes the flow cookie, for the response that completes or
/// refuses a flow. It repeats `Secure` and `Path=/`, without which a browser refuses to touch a
/// `__Host-` cookie.
pub fn clear_cookie() -> HeaderValue {
    HeaderValue::from_static("__Host-sc_flow=; Max-Age=0; Path=/; Secure; HttpOnly; SameSite=Lax")
}

/// Why a cleanup pass stopped.
#[derive(Debug, thiserror::Error)]
pub enum CleanupError {
    #[error("could not list the organizations: {0}")]
    Organizations(#[from] organizations::Error),
    #[error("could not delete expired sign-in flows: {0}")]
    Flows(#[from] auth_flows::Error),
}

/// Delete every flow that has expired at `now`, organization by organization (B21). Returns how
/// many were deleted.
pub async fn delete_expired(db: &PgPool, now: SystemTime) -> Result<u64, CleanupError> {
    let mut deleted = 0;
    for org_id in organizations::ids(db).await? {
        deleted += auth_flows::delete_expired(db, job_scope(org_id), now).await?;
    }
    Ok(deleted)
}

#[expect(
    clippy::disallowed_methods,
    reason = "spec 02 B9 allows the instance scope in a job; the flow cleanup acts in each organization in turn"
)]
fn job_scope(org_id: Uuid) -> OrgScope {
    OrgScope::instance(org_id)
}

/// Run [`delete_expired`] every [`CLEANUP_INTERVAL`] at the clock's time, for as long as the
/// server runs. A failed pass is logged and the next one runs on schedule.
pub fn spawn_cleanup(db: PgPool, clock: Clock) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            match delete_expired(&db, clock.now()).await {
                Ok(0) => {}
                Ok(deleted) => tracing::debug!(deleted, "deleted expired sign-in flows"),
                Err(error) => tracing::error!(%error, "sign-in flow cleanup failed"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_cookie_carries_the_host_attributes_and_the_lifetime() {
        let token = Token::generate().unwrap();
        assert_eq!(
            set_cookie(&token).to_str().unwrap(),
            format!(
                "__Host-sc_flow={}; Max-Age=600; Path=/; Secure; HttpOnly; SameSite=Lax",
                token.expose_secret()
            )
        );
    }

    #[test]
    fn only_the_two_user_caused_outcomes_have_a_code() {
        assert_eq!(FinishError::Invalid.code(), Some("auth.flow_invalid"));
        assert_eq!(FinishError::Expired.code(), Some("auth.flow_expired"));
        assert_eq!(FinishError::Decrypt(DecryptError::Malformed).code(), None);
    }

    #[test]
    fn debug_output_never_contains_the_payload() {
        let start = FlowStart {
            kind: Kind::Oidc,
            user_id: None,
            payload: b"pkce-verifier-zq",
            return_to: Some("/c/1"),
        };
        let finished = Finished {
            id: new_id(),
            user_id: None,
            payload: b"pkce-verifier-zq".to_vec(),
            return_to: ReturnTo::root(),
        };
        let printed = format!("{start:?} {finished:?}");
        assert!(!printed.contains("zq"), "{printed}");
    }
}
