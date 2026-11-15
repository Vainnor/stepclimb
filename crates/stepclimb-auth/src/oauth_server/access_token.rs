// SPDX-License-Identifier: AGPL-3.0-or-later
//! App access tokens (docs/specs/m0/08-sessions-and-csrf.md B32, B33, B36, Q12, ADR 0031).
//!
//! An access token is a JWT (RFC 7519) in the compact JWS form, signed HS256 with the key ring's
//! access token key (`encryption::KeyRing::sign_access_token`): only this server signs and checks
//! them. The header is exactly `alg`, `typ` and `kid`; the payload is exactly the [`Claims`]
//! below. A token lives [`LIFETIME`].
//!
//! [`verify`] reads a presented token in this order and stops at the first problem, before any
//! database query: the length and the three base64url parts; the header, which must name `HS256`,
//! `JWT` and a key of the ring; the signature, compared in constant time; then the payload, whose
//! issuer must be this server's. A token past its `exp` is [`Refusal::Expired`]; every other
//! problem is [`Refusal::Invalid`], which says nothing about which check failed. The server then
//! checks the organization, the client, the user's status and that the refresh family still
//! exists, so revoking a family stops its access tokens at once (Q12).
//!
//! [`AccessToken`] has no `Display` and a redacted `Debug`, so a token reaches no log line.

use std::fmt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::encryption::KeyRing;

/// How long an access token authenticates requests after it is issued (plan: 15 minutes).
pub const LIFETIME: Duration = Duration::from_secs(15 * 60);

/// The longest token text [`verify`] reads. An issued token is about 400 bytes.
pub const MAX_LEN: usize = 2048;

const ALG: &str = "HS256";
const TYP: &str = "JWT";

/// What an access token says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Claims {
    /// The issuer identifier: the server's public URL, as discovery's `oauth.issuer` names it.
    pub iss: String,
    /// The user the token acts as.
    pub sub: Uuid,
    /// The user's organization.
    pub org: Uuid,
    /// The client the token was issued to.
    pub client_id: String,
    /// The refresh token family the token was issued with; revoking it stops the token.
    pub fid: Uuid,
    /// Issued at, in seconds since the Unix epoch.
    pub iat: u64,
    /// Expires at, in seconds since the Unix epoch: `iat` plus [`LIFETIME`].
    pub exp: u64,
}

/// The parts of an access token [`issue`] takes; the times come from the clock it is given.
#[derive(Clone, Copy, Debug)]
pub struct Subject<'a> {
    pub issuer: &'a str,
    pub user_id: Uuid,
    pub org_id: Uuid,
    pub client_id: &'a str,
    pub family_id: Uuid,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    typ: String,
    kid: String,
}

/// An issued access token's text.
pub struct AccessToken(String);

impl AccessToken {
    /// The token's text, for the one token response that hands it to the app. Never log it.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccessToken(<redacted>)")
    }
}

/// `now` cannot be written as seconds since the Unix epoch (before 1970), so no token was made.
#[derive(Debug, thiserror::Error)]
#[error("the clock reads a time before the Unix epoch")]
pub struct ClockBeforeEpoch;

/// Sign a token for `subject`, issued at `now` and expiring [`LIFETIME`] later.
pub fn issue(
    keys: &KeyRing,
    subject: &Subject<'_>,
    now: SystemTime,
) -> Result<AccessToken, ClockBeforeEpoch> {
    let iat = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ClockBeforeEpoch)?
        .as_secs();
    let claims = Claims {
        iss: subject.issuer.to_owned(),
        sub: subject.user_id,
        org: subject.org_id,
        client_id: subject.client_id.to_owned(),
        fid: subject.family_id,
        iat,
        exp: iat + LIFETIME.as_secs(),
    };
    Ok(sign(keys, &claims))
}

fn sign(keys: &KeyRing, claims: &Claims) -> AccessToken {
    let header = Header {
        alg: ALG.to_owned(),
        typ: TYP.to_owned(),
        kid: keys.current_key_id().to_owned(),
    };
    // Structs of strings, UUIDs and integers always serialize.
    let header = serde_json::to_vec(&header).expect("a header serializes");
    let payload = serde_json::to_vec(claims).expect("claims serialize");
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let (_, tag) = keys.sign_access_token(signing_input.as_bytes());
    AccessToken(format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(tag)))
}

