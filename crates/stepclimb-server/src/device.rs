// SPDX-License-Identifier: AGPL-3.0-or-later
//! The device cookie: a browser that signed in to an account with its password before is not
//! held back by that account's failed-check budget (docs/specs/m0/08-sessions-and-csrf.md B40,
//! B42; the owner's decision on issue 26, 13 Nov 2026).
//!
//! A successful password sign-in or password enrollment sets `__Host-sc_device`, which names the
//! account's user. A later password check for that same user from a browser presenting a valid
//! one is neither counted against nor refused by the per-account budget; the per-address budget
//! still applies to every request. Anyone else, an attacker guessing included, has no such
//! cookie, so their failures still throttle the account, while its owner keeps signing in.
//!
//! The value is sealed with the key ring (AES-256-GCM, `stepclimb_auth::encryption`), bound to
//! the organization: the user's id and the time it was issued, in base64url. Nobody without the
//! keys can make one, read whose it is, or move one to another organization. It is good for
//! [`LIFETIME`] after it was issued, a limit the server checks itself; every sign-in issues a
//! new one. A cookie that does not open, is too old, or names another user exempts nothing and
//! is not an error.

use std::time::{Duration, SystemTime};

use axum::http::{HeaderMap, HeaderValue};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use stepclimb_auth::encryption::{EncryptError, KeyRing};
use stepclimb_core::{OrgScope, id::Uuid};

use crate::cookie::{self, Presented};

/// The cookie's name. The `__Host-` prefix keeps every other host from setting or reading it.
pub const COOKIE_NAME: &str = "__Host-sc_device";

/// How long a device cookie exempts its browser: 30 days, a session's longest life (spec 08 B7).
pub const LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// `SameSite=Strict`: only the site's own requests, which every sign-in is, carry it.
const ATTRIBUTES: &str = "Path=/; Secure; HttpOnly; SameSite=Strict";

/// What the value is sealed for (`stepclimb_auth::encryption` context rules).
const CONTEXT_NAME: &[u8] = b"device_cookie";

/// The user id's 16 bytes, then the issue time in seconds since the epoch, 8 bytes big-endian.
const PLAIN_LEN: usize = 24;

/// The `Set-Cookie` value naming the user `user_id` of the scope's organization, issued at
/// `now`, marked sensitive.
pub fn set_cookie(
    keys: &KeyRing,
    scope: OrgScope,
    user_id: Uuid,
    now: SystemTime,
) -> Result<HeaderValue, EncryptError> {
    let issued = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut plain = [0u8; PLAIN_LEN];
    plain[..16].copy_from_slice(user_id.as_bytes());
    plain[16..].copy_from_slice(&issued.to_be_bytes());
    let org_id = scope.org_id();
    let sealed = keys.encrypt(&plain, &[org_id.as_bytes(), CONTEXT_NAME])?;
    let cookie = format!(
        "{COOKIE_NAME}={}; Max-Age={}; {ATTRIBUTES}",
        URL_SAFE_NO_PAD.encode(sealed),
        LIFETIME.as_secs(),
    );
    // base64url and fixed ASCII, all valid in a header value.
    let mut value =
        HeaderValue::from_str(&cookie).expect("a device cookie is a valid header value");
    value.set_sensitive(true);
    Ok(value)
}

/// The user the request's device cookie names in the scope's organization at `now`, if it
/// presents exactly one that opens and is younger than [`LIFETIME`].
pub fn user(keys: &KeyRing, scope: OrgScope, headers: &HeaderMap, now: SystemTime) -> Option<Uuid> {
    let Presented::One(value) = cookie::presented(headers, COOKIE_NAME) else {
        return None;
    };
    let sealed = URL_SAFE_NO_PAD.decode(value).ok()?;
    let org_id = scope.org_id();
    let plain = keys
        .decrypt(&sealed, &[org_id.as_bytes(), CONTEXT_NAME])
        .ok()?;
    let plain: [u8; PLAIN_LEN] = plain.try_into().ok()?;
    let user_id = Uuid::from_slice(&plain[..16]).ok()?;
    let issued_secs = u64::from_be_bytes(plain[16..].try_into().ok()?);
    let issued = SystemTime::UNIX_EPOCH + Duration::from_secs(issued_secs);
    let age = now.duration_since(issued).ok()?;
    (age < LIFETIME).then_some(user_id)
}

#[cfg(test)]
mod tests {
    use axum::http::header;

    use super::*;

    fn keys() -> KeyRing {
        KeyRing::parse("test:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap()
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "a test stands in for the request-organization resolver"
    )]
    fn scope(n: u128) -> OrgScope {
        OrgScope::instance(Uuid::from_u128(n))
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_763_000_000 + secs)
    }

    /// The request headers a browser sends back for `set_cookie`.
    fn presented(set_cookie: &HeaderValue) -> HeaderMap {
        let pair = set_cookie.to_str().unwrap().split(';').next().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_str(pair).unwrap());
        headers
    }

    #[test]
    fn a_cookie_names_its_user_until_its_lifetime_ends() {
        let user_id = Uuid::from_u128(42);
        let set = set_cookie(&keys(), scope(1), user_id, at(0)).unwrap();
        let headers = presented(&set);
        assert_eq!(user(&keys(), scope(1), &headers, at(0)), Some(user_id));
        let last = at(LIFETIME.as_secs() - 1);
        assert_eq!(user(&keys(), scope(1), &headers, last), Some(user_id));
        let over = at(LIFETIME.as_secs());
        assert_eq!(user(&keys(), scope(1), &headers, over), None);
        // Issued in the future (the clock went back): not trusted.
        let set = set_cookie(&keys(), scope(1), user_id, at(100)).unwrap();
        assert_eq!(user(&keys(), scope(1), &presented(&set), at(0)), None);
    }

    #[test]
    fn a_cookie_is_bound_to_its_organization_and_keys() {
        let set = set_cookie(&keys(), scope(1), Uuid::from_u128(42), at(0)).unwrap();
        let headers = presented(&set);
        assert_eq!(user(&keys(), scope(2), &headers, at(0)), None);
        let other = KeyRing::parse("other:AQECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").unwrap();
        assert_eq!(user(&other, scope(1), &headers, at(0)), None);
    }

    #[test]
    fn a_made_up_or_doubled_cookie_names_nobody() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-sc_device=AAAA"),
        );
        assert_eq!(user(&keys(), scope(1), &headers, at(0)), None);
        let set = set_cookie(&keys(), scope(1), Uuid::from_u128(42), at(0)).unwrap();
        let pair = set.to_str().unwrap().split(';').next().unwrap().to_owned();
        let mut doubled = HeaderMap::new();
        let both = format!("{pair}; {pair}");
        doubled.insert(header::COOKIE, HeaderValue::from_str(&both).unwrap());
        assert_eq!(user(&keys(), scope(1), &doubled, at(0)), None);
    }

    #[test]
    fn the_cookie_carries_the_host_attributes_and_hides_its_value() {
        let set = set_cookie(&keys(), scope(1), Uuid::from_u128(42), at(0)).unwrap();
        let text = set.to_str().unwrap();
        assert!(text.starts_with("__Host-sc_device="), "{text}");
        assert!(
            text.ends_with("; Max-Age=2592000; Path=/; Secure; HttpOnly; SameSite=Strict"),
            "{text}"
        );
        assert!(set.is_sensitive());
    }
}
