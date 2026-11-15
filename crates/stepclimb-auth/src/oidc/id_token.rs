// SPDX-License-Identifier: AGPL-3.0-or-later
//! ID token validation (`docs/specs/m0/05-oidc.md` B10, Q5, Q8; ADR 0029).
//!
//! The token is a JWS in compact serialization (RFC 7515 7.1). [`verify_id_token`] checks, in
//! order, and refuses at the first failure:
//!
//! 1. the shape: three base64url parts, a JSON header and a JSON object payload; a header with
//!    `crit`, `cty` or a `typ` other than `JWT` is refused, so a nested or encrypted token or an
//!    access token in JWT form (`at+jwt`) is not read as an ID token;
//! 2. the algorithm: `alg: none` is refused, and so is any algorithm outside the accepted list
//!    of [`accepted_algorithms`] (Q8: asymmetric only, as the discovery document lists them);
//! 3. the signature, with the one key of the provider's key set whose `kid` is the token's and
//!    whose type, use and algorithm fit (no key is [`IdTokenError::UnknownKey`], which the
//!    callback answers with one refetch of the key set);
//! 4. `iss` equals the configured issuer, character for character;
//! 5. `aud` contains the client id; `azp`, when present, equals the client id, and it must be
//!    present when `aud` has more than one value;
//! 6. `exp` is after now, `iat` is not after now and `nbf`, when present, is not after now, each
//!    allowing [`CLOCK_SKEW`], where now is the caller's clock;
//! 7. `nonce` equals the flow's nonce, compared in constant time;
//! 8. `at_hash`, when present, is the left half of the access token's hash under the
//!    signature's hash function, base64url (OIDC Core 3.1.3.6);
//! 9. `sub` is a string an identity can hold (B12): not empty, no NUL, at most
//!    [`MAX_SUBJECT_CHARS`] characters.
//!
//! The claims are read as plain JSON, not as `openidconnect`'s typed claims, so a profile claim
//! of an unexpected type reaches the claims mapping as it is (B13) instead of refusing the
//! token. The signature check and the hash are `openidconnect`'s (`CoreJsonWebKey`).
//!
//! No error carries the token, a claim value or a key.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use openidconnect::core::{CoreJsonWebKey, CoreJwsSigningAlgorithm};
use openidconnect::{JsonWebKey, JsonWebKeyAlgorithm, JsonWebKeyUse, JwsSigningAlgorithm};
use serde_json::{Map, Value};
use subtle::ConstantTimeEq;

use super::ProviderMetadata;
use super::jwks::KeySet;

/// How far the provider's clock may be from the server's for `exp`, `iat` and `nbf` (Q5).
pub const CLOCK_SKEW: Duration = Duration::from_secs(60);

/// The longest subject an identity holds, in characters: `stepclimb_core::identities`'
/// `MAX_SUBJECT_CHARS`, which `identities.subject` is checked against. The same rule as the
/// OAuth 2 subject check of `06-oauth2-and-vatsim-preset.md` B6.
pub const MAX_SUBJECT_CHARS: usize = 255;

/// Whether `subject` can be an identity's subject: not empty, no NUL, at most
/// [`MAX_SUBJECT_CHARS`] characters.
fn usable_subject(subject: &str) -> bool {
    !subject.is_empty() && !subject.contains('\0') && subject.chars().count() <= MAX_SUBJECT_CHARS
}

/// The asymmetric algorithms `openidconnect` verifies (Q8). HMAC (`HS*`), `ES512` and `none`
/// are never accepted.
const ASYMMETRIC: [CoreJwsSigningAlgorithm; 9] = [
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha384,
    CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha512,
    CoreJwsSigningAlgorithm::RsaSsaPssSha256,
    CoreJwsSigningAlgorithm::RsaSsaPssSha384,
    CoreJwsSigningAlgorithm::RsaSsaPssSha512,
    CoreJwsSigningAlgorithm::EcdsaP256Sha256,
    CoreJwsSigningAlgorithm::EcdsaP384Sha384,
    CoreJwsSigningAlgorithm::EdDsa,
];

