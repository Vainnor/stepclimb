// SPDX-License-Identifier: AGPL-3.0-or-later
//! The sign-in flows repository (docs/specs/m0/08-sessions-and-csrf.md B17 to B21, Q8).
//!
//! A sign-in that spans more than one request (an OIDC or OAuth 2 redirect, a WebAuthn ceremony,
//! password then TOTP, an OAuth 2.1 authorization) keeps its state in one `auth_flows` row of the
//! request's organization, which every query here filters by. The browser holds a random token
//! in the flow cookie; the row holds only its SHA-256 hash, so this module never sees a token.
//!
//! The payload is opaque here: the caller seals it with the key ring before [`create`] and opens
//! it after [`consume`] (B20), bound to the row's id, which the caller therefore generates.
//!
//! A flow lives [`LIFETIME`] and is used once: [`consume`] deletes the row it returns, and the
//! caller runs it in the transaction that creates the session or authorization code (B18), so
//! two requests presenting one token cannot both complete. An expired flow is deleted by the
//! request that finds it and by [`delete_expired`], the periodic cleanup (B21).

use std::time::{Duration, SystemTime};

use sqlx::PgExecutor;
use uuid::Uuid;

use crate::{
    OrgScope,
    db::{TimeOutOfRange, epoch_micros, from_epoch_micros},
};

/// How long a flow lasts after it starts (Q8).
pub const LIFETIME: Duration = Duration::from_secs(10 * 60);

/// The longest `return_to` kept, in bytes; a longer one becomes `/`.
pub const MAX_RETURN_TO_LEN: usize = 2048;

/// The composite foreign key that keeps a flow's user in the flow's organization.
const USER_FKEY: &str = "auth_flows_org_id_user_id_fkey";

/// What a flow is for. A flow is completed only by the step of its own kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// An OIDC redirect to the provider and back (spec 05).
    Oidc,
    /// A generic OAuth 2 redirect to the provider and back (spec 06).
    Oauth2,
    /// A WebAuthn registration ceremony (spec 04 B7, B8, B13).
    PasskeyRegister,
    /// A WebAuthn authentication ceremony (spec 04 B11, B12).
    PasskeySignIn,
    /// The TOTP step after a correct password (spec 04 B15, B20).
    PasswordTotp,
    /// An OAuth 2.1 authorization request waiting for the browser to sign in (spec 08 B31).
    OauthAuthorize,
}

impl Kind {
    /// Every kind, in the order the `auth_flows.kind` check constraint lists them.
    pub const ALL: [Self; 6] = [
        Self::Oidc,
        Self::Oauth2,
        Self::PasskeyRegister,
        Self::PasskeySignIn,
        Self::PasswordTotp,
        Self::OauthAuthorize,
    ];

    /// The stored spelling.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Oidc => "oidc",
            Self::Oauth2 => "oauth2",
            Self::PasskeyRegister => "passkey_register",
            Self::PasskeySignIn => "passkey_sign_in",
            Self::PasswordTotp => "password_totp",
            Self::OauthAuthorize => "oauth_authorize",
        }
    }
}

/// A path on this host for the browser to land on after sign-in (B19).
///
/// Only a path that starts with one `/` not followed by `/` or `\` is kept; anything else
/// becomes `/`, so a sign-in never redirects to another host. Every byte must also be printable
/// ASCII: browsers drop tab and newline from a URL, so `/<tab>/evil.example` would otherwise
/// reach the browser as `//evil.example`. A `.` or `..` segment, literal or `%2e`, is refused
/// too: browsers resolve it, so `/a/..//evil.example` would reach them as `//evil.example`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReturnTo(String);

impl ReturnTo {
    /// `requested` if it is a safe path, otherwise `/`. `None` (no `return_to` given) is `/`.
    #[must_use]
    pub fn sanitize(requested: Option<&str>) -> Self {
        match requested {
            Some(path) if is_safe_path(path) => Self(path.to_owned()),
            _ => Self::root(),
        }
    }

    /// `/`.
    #[must_use]
    pub fn root() -> Self {
        Self("/".to_owned())
    }

    /// The path, for a `Location` header or a JSON `return_to`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn is_safe_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() <= MAX_RETURN_TO_LEN
        && bytes.first() == Some(&b'/')
        && !matches!(bytes.get(1), Some(b'/' | b'\\'))
        && bytes.iter().all(|byte| (b'!'..=b'~').contains(byte))
        && !has_dot_segment(path)
}