/// Why [`verify`] refused a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Not a token this server signed for `issuer`, or one it can no longer check.
    Invalid,
    /// A token this server signed, past its expiry.
    Expired,
}

/// Check `presented` (the module docs list the order) and return its claims.
pub fn verify(
    keys: &KeyRing,
    presented: &str,
    issuer: &str,
    now: SystemTime,
) -> Result<Claims, Refusal> {
    if presented.len() > MAX_LEN {
        return Err(Refusal::Invalid);
    }
    let mut parts = presented.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal::Invalid);
    };
    let header: Header = decode_json(header)?;
    if header.alg != ALG || header.typ != TYP {
        return Err(Refusal::Invalid);
    }
    let tag = URL_SAFE_NO_PAD
        .decode(signature)
        .map_err(|_| Refusal::Invalid)?;
    let signing_input = &presented[..presented.len() - signature.len() - 1];
    if !keys.verify_access_token(&header.kid, signing_input.as_bytes(), &tag) {
        return Err(Refusal::Invalid);
    }
    let claims: Claims = decode_json(payload)?;
    if claims.iss != issuer {
        return Err(Refusal::Invalid);
    }
    let expires = UNIX_EPOCH
        .checked_add(Duration::from_secs(claims.exp))
        .ok_or(Refusal::Invalid)?;
    if now >= expires {
        return Err(Refusal::Expired);
    }
    Ok(claims)
}