/// The algorithms the server accepts from this provider (Q8): those of [`ASYMMETRIC`] that the
/// discovery document lists in `id_token_signing_alg_values_supported`.
#[must_use]
pub fn accepted_algorithms(metadata: &ProviderMetadata) -> Vec<CoreJwsSigningAlgorithm> {
    let listed = metadata.id_token_signing_alg_values_supported();
    ASYMMETRIC
        .into_iter()
        .filter(|alg| listed.contains(alg))
        .collect()
}

/// What the token must match.
pub struct Expected<'a> {
    /// The configured issuer, exactly as written.
    pub issuer: &'a str,
    pub client_id: &'a str,
    /// The flow's nonce. A secret until the flow ends: never log it.
    pub nonce: &'a str,
    /// The access token the same response carried, for `at_hash`. Never log it.
    pub access_token: &'a str,
    /// From [`accepted_algorithms`].
    pub algorithms: &'a [CoreJwsSigningAlgorithm],
}

impl std::fmt::Debug for Expected<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expected")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .field("algorithms", &self.algorithms)
            .finish_non_exhaustive()
    }
}

/// An ID token that passed every check: its `sub` and its whole payload.
#[derive(Clone, PartialEq)]
pub struct VerifiedIdToken {
    pub subject: String,
    /// Every claim of the payload, as JSON.
    pub claims: Map<String, Value>,
}

impl std::fmt::Debug for VerifiedIdToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedIdToken")
            .field("claims", &self.claims.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

/// Why an ID token was refused (B10). Every one but [`IdTokenError::Subject`] is
/// `auth.id_token_invalid` to the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdTokenError {
    #[error("the ID token is not a signed JWT the server reads")]
    Malformed,
    #[error("the ID token is unsigned (alg: none)")]
    Unsigned,
    #[error("the ID token's algorithm is not one the server accepts from this provider")]
    Algorithm,
    #[error("the provider's key set has no key for the ID token's kid and algorithm")]
    UnknownKey,
    #[error("the provider's key set has more than one key that fits the ID token")]
    AmbiguousKey,
    #[error("the ID token's signature does not verify")]
    Signature,
    #[error("the ID token's iss is not the configured issuer")]
    Issuer,
    #[error("the ID token's aud does not contain the client id")]
    Audience,
    #[error("the ID token's azp is missing or not the client id")]
    AuthorizedParty,
    #[error("the ID token has expired")]
    Expired,
    #[error("the ID token's iat is in the future")]
    IssuedInFuture,
    #[error("the ID token's nbf is in the future")]
    NotYetValid,
    #[error("the ID token's nonce is not the flow's")]
    Nonce,
    #[error("the ID token's at_hash does not match the access token")]
    AtHash,
    /// `auth.subject_missing` to the user, not `auth.id_token_invalid`.
    #[error("the ID token's sub is missing, empty, holds NUL or is too long")]
    Subject,
}

/// A compact JWS split and decoded.
struct Jws<'a> {
    header: Map<String, Value>,
    payload: Map<String, Value>,
    /// `<header>.<payload>` as received: what the signature covers.
    signing_input: &'a str,
    signature: Vec<u8>,
}

/// Check `token` as the module documentation lists, with the keys of `keys`, at `now`.
pub fn verify_id_token(
    token: &str,
    keys: &KeySet,
    expected: &Expected<'_>,
    now: SystemTime,
) -> Result<VerifiedIdToken, IdTokenError> {
    let jws = split(token)?;
    let alg = algorithm(&jws.header, expected.algorithms)?;
    let key = signing_key(keys, &jws.header, &alg)?;
    key.verify_signature(&alg, jws.signing_input.as_bytes(), &jws.signature)
        .map_err(|_| IdTokenError::Signature)?;
    let claims = jws.payload;
    check_issuer(&claims, expected.issuer)?;
    check_audience(&claims, expected.client_id)?;
    check_times(&claims, now)?;
    check_nonce(&claims, expected.nonce)?;
    check_at_hash(&claims, key, &alg, expected.access_token)?;
    let subject = match claims.get("sub") {
        Some(Value::String(sub)) if usable_subject(sub) => sub.clone(),
        _ => return Err(IdTokenError::Subject),
    };
    Ok(VerifiedIdToken { subject, claims })
}