/// True when the path part (before `?` or `#`) has a `.` or `..` segment, written literally or
/// with `%2e`. Browsers resolve those segments, so `/a/..//evil.example` reaches them as
/// `//evil.example`. `\` counts as a separator, as browsers read it in an `http` URL.
fn has_dot_segment(path: &str) -> bool {
    let end = path.find(['?', '#']).unwrap_or(path.len());
    path[..end].split(['/', '\\']).any(|segment| {
        let decoded = segment.to_ascii_lowercase().replace("%2e", ".");
        decoded == "." || decoded == ".."
    })
}

/// A flow to store.
#[derive(Clone, Copy, Debug)]
pub struct NewFlow<'a> {
    /// The row's key, generated by the caller (`id::new_id`) so the payload can be sealed bound
    /// to it before the insert.
    pub id: Uuid,
    pub kind: Kind,
    /// The user the flow is for, when one is known before it completes (a TOTP step, adding a
    /// passkey while signed in). Must be a user of the scope's organization.
    pub user_id: Option<Uuid>,
    /// SHA-256 of the token the caller has just generated for the flow cookie.
    pub token_hash: &'a [u8; 32],
    /// The sealed payload.
    pub payload: &'a [u8],
    pub return_to: &'a ReturnTo,
}

/// A flow [`consume`] took, already deleted.
#[derive(Clone, PartialEq, Eq)]
pub struct Flow {
    pub id: Uuid,
    pub kind: Kind,
    pub user_id: Option<Uuid>,
    /// The sealed payload, as stored.
    pub payload: Vec<u8>,
    pub return_to: ReturnTo,
    pub expires_at: SystemTime,
}

impl std::fmt::Debug for Flow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flow")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("user_id", &self.user_id)
            .field("return_to", &self.return_to)
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
    }
}

/// What a presented flow token turned out to be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Consumed {
    /// The flow was live; its row is deleted.
    Valid(Flow),
    /// The flow had passed its expiry; its row is deleted.
    Expired,
    /// No flow of this kind in the scope's organization has this hash: never created, already
    /// used, cleaned up, another kind's or another organization's.
    Unknown,
}

/// Why a flows repository call failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// `user_id` names no user of the scope's organization.
    #[error("the user does not exist")]
    UserNotFound,
    /// A stored `kind` this release does not know.
    #[error("the stored flow kind {0:?} is not one this release knows")]
    UnknownKind(String),
    /// A time that cannot be stored or read.
    #[error(transparent)]
    Time(#[from] TimeOutOfRange),
    #[error(transparent)]
    Database(sqlx::Error),
}

impl From<sqlx::Error> for Error {
    fn from(error: sqlx::Error) -> Self {
        let constraint = error.as_database_error().and_then(|e| e.constraint());
        match constraint {
            Some(USER_FKEY) => Self::UserNotFound,
            _ => Self::Database(error),
        }
    }
}

/// Store a flow in the scope's organization, started at `now` and expiring [`LIFETIME`] after
/// it. Returns the expiry.
pub async fn create(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    flow: &NewFlow<'_>,
    now: SystemTime,
) -> Result<SystemTime, Error> {
    let expires_at = now.checked_add(LIFETIME).ok_or(TimeOutOfRange)?;
    sqlx::query!(
        r#"INSERT INTO auth_flows (id, org_id, token_hash, kind, user_id, payload, return_to,
                                   expires_at, created_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7,
                   timestamptz 'epoch' + $9::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $8::bigint * interval '1 microsecond',
                   timestamptz 'epoch' + $8::bigint * interval '1 microsecond')"#,
        flow.id,
        scope.org_id(),
        flow.token_hash.as_slice(),
        flow.kind.as_str(),
        flow.user_id,
        flow.payload,
        flow.return_to.as_str(),
        epoch_micros(now)?,
        epoch_micros(expires_at)?,
    )
    .execute(db)
    .await?;
    Ok(expires_at)
}

/// An `auth_flows` row as [`consume`] deletes it.
struct Row {
    id: Uuid,
    kind: String,
    user_id: Option<Uuid>,
    payload: Vec<u8>,
    return_to: String,
    expires_at_us: i64,
}

