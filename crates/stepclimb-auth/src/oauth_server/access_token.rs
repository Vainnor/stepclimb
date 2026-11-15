// SPDX-License-Identifier: AGPL-3.0-or-later
//! App access tokens (docs/specs/m0/08-sessions-and-csrf.md B32, B33, B36, B46, Q12; ADR 0032).
//!
//! An access token is a JWT (RFC 7519) in the compact JWS form, signed HS256 with `jsonwebtoken`
//! (plan, Technical -> Decided) under the key ring's access token key
//! (`encryption::KeyRing::current_access_token_key`): only this server signs and checks them. The
//! header is exactly `alg`, `typ` and `kid`; the payload is exactly the [`Claims`] below. A token
//! lives [`LIFETIME`].
//!
//! `jsonwebtoken` runs on no crypto backend of its own: the first call here installs
//! `PROVIDER` as its process-wide `CryptoProvider`, which signs and verifies HS256 only, with the
//! workspace's `hmac` and `sha2`, and refuses every other algorithm. If a different provider was
//! installed first, every token is refused and none is issued.
//!
//! [`verify`] reads a presented token in this order and stops at the first problem, before any
//! database query: its length; the header, which must be exactly `alg: "HS256"`, `typ: "JWT"`
//! and the `kid` of a key in the ring; then `jsonwebtoken::decode` with HS256 alone allowed, which
//! checks the signature in constant time, reads the payload into [`Claims`] (no other member) and
//! requires `exp`, `iss` and `sub`, with `iss` this server's issuer and no `aud`. Last, `exp` is
//! compared with the clock the caller passes, not the system time the crate would read. A token
//! past its `exp` is [`Refusal::Expired`]; every other problem is [`Refusal::Invalid`], which says
//! nothing about which check failed. The server then checks the organization, the client, the
//! user's status and that the refresh family still exists, so revoking a family stops its access
//! tokens at once (Q12).
//!
//! [`AccessToken`] has no `Display` and a redacted `Debug`, so a token reaches no log line.

use std::fmt;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, KeyInit, Mac};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation,
    crypto::{CryptoProvider, JwtSigner, JwtVerifier, KeyUtils},
    errors::ErrorKind,
    signature,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::encryption::KeyRing;

/// How long an access token authenticates requests after it is issued (plan: 15 minutes).
pub const LIFETIME: Duration = Duration::from_secs(15 * 60);

/// The longest token text [`verify`] reads. An issued token is about 400 bytes.
pub const MAX_LEN: usize = 2048;

/// The one algorithm tokens are signed and checked with.
const ALGORITHM: Algorithm = Algorithm::HS256;

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

/// Why [`issue`] made no token.
#[derive(Debug, thiserror::Error)]
pub enum IssueError {
    /// `now` cannot be written as seconds since the Unix epoch (before 1970).
    #[error("the clock reads a time before the Unix epoch")]
    ClockBeforeEpoch,
    /// Another `jsonwebtoken` crypto provider was installed in this process before this one.
    #[error("another jsonwebtoken crypto provider is installed; access tokens cannot be signed")]
    ForeignProvider,
    #[error("the access token could not be signed: {0}")]
    Sign(#[from] jsonwebtoken::errors::Error),
}

/// Sign a token for `subject`, issued at `now` and expiring [`LIFETIME`] later.
pub fn issue(
    keys: &KeyRing,
    subject: &Subject<'_>,
    now: SystemTime,
) -> Result<AccessToken, IssueError> {
    if !provider_installed() {
        return Err(IssueError::ForeignProvider);
    }
    let iat = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IssueError::ClockBeforeEpoch)?
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
    let (kid, key) = keys.current_access_token_key();
    let text = jsonwebtoken::encode(&header(kid), &claims, &EncodingKey::from_secret(key))?;
    Ok(AccessToken(text))
}