fn split(token: &str) -> Result<Jws<'_>, IdTokenError> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(IdTokenError::Malformed);
    };
    let signing_input = &token[..header.len() + 1 + payload.len()];
    Ok(Jws {
        header: json_object(header)?,
        payload: json_object(payload)?,
        signing_input,
        signature: URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| IdTokenError::Malformed)?,
    })
}

fn json_object(part: &str) -> Result<Map<String, Value>, IdTokenError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|_| IdTokenError::Malformed)?;
    match serde_json::from_slice(&bytes) {
        Ok(Value::Object(object)) => Ok(object),
        _ => Err(IdTokenError::Malformed),
    }
}

/// The header's `alg`, when it is one of `accepted`, after the header checks of step 1.
fn algorithm(
    header: &Map<String, Value>,
    accepted: &[CoreJwsSigningAlgorithm],
) -> Result<CoreJwsSigningAlgorithm, IdTokenError> {
    if header.contains_key("crit") || header.contains_key("cty") || header.contains_key("enc") {
        return Err(IdTokenError::Malformed);
    }
    match header.get("typ") {
        None => {}
        Some(Value::String(typ))
            if typ.eq_ignore_ascii_case("JWT") || typ.eq_ignore_ascii_case("application/jwt") => {}
        Some(_) => return Err(IdTokenError::Malformed),
    }
    let Some(Value::String(name)) = header.get("alg") else {
        return Err(IdTokenError::Malformed);
    };
    if name == "none" {
        return Err(IdTokenError::Unsigned);
    }
    let alg: CoreJwsSigningAlgorithm =
        serde_json::from_value(Value::String(name.clone())).map_err(|_| IdTokenError::Algorithm)?;
    if !accepted.contains(&alg) {
        return Err(IdTokenError::Algorithm);
    }
    Ok(alg)
}

/// The one key of `keys` with the header's `kid` (any key when the header has none) whose type,
/// use and algorithm fit `alg`.
fn signing_key<'k>(
    keys: &'k KeySet,
    header: &Map<String, Value>,
    alg: &CoreJwsSigningAlgorithm,
) -> Result<&'k CoreJsonWebKey, IdTokenError> {
    let kid = match header.get("kid") {
        None => None,
        Some(Value::String(kid)) => Some(kid.as_str()),
        Some(_) => return Err(IdTokenError::Malformed),
    };
    let mut fitting = keys.keys().iter().filter(|key| {
        let kid_matches = kid.is_none_or(|kid| key.key_id().is_some_and(|id| id.as_str() == kid));
        kid_matches && fits(key, alg)
    });
    match (fitting.next(), fitting.next()) {
        (None, _) => Err(IdTokenError::UnknownKey),
        (Some(key), None) => Ok(key),
        (Some(_), Some(_)) => Err(IdTokenError::AmbiguousKey),
    }
}

/// Whether `key` may verify a signature made with `alg` (RFC 7517 4.2, 4.4).
fn fits(key: &CoreJsonWebKey, alg: &CoreJwsSigningAlgorithm) -> bool {
    let use_allows = key
        .key_use()
        .is_none_or(|key_use| key_use.allows_signature());
    let type_matches = alg.key_type().as_ref() == Some(key.key_type());
    let alg_matches = match key.signing_alg() {
        JsonWebKeyAlgorithm::Unspecified => true,
        JsonWebKeyAlgorithm::Algorithm(key_alg) => key_alg == alg,
        JsonWebKeyAlgorithm::Unsupported => false,
    };
    use_allows && type_matches && alg_matches
}