fn decode_json<T: for<'de> Deserialize<'de>>(part: &str) -> Result<T, Refusal> {
    let bytes = URL_SAFE_NO_PAD.decode(part).map_err(|_| Refusal::Invalid)?;
    serde_json::from_slice(&bytes).map_err(|_| Refusal::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ISSUER: &str = "https://lms.example.org";
    const KEY_A: &str = "a:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
    const KEY_B: &str = "b:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8=";

    fn keys(text: &str) -> KeyRing {
        KeyRing::parse(text).unwrap()
    }

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_763_000_000 + secs)
    }

    fn subject() -> Subject<'static> {
        Subject {
            issuer: ISSUER,
            user_id: Uuid::from_u128(1),
            org_id: Uuid::from_u128(2),
            client_id: "stepclimb-app",
            family_id: Uuid::from_u128(3),
        }
    }

    fn issued(ring: &KeyRing) -> String {
        issue(ring, &subject(), at(0))
            .unwrap()
            .expose_secret()
            .to_owned()
    }

    fn encode(value: &serde_json::Value) -> String {
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(value).unwrap())
    }

    /// A token with `header` and `payload` as given, signed under `ring`'s current key.
    fn forged(ring: &KeyRing, header: &serde_json::Value, payload: &serde_json::Value) -> String {
        let input = format!("{}.{}", encode(header), encode(payload));
        let (_, tag) = ring.sign_access_token(input.as_bytes());
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(tag))
    }

    fn good_payload() -> serde_json::Value {
        serde_json::to_value(Claims {
            iss: ISSUER.into(),
            sub: Uuid::from_u128(1),
            org: Uuid::from_u128(2),
            client_id: "stepclimb-app".into(),
            fid: Uuid::from_u128(3),
            iat: 1_763_000_000,
            exp: 1_763_000_900,
        })
        .unwrap()
    }

    #[test]
    fn an_issued_token_verifies_and_carries_its_subject() {
        let ring = keys(KEY_A);
        let claims = verify(&ring, &issued(&ring), ISSUER, at(0)).unwrap();
        assert_eq!(claims.sub, Uuid::from_u128(1));
        assert_eq!(claims.org, Uuid::from_u128(2));
        assert_eq!(claims.client_id, "stepclimb-app");
        assert_eq!(claims.fid, Uuid::from_u128(3));
        assert_eq!(claims.exp - claims.iat, 900);
    }

    #[test]
    fn t31_a_token_works_until_fifteen_minutes_and_not_at_them() {
        let ring = keys(KEY_A);
        let token = issued(&ring);
        assert!(verify(&ring, &token, ISSUER, at(899)).is_ok());
        assert!(verify(&ring, &token, ISSUER, at(899) + Duration::from_millis(999)).is_ok());
        assert_eq!(
            verify(&ring, &token, ISSUER, at(900)),
            Err(Refusal::Expired)
        );
    }

    #[test]
    fn the_header_is_three_known_members() {
        let ring = keys(KEY_A);
        for header in [
            serde_json::json!({"alg": "none", "typ": "JWT", "kid": "a"}),
            serde_json::json!({"alg": "HS512", "typ": "JWT", "kid": "a"}),
            serde_json::json!({"alg": "hs256", "typ": "JWT", "kid": "a"}),
            serde_json::json!({"alg": "HS256", "typ": "at+jwt", "kid": "a"}),
            serde_json::json!({"alg": "HS256", "kid": "a"}),
            serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "b"}),
            serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a", "crit": ["x"]}),
        ] {
            let token = forged(&ring, &header, &good_payload());
            assert_eq!(
                verify(&ring, &token, ISSUER, at(0)),
                Err(Refusal::Invalid),
                "{header}"
            );
        }
        let good = forged(
            &ring,
            &serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a"}),
            &good_payload(),
        );
        assert!(verify(&ring, &good, ISSUER, at(0)).is_ok());
    }

    #[test]
    fn a_payload_with_another_issuer_or_member_is_invalid() {
        let ring = keys(KEY_A);
        let header = serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a"});
        let mut other_issuer = good_payload();
        other_issuer["iss"] = "https://other.example".into();
        let mut extra = good_payload();
        extra["scope"] = "admin".into();
        let mut missing = good_payload();
        missing.as_object_mut().unwrap().remove("fid");
        for payload in [other_issuer, extra, missing] {
            let token = forged(&ring, &header, &payload);
            assert_eq!(
                verify(&ring, &token, ISSUER, at(0)),
                Err(Refusal::Invalid),
                "{payload}"
            );
        }
    }

    #[test]
    fn a_changed_or_foreign_signature_is_invalid() {
        let ring = keys(KEY_A);
        let token = issued(&ring);
        let (input, signature) = token.rsplit_once('.').unwrap();
        let mut tag = URL_SAFE_NO_PAD.decode(signature).unwrap();
        tag[5] ^= 1;
        let altered = format!("{input}.{}", URL_SAFE_NO_PAD.encode(&tag));
        let (header, payload) = input.split_once('.').unwrap();
        let mut claims = good_payload();
        claims["sub"] = serde_json::to_value(Uuid::from_u128(9)).unwrap();
        let swapped = format!("{header}.{}.{signature}", encode(&claims));
        let other_key = issue(
            &keys("a:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8="),
            &subject(),
            at(0),
        )
        .unwrap()
        .expose_secret()
        .to_owned();
        for token in [
            altered,
            swapped,
            other_key,
            format!("{header}.{payload}."),
            format!("{header}.{payload}"),
            format!("{token}.x"),
            String::new(),
            "x".repeat(MAX_LEN + 1),
        ] {
            assert_eq!(
                verify(&ring, &token, ISSUER, at(0)),
                Err(Refusal::Invalid),
                "{token}"
            );
        }
    }

    #[test]
    fn an_expired_token_with_a_bad_signature_is_invalid_not_expired() {
        let ring = keys(KEY_A);
        let token = issued(&keys(KEY_B));
        assert_eq!(
            verify(&ring, &token, ISSUER, at(10_000)),
            Err(Refusal::Invalid)
        );
    }

    #[test]
    fn a_token_signed_before_a_rotation_still_verifies() {
        let token = issued(&keys(KEY_A));
        let rotated = keys(&format!("{KEY_B},{KEY_A}"));
        assert!(verify(&rotated, &token, ISSUER, at(0)).is_ok());
        assert!(verify(&rotated, &issued(&rotated), ISSUER, at(0)).is_ok());
        assert_eq!(
            verify(&keys(KEY_B), &token, ISSUER, at(0)),
            Err(Refusal::Invalid)
        );
    }

    #[test]
    fn debug_output_hides_the_token() {
        let token = issue(&keys(KEY_A), &subject(), at(0)).unwrap();
        assert_eq!(format!("{token:?}"), "AccessToken(<redacted>)");
    }
}