/// Take the flow of kind `kind` in the scope's organization whose token hashes to `token_hash`
/// (B18, B21).
///
/// The row is deleted whether it is live or expired, so a second call with the same hash answers
/// [`Consumed::Unknown`]. Run it in the transaction that completes the sign-in: if that rolls
/// back, so does the delete, and while it is open a concurrent call waits on the row and then
/// finds nothing. A flow is live while `now < expires_at`. A flow of another kind is left alone.
pub async fn consume(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    token_hash: &[u8; 32],
    kind: Kind,
    now: SystemTime,
) -> Result<Consumed, Error> {
    let row = sqlx::query_as!(
        Row,
        r#"DELETE FROM auth_flows
           WHERE org_id = $1 AND token_hash = $2 AND kind = $3
           RETURNING id, kind, user_id, payload, return_to,
                     (extract(epoch FROM expires_at) * 1000000)::bigint AS "expires_at_us!""#,
        scope.org_id(),
        token_hash.as_slice(),
        kind.as_str(),
    )
    .fetch_optional(db)
    .await?;
    let Some(row) = row else {
        return Ok(Consumed::Unknown);
    };
    let expires_at = from_epoch_micros(row.expires_at_us)?;
    if now >= expires_at {
        return Ok(Consumed::Expired);
    }
    let stored_kind = Kind::ALL
        .into_iter()
        .find(|known| known.as_str() == row.kind)
        .ok_or(Error::UnknownKind(row.kind))?;
    Ok(Consumed::Valid(Flow {
        id: row.id,
        kind: stored_kind,
        user_id: row.user_id,
        payload: row.payload,
        // The column's check constraint holds stored values to the same rule, so this keeps the
        // value; it is sanitized again so nothing read back can name another host.
        return_to: ReturnTo::sanitize(Some(&row.return_to)),
        expires_at,
    }))
}

/// Delete every flow of the scope's organization that has expired at `now`, for the periodic
/// cleanup (B21). Returns how many were deleted.
pub async fn delete_expired(
    db: impl PgExecutor<'_>,
    scope: OrgScope,
    now: SystemTime,
) -> Result<u64, Error> {
    let result = sqlx::query!(
        r#"DELETE FROM auth_flows
           WHERE org_id = $1
             AND expires_at <= timestamptz 'epoch' + $2::bigint * interval '1 microsecond'"#,
        scope.org_id(),
        epoch_micros(now)?,
    )
    .execute(db)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sanitized(requested: &str) -> String {
        ReturnTo::sanitize(Some(requested)).as_str().to_owned()
    }

    #[test]
    fn a_path_on_this_host_is_kept() {
        for path in [
            "/",
            "/c/s1/m/2",
            "/oauth/authorize?client_id=app&state=a%2Fb",
            "/a//b",
            "/a\\b",
            "/.well-known/x",
            "/a/...",
            "/a/..b",
            "/a?x=/../y",
            "/a#/../b",
        ] {
            assert_eq!(sanitized(path), path);
        }
    }

    #[test]
    fn anything_that_could_leave_the_host_becomes_root() {
        for requested in [
            "https://evil.example",
            "//evil.example",
            "/\\evil.example",
            "javascript:x",
            "",
            "evil.example/",
            "\\\\evil.example",
            "/\t/evil.example",
            "/\n/evil.example",
            "/ /evil.example",
            "/caf\u{e9}",
            " /c/s1",
        ] {
            assert_eq!(sanitized(requested), "/", "{requested:?}");
        }
        assert_eq!(ReturnTo::sanitize(None).as_str(), "/");
    }

    #[test]
    fn a_dot_segment_becomes_root() {
        for requested in [
            "/..//evil.example",
            "/.//evil.example",
            "/a/..//evil",
            "/%2e%2e//evil.example",
            "/%2E//evil.example",
            "/a/.%2e//evil",
            "/a/%2e./b",
            "/a\\..\\/evil",
            "/a/.",
            "/a/..",
            "/a/..?x=1",
        ] {
            assert_eq!(sanitized(requested), "/", "{requested:?}");
        }
    }

    #[test]
    fn a_path_longer_than_the_limit_becomes_root() {
        let longest = format!("/{}", "a".repeat(MAX_RETURN_TO_LEN - 1));
        assert_eq!(sanitized(&longest), longest);
        assert_eq!(sanitized(&format!("{longest}a")), "/");
    }

    #[test]
    fn the_lifetime_is_ten_minutes() {
        assert_eq!(LIFETIME.as_secs(), 10 * 60);
    }

    #[test]
    fn kinds_have_distinct_stored_spellings() {
        let spellings: std::collections::HashSet<&str> =
            Kind::ALL.iter().map(|kind| kind.as_str()).collect();
        assert_eq!(spellings.len(), Kind::ALL.len());
    }
}