fn check_issuer(claims: &Map<String, Value>, issuer: &str) -> Result<(), IdTokenError> {
    match claims.get("iss") {
        Some(Value::String(iss)) if iss == issuer => Ok(()),
        _ => Err(IdTokenError::Issuer),
    }
}

/// `aud` (a string or an array of strings) contains `client_id`; `azp`, when present, is
/// `client_id`, and must be present when `aud` has more than one value.
fn check_audience(claims: &Map<String, Value>, client_id: &str) -> Result<(), IdTokenError> {
    let audiences: Vec<&str> = match claims.get("aud") {
        Some(Value::String(aud)) => vec![aud.as_str()],
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_str().ok_or(IdTokenError::Audience))
            .collect::<Result<_, _>>()?,
        _ => return Err(IdTokenError::Audience),
    };
    if !audiences.contains(&client_id) {
        return Err(IdTokenError::Audience);
    }
    match claims.get("azp") {
        None if audiences.len() == 1 => Ok(()),
        Some(Value::String(azp)) if azp == client_id => Ok(()),
        _ => Err(IdTokenError::AuthorizedParty),
    }
}

/// `exp` after `now`, `iat` and `nbf` not after it, each by up to [`CLOCK_SKEW`].
fn check_times(claims: &Map<String, Value>, now: SystemTime) -> Result<(), IdTokenError> {
    let now = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| IdTokenError::Expired)?
        .as_secs_f64();
    let skew = CLOCK_SKEW.as_secs_f64();
    let exp = numeric_date(claims, "exp")?.ok_or(IdTokenError::Malformed)?;
    if exp + skew <= now {
        return Err(IdTokenError::Expired);
    }
    let iat = numeric_date(claims, "iat")?.ok_or(IdTokenError::Malformed)?;
    if iat > now + skew {
        return Err(IdTokenError::IssuedInFuture);
    }
    if let Some(nbf) = numeric_date(claims, "nbf")?
        && nbf > now + skew
    {
        return Err(IdTokenError::NotYetValid);
    }
    Ok(())
}

/// A NumericDate claim (RFC 7519 2): seconds since the epoch, possibly fractional. Absent is
/// `None`; any other JSON type is malformed.
fn numeric_date(claims: &Map<String, Value>, name: &str) -> Result<Option<f64>, IdTokenError> {
    match claims.get(name) {
        None => Ok(None),
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|secs| secs.is_finite())
            .map(Some)
            .ok_or(IdTokenError::Malformed),
        Some(_) => Err(IdTokenError::Malformed),
    }
}

fn check_nonce(claims: &Map<String, Value>, nonce: &str) -> Result<(), IdTokenError> {
    match claims.get("nonce") {
        Some(Value::String(claimed)) if bool::from(claimed.as_bytes().ct_eq(nonce.as_bytes())) => {
            Ok(())
        }
        _ => Err(IdTokenError::Nonce),
    }
}

/// When the token carries `at_hash`, it must be the access token's (B10 6).
fn check_at_hash(
    claims: &Map<String, Value>,
    key: &CoreJsonWebKey,
    alg: &CoreJwsSigningAlgorithm,
    access_token: &str,
) -> Result<(), IdTokenError> {
    let claimed = match claims.get("at_hash") {
        None => return Ok(()),
        Some(Value::String(claimed)) => claimed,
        Some(_) => return Err(IdTokenError::AtHash),
    };
    let hash = key
        .hash_bytes(access_token.as_bytes(), alg)
        .map_err(|_| IdTokenError::AtHash)?;
    let expected = URL_SAFE_NO_PAD.encode(&hash[..hash.len() / 2]);
    if bool::from(claimed.as_bytes().ct_eq(expected.as_bytes())) {
        Ok(())
    } else {
        Err(IdTokenError::AtHash)
    }
}