/// The one header a token may carry: `alg: "HS256"`, `typ: "JWT"` and `kid`.
fn header(kid: &str) -> Header {
    Header {
        kid: Some(kid.to_owned()),
        ..Header::new(ALGORITHM)
    }
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
    if presented.len() > MAX_LEN || !provider_installed() {
        return Err(Refusal::Invalid);
    }
    let presented_header = jsonwebtoken::decode_header(presented).map_err(|_| Refusal::Invalid)?;
    let kid = presented_header.kid.as_deref().ok_or(Refusal::Invalid)?;
    if presented_header != header(kid) {
        return Err(Refusal::Invalid);
    }
    let key = keys.access_token_key(kid).ok_or(Refusal::Invalid)?;
    let decoded = jsonwebtoken::decode::<Claims>(
        presented,
        &DecodingKey::from_secret(key),
        &validation(issuer),
    )
    .map_err(|_| Refusal::Invalid)?;
    let claims = decoded.claims;
    let expires = UNIX_EPOCH
        .checked_add(Duration::from_secs(claims.exp))
        .ok_or(Refusal::Invalid)?;
    if now >= expires {
        return Err(Refusal::Expired);
    }
    Ok(claims)
}

/// HS256 alone; `exp`, `iss` and `sub` required, `iss` this server's; any `aud` refused (none is
/// expected). The crate's own expiry and not-before checks are off, since they read the system
/// time: [`verify`] compares `exp` with the clock it is given.
fn validation(issuer: &str) -> Validation {
    let mut validation = Validation::new(ALGORITHM);
    validation.validate_exp = false;
    validation.validate_nbf = false;
    validation.leeway = 0;
    validation.validate_aud = true;
    validation.set_required_spec_claims(&["exp", "iss", "sub"]);
    validation.set_issuer(&[issuer]);
    validation
}

/// The `jsonwebtoken` crypto provider: HS256 only, over the workspace's `hmac` and `sha2`.
static PROVIDER: CryptoProvider = CryptoProvider {
    signer_factory,
    verifier_factory,
    key_utils: KeyUtils::new_unimplemented(),
};

/// Whether [`PROVIDER`] is this process's `jsonwebtoken` provider, installing it on the first
/// call. `false` when another provider was installed before it.
fn provider_installed() -> bool {
    static INSTALLED: OnceLock<bool> = OnceLock::new();
    *INSTALLED.get_or_init(|| PROVIDER.install_default().is_ok())
}

type HmacSha256 = Hmac<Sha256>;

fn hs256(algorithm: &Algorithm, key: &[u8]) -> jsonwebtoken::errors::Result<HmacSha256> {
    if *algorithm != ALGORITHM {
        return Err(ErrorKind::InvalidAlgorithm.into());
    }
    HmacSha256::new_from_slice(key).map_err(|_| ErrorKind::InvalidKeyFormat.into())
}

fn signer_factory(
    algorithm: &Algorithm,
    key: &EncodingKey,
) -> jsonwebtoken::errors::Result<Box<dyn JwtSigner>> {
    Ok(Box::new(Hs256(hs256(algorithm, key.as_bytes())?)))
}

fn verifier_factory(
    algorithm: &Algorithm,
    key: &DecodingKey,
) -> jsonwebtoken::errors::Result<Box<dyn JwtVerifier>> {
    Ok(Box::new(Hs256(hs256(algorithm, key.try_get_as_bytes()?)?)))
}

/// HMAC-SHA-256 under one key, for both directions.
struct Hs256(HmacSha256);

impl signature::Signer<Vec<u8>> for Hs256 {
    fn try_sign(&self, message: &[u8]) -> Result<Vec<u8>, signature::Error> {
        Ok(self
            .0
            .clone()
            .chain_update(message)
            .finalize()
            .into_bytes()
            .to_vec())
    }
}

impl signature::Verifier<Vec<u8>> for Hs256 {
    /// The tag is compared in constant time (`Mac::verify_slice`).
    fn verify(&self, message: &[u8], tag: &Vec<u8>) -> Result<(), signature::Error> {
        self.0
            .clone()
            .chain_update(message)
            .verify_slice(tag)
            .map_err(|_| signature::Error::new())
    }
}

impl JwtSigner for Hs256 {
    fn algorithm(&self) -> Algorithm {
        ALGORITHM
    }
}

impl JwtVerifier for Hs256 {
    fn algorithm(&self) -> Algorithm {
        ALGORITHM
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine as _;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

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

    /// A token with `header` and `payload` as given, signed HS256 under `ring`'s current access
    /// token key without this module, so a test can build what [`issue`] never would.
    fn forged(ring: &KeyRing, header: &serde_json::Value, payload: &serde_json::Value) -> String {
        let input = format!("{}.{}", encode(header), encode(payload));
        let (_, key) = ring.current_access_token_key();
        let tag = HmacSha256::new_from_slice(key)
            .unwrap()
            .chain_update(input.as_bytes())
            .finalize()
            .into_bytes();
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(tag))
    }

    fn good_header() -> serde_json::Value {
        serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a"})
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
    fn an_issued_token_has_exactly_the_three_header_members() {
        let token = issued(&keys(KEY_A));
        let header = token.split('.').next().unwrap();
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header).unwrap()).unwrap();
        assert_eq!(header, good_header());
    }

    #[test]
    fn a_token_built_without_this_module_verifies_the_same() {
        let ring = keys(KEY_A);
        let token = forged(&ring, &good_header(), &good_payload());
        assert_eq!(
            verify(&ring, &token, ISSUER, at(0)),
            Ok(serde_json::from_value(good_payload()).unwrap())
        );
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
            serde_json::json!({"alg": "HS384", "typ": "JWT", "kid": "a"}),
            serde_json::json!({"alg": "hs256", "typ": "JWT", "kid": "a"}),
            serde_json::json!({"alg": "RS256", "typ": "JWT", "kid": "a"}),
            serde_json::json!({"alg": "HS256", "typ": "at+jwt", "kid": "a"}),
            serde_json::json!({"alg": "HS256", "kid": "a"}),
            serde_json::json!({"alg": "HS256", "typ": "JWT"}),
            serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "b"}),
            serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a", "crit": ["x"]}),
            serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a", "cty": "JWT"}),
            serde_json::json!({"alg": "HS256", "typ": "JWT", "kid": "a", "x": 1}),
        ] {
            let token = forged(&ring, &header, &good_payload());
            assert_eq!(
                verify(&ring, &token, ISSUER, at(0)),
                Err(Refusal::Invalid),
                "{header}"
            );
        }
    }

    #[test]
    fn a_payload_with_another_issuer_or_member_is_invalid() {
        let ring = keys(KEY_A);
        let mut other_issuer = good_payload();
        other_issuer["iss"] = "https://other.example".into();
        let mut extra = good_payload();
        extra["scope"] = "admin".into();
        let mut audience = good_payload();
        audience["aud"] = "stepclimb-app".into();
        let mut missing = good_payload();
        missing.as_object_mut().unwrap().remove("fid");
        let mut no_exp = good_payload();
        no_exp.as_object_mut().unwrap().remove("exp");
        for payload in [other_issuer, extra, audience, missing, no_exp] {
            let token = forged(&ring, &good_header(), &payload);
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
        let other_key = issued(&keys("a:ICEiIyQlJicoKSorLC0uLzAxMjM0NTY3ODk6Ozw9Pj8="));
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
    fn the_provider_refuses_every_algorithm_but_hs256() {
        let key = EncodingKey::from_secret(b"k");
        for algorithm in [
            Algorithm::HS384,
            Algorithm::HS512,
            Algorithm::RS256,
            Algorithm::ES256,
            Algorithm::EdDSA,
        ] {
            assert!(signer_factory(&algorithm, &key).is_err(), "{algorithm:?}");
            assert!(
                verifier_factory(&algorithm, &DecodingKey::from_secret(b"k")).is_err(),
                "{algorithm:?}"
            );
        }
        assert!(signer_factory(&Algorithm::HS256, &key).is_ok());
    }

    #[test]
    fn debug_output_hides_the_token() {
        let token = issue(&keys(KEY_A), &subject(), at(0)).unwrap();
        assert_eq!(format!("{token:?}"), "AccessToken(<redacted>)");
    }
}
